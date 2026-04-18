//! D3D11 + DirectWrite renderer for the Windows terminal pane.
//!
//! Structure:
//!
//! ```text
//! Renderer
//!   ├── device (ID3D11Device) + context
//!   ├── swapchain (IDXGISwapChain1 against the WS_CHILD HWND)
//!   ├── rtv (ID3D11RenderTargetView onto the backbuffer)
//!   ├── dwrite (IDWriteFactory)
//!   ├── atlas (GlyphCache — etagere skyline + Direct2D DrawGlyphRun)
//!   └── pipeline (VS/PS, IA layout, instance + index + cbuffer)
//! ```
//!
//! One `DrawIndexedInstanced(6, cell_count)` per frame. Grayscale
//! coverage; bg/fg lerp in the pixel shader. See `shaders.hlsl` for the
//! actual shader code and `pipeline.rs` for the D3D11 plumbing.

mod atlas;
mod pipeline;

use std::sync::Mutex;

use anyhow::{Context, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_RENDER_TARGET_VIEW_DESC,
    D3D11_RTV_DIMENSION_TEXTURE2D, D3D11_SDK_VERSION, D3D11_TEX2D_RTV, D3D11_VIEWPORT,
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11RenderTargetView,
    ID3D11Texture2D,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWriteCreateFactory, IDWriteFactory,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIFactory6, IDXGISwapChain1,
};

use super::vt::ScreenSnapshot;
use atlas::{CellMetrics, GlyphCache, GlyphKey};
use pipeline::{instance_for_cell, Globals, Instance, Pipeline};

const ATLAS_SIZE_PX: u32 = 2048;
/// Initial instance-buffer capacity. 16 384 covers a 200×80 grid
/// without reallocation; panes larger than that grow via
/// `Pipeline::ensure_instance_capacity` in the hot path.
const INITIAL_INSTANCE_CAPACITY: u32 = 16 * 1024;

/// Rendering config that tracks the user's font + theme choice. The
/// backend-facade (`WindowsGhosttyApp::update_appearance`) writes this
/// when the user changes theme / font.
#[derive(Debug, Clone)]
pub struct RendererConfig {
    pub font_family: String,
    pub font_size_px: f32,
    pub initial_width: u32,
    pub initial_height: u32,
    pub clear_color: [f32; 4],
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

pub struct Renderer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    swapchain: IDXGISwapChain1,
    rtv: Option<ID3D11RenderTargetView>,
    _dwrite: IDWriteFactory,

    pipeline: std::sync::Mutex<Pipeline>,
    atlas: Mutex<GlyphCache>,

    /// CPU-side reusable scratch for the per-frame instance buffer.
    instances: Mutex<Vec<Instance>>,
    last_generation: Mutex<u64>,

    width_px: u32,
    height_px: u32,
}

unsafe impl Send for Renderer {}
unsafe impl Sync for Renderer {}

impl Renderer {
    pub fn new(hwnd: HWND, config: &RendererConfig) -> Result<Self> {
        log::info!("Renderer: creating D3D11 device");
        let (device, context) = create_device()?;
        log::info!("Renderer: D3D11 device created");
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
            // CreateSwapChainForHwnd rejects PREMULTIPLIED with
            // DXGI_ERROR_INVALID_CALL (0x887A0001). PREMULTIPLIED is
            // only valid with CreateSwapChainForComposition (DComp).
            // IGNORE is fine for an opaque WS_CHILD rect — the
            // terminal pane doesn't need per-pixel alpha against the
            // GPUI compositor; the HWND already clips the region.
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            Flags: 0,
        };

        // SAFETY: hwnd owned by caller (host_view); device + desc valid.
        log::info!("Renderer: creating swapchain");
        let swapchain: IDXGISwapChain1 = unsafe {
            dxgi_factory.CreateSwapChainForHwnd(&device, hwnd, &desc, None, None)
        }
        .context("CreateSwapChainForHwnd failed")?;
        log::info!("Renderer: swapchain created");

        let rtv = Some(create_rtv(&device, &swapchain)?);
        log::info!("Renderer: RTV created");

        let dwrite: IDWriteFactory =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }
                .context("DWriteCreateFactory failed")?;
        log::info!("Renderer: DWrite factory created");

        log::info!("Renderer: creating GlyphCache (font={})", config.font_family);
        let atlas = GlyphCache::new(
            &device,
            &context,
            &dwrite,
            &config.font_family,
            config.font_size_px,
            ATLAS_SIZE_PX,
        )
        .context("GlyphCache::new failed")?;
        log::info!("Renderer: GlyphCache created");

        log::info!("Renderer: creating D3D11 pipeline (HLSL compile)");
        let pipeline = Pipeline::new(&device, INITIAL_INSTANCE_CAPACITY)
            .context("Pipeline::new failed")?;
        log::info!("Renderer: pipeline ready");

        Ok(Self {
            device,
            context,
            swapchain,
            rtv,
            _dwrite: dwrite,
            pipeline: std::sync::Mutex::new(pipeline),
            atlas: Mutex::new(atlas),
            instances: Mutex::new(Vec::with_capacity(INITIAL_INSTANCE_CAPACITY as usize)),
            last_generation: Mutex::new(0),
            width_px: config.initial_width,
            height_px: config.initial_height,
        })
    }

    pub fn resize(&mut self, width_px: u32, height_px: u32) -> Result<()> {
        if width_px == 0 || height_px == 0 {
            return Ok(());
        }
        self.rtv = None;

        // SAFETY: RTV dropped; ResizeBuffers contract satisfied.
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

    /// Cell metrics the host_view uses to decide `ResizePseudoConsole`
    /// / `ghostty_terminal_resize` arguments.
    pub fn metrics(&self) -> CellMetrics {
        self.atlas.lock().unwrap().metrics()
    }

    pub fn grid_for_dimensions(&self, _config: &RendererConfig) -> (u16, u16) {
        let m = self.metrics();
        let cols = (self.width_px / m.cell_width_px.max(1)).max(1) as u16;
        let rows = (self.height_px / m.cell_height_px.max(1)).max(1) as u16;
        (cols, rows)
    }

    /// Render one frame from `snapshot`. Skips when generation hasn't
    /// advanced. Layout (top-down):
    ///
    /// 1. Gate on generation.
    /// 2. Build the per-instance buffer on the CPU from the snapshot:
    ///    per cell, look up its glyph in the atlas (rasterize on miss),
    ///    pack into an [`Instance`] struct.
    /// 3. Upload instances via `Map(WRITE_DISCARD)`.
    /// 4. Upload `Globals` cbuffer.
    /// 5. Bind + `DrawIndexedInstanced(6, cell_count)`.
    /// 6. `Present(1, 0)`.
    pub fn render(&self, snapshot: &ScreenSnapshot, config: &RendererConfig) -> Result<()> {
        {
            let mut last = self.last_generation.lock().unwrap();
            if *last == snapshot.generation {
                return Ok(());
            }
            *last = snapshot.generation;
        }

        let Some(rtv) = self.rtv.as_ref() else {
            return Ok(());
        };

        // Clear + set viewport/render targets.
        let vp = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: self.width_px as f32,
            Height: self.height_px as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        // SAFETY: context + rtv owned by self; single-threaded.
        unsafe {
            self.context.RSSetViewports(Some(&[vp]));
            self.context
                .OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
            self.context.ClearRenderTargetView(rtv, &config.clear_color);
        }

        if snapshot.cells.is_empty() {
            // Still present the clear so the pane isn't stale garbage.
            unsafe { self.swapchain.Present(1, DXGI_PRESENT(0)) }
                .ok()
                .context("swapchain Present failed")?;
            return Ok(());
        }

        // Build per-instance array.
        let mut atlas = self.atlas.lock().unwrap();
        let mut instances = self.instances.lock().unwrap();
        instances.clear();
        instances.reserve(snapshot.cells.len());
        let mut logged_non_empty: u32 = 0;

        for (i, cell) in snapshot.cells.iter().enumerate() {
            let col = (i % snapshot.cols as usize) as u16;
            let row = (i / snapshot.cols as usize) as u16;

            if cell.codepoint == 0 || cell.codepoint == 0x20 {
                // Empty / space: emit a bg-only quad by using a 0x0
                // glyph rect (sampler returns zero coverage → all bg).
                instances.push(Instance {
                    cell_pos: [col as u32, row as u32],
                    atlas_pos: [0, 0],
                    atlas_size: [0, 0],
                    fg: cell.fg,
                    bg: cell.bg,
                    attrs: cell.attrs as u32,
                });
                continue;
            }

            let key = GlyphKey {
                codepoint: cell.codepoint,
                bold: (cell.attrs & 1) != 0,
                italic: (cell.attrs & 2) != 0,
            };
            let Some(glyph) = atlas.get_or_rasterize(key) else {
                // Atlas exhausted — emit a bg quad; TODO(3b+): resize atlas.
                instances.push(Instance {
                    cell_pos: [col as u32, row as u32],
                    atlas_pos: [0, 0],
                    atlas_size: [0, 0],
                    fg: cell.fg,
                    bg: cell.bg,
                    attrs: cell.attrs as u32,
                });
                continue;
            };

            let inst = instance_for_cell(col, row, glyph, cell.fg, cell.bg, cell.attrs);
            if logged_non_empty < 6 {
                log::trace!(
                    "instance[{i}]: cell_pos=({},{}) atlas_pos=({},{}) atlas_size=({},{}) fg={:#010x} bg={:#010x} codepoint=U+{:04X}",
                    inst.cell_pos[0], inst.cell_pos[1],
                    inst.atlas_pos[0], inst.atlas_pos[1],
                    inst.atlas_size[0], inst.atlas_size[1],
                    inst.fg, inst.bg, cell.codepoint,
                );
                logged_non_empty += 1;
            }
            instances.push(inst);
        }

        drop(atlas); // release atlas lock before GPU upload

        // Cursor overlay: inverse-video block cursor. Re-emit the
        // cell-at-cursor with fg/bg swapped so the underlying glyph
        // stays legible inside the cursor block.
        //
        // Drawn AFTER the main glyph pass (we push to the end of
        // `instances`), so in the single DrawIndexedInstanced call this
        // instance writes last and visually overlays the cell beneath.
        // The pixel shader already lerps bg→fg by coverage, so swapping
        // gives us the classic "glyph in bg color on fg-color block"
        // look without any shader changes.
        if snapshot.cursor.visible {
            let col = snapshot.cursor.col as usize;
            let row = snapshot.cursor.row as usize;
            let cols_u = snapshot.cols as usize;
            let rows_u = snapshot.rows as usize;
            if col < cols_u && row < rows_u {
                let idx = row * cols_u + col;
                if let Some(src) = instances.get(idx).copied() {
                    instances.push(Instance {
                        cell_pos: src.cell_pos,
                        atlas_pos: src.atlas_pos,
                        atlas_size: src.atlas_size,
                        fg: src.bg,
                        bg: src.fg,
                        attrs: src.attrs,
                    });
                }
            }
        }

        // Grow instance buffer if the grid exceeded what we allocated.
        let mut pipeline = self
            .pipeline
            .lock()
            .expect("pipeline mutex poisoned");
        let needed = instances.len() as u32;
        if needed > pipeline.instance_capacity {
            // Allocate with 50% headroom so we don't thrash on
            // every-frame minor growth during resize.
            let new_capacity = (needed + needed / 2).max(INITIAL_INSTANCE_CAPACITY);
            log::debug!(
                "growing instance buffer: {} -> {} (need {})",
                pipeline.instance_capacity,
                new_capacity,
                needed
            );
            pipeline
                .ensure_instance_capacity(&self.device, new_capacity)
                .context("ensure_instance_capacity failed")?;
        }

        pipeline
            .upload_instances(&self.context, &instances)
            .context("upload_instances failed")?;

        // Globals.
        let metrics = self.metrics();
        let atlas_size = self.atlas.lock().unwrap().atlas_size() as f32;
        let inv_viewport = [
            2.0 / self.width_px.max(1) as f32,
            -2.0 / self.height_px.max(1) as f32,
        ];
        let globals = Globals {
            inv_viewport,
            cell_size: [
                metrics.cell_width_px as f32,
                metrics.cell_height_px as f32,
            ],
            grid_cols: snapshot.cols as u32,
            grid_rows: snapshot.rows as u32,
            inv_atlas_size: [1.0 / atlas_size, 1.0 / atlas_size],
        };
        pipeline
            .upload_globals(&self.context, &globals)
            .context("upload_globals failed")?;

        // Draw.
        let instance_count = instances.len() as u32;
        drop(instances);

        // Atlas SRV: re-lock to hand a reference to the pipeline.
        let atlas = self.atlas.lock().unwrap();
        pipeline.bind_and_draw(&self.context, atlas.atlas_srv(), instance_count);
        drop(atlas);
        drop(pipeline);

        // SAFETY: swapchain owned; Present(1, 0) waits for vsync.
        unsafe { self.swapchain.Present(1, DXGI_PRESENT(0)) }
            .ok()
            .context("swapchain Present failed")?;
        Ok(())
    }

    pub fn dimensions_px(&self) -> (u32, u32) {
        (self.width_px, self.height_px)
    }

    /// Rebuild atlas at a new font size (WM_DPICHANGED / theme change).
    pub fn rebuild_atlas(&self, font_size_px: f32) -> Result<()> {
        self.atlas.lock().unwrap().rebuild(font_size_px)
    }
}

fn create_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut context = None;
    let mut feature_level = D3D_FEATURE_LEVEL_11_0;

    // SAFETY: out params; BGRA flag needed for D2D interop on the atlas.
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
        // Fall back to WARP on RDP / VMs without a GPU driver.
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
    // SAFETY: GetBuffer(0) is the swapchain's backbuffer.
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
    // SAFETY: back_buffer + desc valid for the call.
    unsafe {
        device.CreateRenderTargetView(&back_buffer, Some(&desc), Some(&mut rtv))
    }
    .context("CreateRenderTargetView failed")?;
    rtv.context("CreateRenderTargetView produced no view")
}
