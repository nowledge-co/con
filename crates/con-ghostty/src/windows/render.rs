//! D3D11 + DirectWrite renderer for the Windows terminal pane.
//!
//! Pipeline:
//!
//! 1. **Device** — `D3D11CreateDevice` with `D3D_DRIVER_TYPE_HARDWARE`,
//!    falling back to WARP, falling back to reference. Single device per
//!    `WindowsTerminalView`.
//! 2. **Swapchain** — `IDXGIFactory6::CreateSwapChainForHwnd` against
//!    the WS_CHILD HWND owned by [`super::host_view`]. (Future: swap to
//!    `CreateSwapChainForCompositionSurfaceHandle` for the upstream
//!    DComp-handoff design.)
//! 3. **Glyph atlas** — DirectWrite (`IDWriteFactory`) measures and
//!    rasterizes glyph runs into an `ID3D11Texture2D` of monochrome
//!    coverage. Lazy: each (codepoint, attrs) pair is rasterized on
//!    first sight, cached by key.
//! 4. **Grid render** — for each visible cell: emit one quad with two
//!    attributes (background color, glyph atlas UV). A single draw
//!    call per frame using a vertex buffer; instance count = rows*cols.
//! 5. **Present** — `swapchain.Present(1, 0)` (vsync on) then DComp
//!    Commit (when in DComp swapchain mode).
//!
//! Status: scaffolding + Clear-to-color implementation. The full
//! glyph atlas + grid draw path is the next focused PR — that work
//! lives entirely within this file.

use anyhow::{Context, Result};
use parking_lot::Mutex;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_RENDER_TARGET_VIEW_DESC,
    D3D11_RTV_DIMENSION_TEXTURE2D, D3D11_SDK_VERSION, D3D11_TEX2D_RTV, D3D11CreateDevice,
    ID3D11Device, ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Texture2D,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWriteCreateFactory, IDWriteFactory,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIFactory6, IDXGISwapChain1,
};

use super::vt::ScreenSnapshot;

/// One renderer instance per terminal pane. Holds the device, swapchain,
/// glyph cache, and font factory.
pub struct Renderer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    swapchain: IDXGISwapChain1,
    /// `Option` so we can drop the RTV around `ResizeBuffers` (DXGI
    /// rejects the call if any backbuffer view is still alive).
    rtv: Option<ID3D11RenderTargetView>,
    dwrite: IDWriteFactory,
    /// Last-rendered ScreenSnapshot generation; used to skip
    /// re-rendering identical frames.
    last_generation: Mutex<u64>,
    /// Window backing dimensions in physical pixels (post-DPI).
    width_px: u32,
    height_px: u32,
}

unsafe impl Send for Renderer {}
unsafe impl Sync for Renderer {}

/// Public renderer config — font, sizes, colors.
#[derive(Debug, Clone)]
pub struct RendererConfig {
    pub font_family: String,
    pub font_size_px: f32,
    pub initial_width: u32,
    pub initial_height: u32,
    pub clear_color: [f32; 4], // RGBA premultiplied
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            font_family: "Cascadia Mono".to_string(),
            font_size_px: 14.0,
            initial_width: 800,
            initial_height: 600,
            clear_color: [0.06, 0.06, 0.07, 1.0],
        }
    }
}

impl Renderer {
    /// Create a renderer that presents to the given child HWND.
    pub fn new(hwnd: HWND, config: &RendererConfig) -> Result<Self> {
        let (device, context) = create_device()?;
        let dxgi_factory: IDXGIFactory6 =
            unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }
                .context("CreateDXGIFactory2 failed")?;

        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: config.initial_width,
            Height: config.initial_height,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            Flags: 0,
        };

        // SAFETY: hwnd is owned by the host_view caller; device & desc
        // are valid for the call.
        let swapchain: IDXGISwapChain1 = unsafe {
            dxgi_factory.CreateSwapChainForHwnd(&device, hwnd, &desc, None, None)
        }
        .context("CreateSwapChainForHwnd failed")?;

        let rtv = Some(create_rtv(&device, &swapchain)?);
        let dwrite: IDWriteFactory =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }
                .context("DWriteCreateFactory failed")?;

        Ok(Self {
            device,
            context,
            swapchain,
            rtv,
            dwrite,
            last_generation: Mutex::new(0),
            width_px: config.initial_width,
            height_px: config.initial_height,
        })
    }

    /// Resize the swapchain backing buffers. Call from `WM_SIZE`.
    pub fn resize(&mut self, width_px: u32, height_px: u32) -> Result<()> {
        if width_px == 0 || height_px == 0 {
            return Ok(());
        }
        // Drop the RTV before resizing or DXGI will reject the call.
        self.rtv = None;

        // SAFETY: ResizeBuffers takes the new dims and matches the
        // existing buffer count via 0.
        unsafe {
            self.swapchain.ResizeBuffers(
                0,
                width_px,
                height_px,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_SWAP_CHAIN_FLAG(0),
            )
        }
        .context("ResizeBuffers failed")?;

        self.rtv = Some(create_rtv(&self.device, &self.swapchain)?);
        self.width_px = width_px;
        self.height_px = height_px;
        Ok(())
    }

    /// Render one frame from a [`ScreenSnapshot`]. Skips work if the
    /// snapshot generation matches the last-rendered one.
    pub fn render(&self, snapshot: &ScreenSnapshot, config: &RendererConfig) -> Result<()> {
        {
            let mut last = self.last_generation.lock();
            if *last == snapshot.generation {
                return Ok(());
            }
            *last = snapshot.generation;
        }

        // SAFETY: rtv and context are owned by self; ClearRenderTargetView
        // is the simplest valid scene we can render today.
        let Some(rtv) = self.rtv.as_ref() else {
            return Ok(()); // Resize in progress; skip this frame.
        };
        unsafe {
            self.context
                .OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            self.context.ClearRenderTargetView(rtv, &config.clear_color);
        }

        // TODO(phase-3-render): emit one quad per cell, sample glyph atlas.
        // The full pipeline is staged as the next focused PR within Phase 3.
        // The skeleton above proves: device init, swapchain, RTV, present.
        let _ = (snapshot, &self.dwrite); // silence dead-code

        // SAFETY: swapchain is valid; Present(1, 0) = vsync on, no flags.
        unsafe { self.swapchain.Present(1, DXGI_PRESENT(0)) }
            .ok()
            .context("swapchain Present failed")?;
        Ok(())
    }

    pub fn dimensions_px(&self) -> (u32, u32) {
        (self.width_px, self.height_px)
    }

    /// Compute the cell grid size that fits in the current swapchain
    /// dimensions for a given font. Used by host_view to drive
    /// `ResizePseudoConsole`. Stub: assumes 8x16 cells until DirectWrite
    /// metrics are wired up in the next render PR.
    pub fn grid_for_dimensions(&self, _config: &RendererConfig) -> (u16, u16) {
        const STUB_CELL_W: u32 = 8;
        const STUB_CELL_H: u32 = 16;
        let cols = (self.width_px / STUB_CELL_W).max(1) as u16;
        let rows = (self.height_px / STUB_CELL_H).max(1) as u16;
        (cols, rows)
    }
}

fn create_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut context = None;
    let mut feature_level = D3D_FEATURE_LEVEL_11_0;

    // Try hardware first.
    // SAFETY: out params; create_flags is BGRA-required for DComp interop.
    let result = unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            windows::Win32::Foundation::HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut feature_level),
            Some(&mut context),
        )
    };

    if result.is_err() {
        // Fall back to WARP (CPU rasterizer) so we still come up on
        // RDP / VMs without a GPU driver.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                windows::Win32::Foundation::HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut feature_level),
                Some(&mut context),
            )
        }
        .context("D3D11CreateDevice failed for both HARDWARE and WARP")?;
    }

    let device = device.context("D3D11CreateDevice produced no device")?;
    let context = context.context("D3D11CreateDevice produced no context")?;
    Ok((device, context))
}

fn create_rtv(
    device: &ID3D11Device,
    swapchain: &IDXGISwapChain1,
) -> Result<ID3D11RenderTargetView> {
    // SAFETY: GetBuffer indices are zero-based; backbuffer is at 0.
    let back_buffer: ID3D11Texture2D =
        unsafe { swapchain.GetBuffer(0) }.context("swapchain GetBuffer(0) failed")?;

    let mut rtv = None;
    let desc = D3D11_RENDER_TARGET_VIEW_DESC {
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
        Anonymous: windows::Win32::Graphics::Direct3D11::D3D11_RENDER_TARGET_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_RTV { MipSlice: 0 },
        },
    };
    // SAFETY: device and back_buffer are valid; desc is on stack.
    unsafe {
        device.CreateRenderTargetView(&back_buffer, Some(&desc), Some(&mut rtv))
    }
    .context("CreateRenderTargetView failed")?;
    rtv.context("CreateRenderTargetView produced no view")
}

/// Test helper: Renderer should be Send+Sync so the host_view can
/// shuttle it between message-pump and render threads.
#[allow(dead_code)]
fn _assert_send_sync() {
    fn assert<T: Send + Sync>() {}
    assert::<Renderer>();
}
