//! D3D11 + DirectWrite renderer for the Windows terminal pane.
//!
//! Structure:
//!
//! ```text
//! Renderer
//!   ├── device (ID3D11Device) + context
//!   ├── rt_texture (ID3D11Texture2D, offscreen B8G8R8A8_UNORM)
//!   ├── rtv (ID3D11RenderTargetView onto rt_texture)
//!   ├── staging (ID3D11Texture2D, USAGE_STAGING | CPU_ACCESS_READ)
//!   ├── dwrite (IDWriteFactory)
//!   ├── atlas (GlyphCache — etagere skyline + Direct2D DrawGlyphRun)
//!   └── pipeline (VS/PS, IA layout, instance + index + cbuffer)
//! ```
//!
//! We render into an offscreen texture instead of an HWND swapchain so
//! the caller can CPU-read the BGRA bytes and hand them to GPUI as an
//! `ImageSource::Render(Arc<RenderImage>)`. GPUI then composes the pane
//! into its own DirectComposition tree — no child HWND on top of the
//! app's modals, no z-order glitches on tab switch.
//!
//! One `DrawIndexedInstanced(6, cell_count)` per dirty frame. Grayscale
//! coverage; bg/fg lerp in the pixel shader. See `shaders.hlsl` for the
//! shader code and `pipeline.rs` for the D3D11 plumbing.

mod atlas;
mod font_loader;
mod pipeline;

use std::sync::Mutex;

use anyhow::{Context, Result};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_11_0,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_RENDER_TARGET_VIEW_DESC, D3D11_RTV_DIMENSION_TEXTURE2D, D3D11_SDK_VERSION,
    D3D11_TEX2D_RTV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
    D3D11_VIEWPORT, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
    ID3D11RenderTargetView, ID3D11Texture2D,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWriteCreateFactory, IDWriteFactory,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};

use super::vt::ScreenSnapshot;
use atlas::{GlyphCache, GlyphKey};
use pipeline::{Globals, Instance, Pipeline, instance_for_cell};

pub use atlas::CellMetrics;

const ATLAS_SIZE_PX: u32 = 2048;
/// Initial instance-buffer capacity. 16 384 covers a 200×80 grid
/// without reallocation; panes larger than that grow via
/// `Pipeline::ensure_instance_capacity` in the hot path.
const INITIAL_INSTANCE_CAPACITY: u32 = 16 * 1024;

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
            font_family: font_loader::BUNDLED_FONT_FAMILY.to_string(),
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
    rt_texture: ID3D11Texture2D,
    rtv: ID3D11RenderTargetView,
    staging: ID3D11Texture2D,
    _dwrite: IDWriteFactory,

    pipeline: std::sync::Mutex<Pipeline>,
    atlas: Mutex<GlyphCache>,

    instances: Mutex<Vec<Instance>>,
    /// Generation fingerprint of the last frame we actually rendered
    /// (snapshot.generation ⨁ selection). Seeded with `u64::MAX` so the
    /// very first call — which sees `generation = 0` on a quiet VT —
    /// still produces a frame (the cleared background), giving the pane
    /// something to show before the shell has printed anything.
    last_generation: Mutex<u64>,
    selection: Mutex<Option<Selection>>,

    width_px: u32,
    height_px: u32,
}

/// Freshly rendered BGRA frame. Width/height are in physical pixels.
pub struct FrameBgra {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Result of [`Renderer::render`].
pub enum RenderOutcome {
    /// No change since the previous call — reuse the prior image.
    Unchanged,
    /// Fresh BGRA bytes, ready to hand to GPUI as an `ImageSource`.
    Rendered(FrameBgra),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Selection {
    pub anchor: (u16, u16),
    pub extent: (u16, u16),
}

impl Selection {
    pub fn contains(&self, col: u16, row: u16, cols: u16) -> bool {
        let to_lin = |p: (u16, u16)| (p.1 as u32) * (cols as u32) + (p.0 as u32);
        let a = to_lin(self.anchor);
        let b = to_lin(self.extent);
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let here = to_lin((col, row));
        here >= lo && here <= hi
    }

    pub fn hash_u64(&self) -> u64 {
        let a = ((self.anchor.0 as u64) << 16) | (self.anchor.1 as u64);
        let b = ((self.extent.0 as u64) << 16) | (self.extent.1 as u64);
        (a << 32) | b
    }
}

unsafe impl Send for Renderer {}
unsafe impl Sync for Renderer {}

impl Renderer {
    pub fn new(config: &RendererConfig) -> Result<Self> {
        log::info!("Renderer: creating D3D11 device");
        let (device, context) = create_device()?;
        log::info!("Renderer: D3D11 device created");

        let width = config.initial_width.max(1);
        let height = config.initial_height.max(1);

        log::info!("Renderer: creating offscreen render-target texture {width}x{height}");
        let (rt_texture, rtv) = create_rt_texture(&device, width, height)?;
        log::info!("Renderer: RT texture + RTV created");

        let staging = create_staging_texture(&device, width, height)?;
        log::info!("Renderer: staging texture created");

        let dwrite: IDWriteFactory =
            unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }
                .context("DWriteCreateFactory failed")?;
        log::info!("Renderer: DWrite factory created");

        let bundled_collection =
            font_loader::build_bundled_collection(&dwrite).unwrap_or_else(|err| {
                log::warn!("font bundling failed; using system fallback: {err:#}");
                None
            });

        log::info!(
            "Renderer: creating GlyphCache (font={})",
            config.font_family
        );
        let atlas = GlyphCache::new(
            &device,
            &context,
            &dwrite,
            bundled_collection,
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
            rt_texture,
            rtv,
            staging,
            _dwrite: dwrite,
            pipeline: std::sync::Mutex::new(pipeline),
            atlas: Mutex::new(atlas),
            instances: Mutex::new(Vec::with_capacity(INITIAL_INSTANCE_CAPACITY as usize)),
            last_generation: Mutex::new(u64::MAX),
            selection: Mutex::new(None),
            width_px: width,
            height_px: height,
        })
    }

    pub fn resize(&mut self, width_px: u32, height_px: u32) -> Result<()> {
        if width_px == 0 || height_px == 0 {
            return Ok(());
        }
        if width_px == self.width_px && height_px == self.height_px {
            return Ok(());
        }

        let (rt_texture, rtv) = create_rt_texture(&self.device, width_px, height_px)?;
        let staging = create_staging_texture(&self.device, width_px, height_px)?;
        self.rt_texture = rt_texture;
        self.rtv = rtv;
        self.staging = staging;
        self.width_px = width_px;
        self.height_px = height_px;
        *self
            .last_generation
            .lock()
            .expect("last_generation mutex poisoned in resize()") = u64::MAX;
        Ok(())
    }

    pub fn metrics(&self) -> CellMetrics {
        self.atlas
            .lock()
            .expect("atlas mutex poisoned in metrics()")
            .metrics()
    }

    /// Install / clear the current selection. The render-gate combines
    /// `snapshot.generation` with the selection fingerprint, so setting
    /// a new selection naturally invalidates the last frame.
    pub fn set_selection(&self, selection: Option<Selection>) {
        *self
            .selection
            .lock()
            .expect("selection mutex poisoned in set_selection()") = selection;
    }

    pub fn selection(&self) -> Option<Selection> {
        *self
            .selection
            .lock()
            .expect("selection mutex poisoned in selection()")
    }

    pub fn grid_for_dimensions(&self, _config: &RendererConfig) -> (u16, u16) {
        let m = self.metrics();
        let cols = (self.width_px / m.cell_width_px.max(1)).max(1) as u16;
        let rows = (self.height_px / m.cell_height_px.max(1)).max(1) as u16;
        (cols, rows)
    }

    pub fn dimensions_px(&self) -> (u32, u32) {
        (self.width_px, self.height_px)
    }

    pub fn rebuild_atlas(&self, font_size_px: f32) -> Result<()> {
        self.atlas
            .lock()
            .expect("atlas mutex poisoned in rebuild_atlas()")
            .rebuild(font_size_px)?;
        *self
            .last_generation
            .lock()
            .expect("last_generation mutex poisoned in rebuild_atlas()") = u64::MAX;
        Ok(())
    }

    /// Render one frame and return a BGRA byte buffer sized to the
    /// current render target. Returns `Unchanged` when nothing has moved
    /// since the last call (caller reuses its cached image).
    pub fn render(
        &self,
        snapshot: &ScreenSnapshot,
        config: &RendererConfig,
    ) -> Result<RenderOutcome> {
        let selection = *self
            .selection
            .lock()
            .expect("selection mutex poisoned in render()");
        let sel_hash = selection.map(|s| s.hash_u64()).unwrap_or(0);
        let combined = snapshot
            .generation
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(sel_hash);
        {
            let mut last = self
                .last_generation
                .lock()
                .expect("last_generation mutex poisoned in render()");
            if *last == combined {
                return Ok(RenderOutcome::Unchanged);
            }
            *last = combined;
        }

        let vp = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: self.width_px as f32,
            Height: self.height_px as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        unsafe {
            self.context.RSSetViewports(Some(&[vp]));
            self.context
                .OMSetRenderTargets(Some(&[Some(self.rtv.clone())]), None);
            self.context
                .ClearRenderTargetView(&self.rtv, &config.clear_color);
        }

        if !snapshot.cells.is_empty() {
            self.draw_cells(snapshot)?;
        }

        let bytes = self.readback_rt()?;
        Ok(RenderOutcome::Rendered(FrameBgra {
            bytes,
            width: self.width_px,
            height: self.height_px,
        }))
    }

    fn draw_cells(&self, snapshot: &ScreenSnapshot) -> Result<()> {
        let selection = *self
            .selection
            .lock()
            .expect("selection mutex poisoned in draw_cells()");

        let mut atlas = self
            .atlas
            .lock()
            .expect("atlas mutex poisoned in draw_cells() glyph pass");
        let mut instances = self
            .instances
            .lock()
            .expect("instances mutex poisoned in draw_cells()");
        instances.clear();
        instances.reserve(snapshot.cells.len());

        for (i, cell) in snapshot.cells.iter().enumerate() {
            let col = (i % snapshot.cols as usize) as u16;
            let row = (i / snapshot.cols as usize) as u16;

            let in_sel = selection
                .map(|s| s.contains(col, row, snapshot.cols))
                .unwrap_or(false);
            let effective_attrs = if in_sel {
                cell.attrs ^ 0x10
            } else {
                cell.attrs
            };

            if cell.codepoint == 0 || cell.codepoint == 0x20 {
                instances.push(Instance {
                    cell_pos: [col as u32, row as u32],
                    atlas_pos: [0, 0],
                    atlas_size: [0, 0],
                    fg: cell.fg,
                    bg: cell.bg,
                    attrs: effective_attrs as u32,
                });
                continue;
            }

            let key = GlyphKey {
                codepoint: cell.codepoint,
                bold: (cell.attrs & 1) != 0,
                italic: (cell.attrs & 2) != 0,
            };
            let glyph = match atlas.get_or_rasterize(key) {
                Some(g) => g,
                None => {
                    log::debug!(
                        "atlas overflow at cell ({col},{row}) U+{:04X}; purging and retrying",
                        cell.codepoint
                    );
                    atlas.purge();
                    match atlas.get_or_rasterize(key) {
                        Some(g) => g,
                        None => {
                            log::warn!(
                                "glyph larger than atlas capacity at U+{:04X}; skipping",
                                cell.codepoint
                            );
                            instances.push(Instance {
                                cell_pos: [col as u32, row as u32],
                                atlas_pos: [0, 0],
                                atlas_size: [0, 0],
                                fg: cell.fg,
                                bg: cell.bg,
                                attrs: effective_attrs as u32,
                            });
                            continue;
                        }
                    }
                }
            };

            instances.push(instance_for_cell(
                col,
                row,
                glyph,
                cell.fg,
                cell.bg,
                effective_attrs,
            ));
        }
        let cell_w_px = atlas.metrics().cell_width_px;
        drop(atlas);

        // Sort so oversized PUA icons render LAST within the grid
        // pass. Their atlas slots are wider than a cell, so their
        // quad overflows into the neighbour column. DX11 guarantees
        // in-order per-pixel writes within a single draw call, so
        // placing wide instances at the end of the buffer makes
        // their pixels win over the narrow bg-only instance that
        // would otherwise paint on top. Stable partition keeps the
        // grid's row-major order within the narrow and wide groups
        // independently — bad ordering would show up as flicker.
        instances.sort_by_key(|inst| (inst.atlas_size[0] > cell_w_px) as u8);

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
                        attrs: src.attrs & !0x10,
                    });
                }
            }
        }

        let mut pipeline = self
            .pipeline
            .lock()
            .expect("pipeline mutex poisoned");
        let needed = instances.len() as u32;
        if needed > pipeline.instance_capacity {
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

        let metrics = self.metrics();
        let atlas_size = self
            .atlas
            .lock()
            .expect("atlas mutex poisoned reading atlas_size")
            .atlas_size() as f32;
        let inv_viewport = [
            2.0 / self.width_px.max(1) as f32,
            -2.0 / self.height_px.max(1) as f32,
        ];
        let globals = Globals {
            inv_viewport,
            cell_size: [metrics.cell_width_px as f32, metrics.cell_height_px as f32],
            grid_cols: snapshot.cols as u32,
            grid_rows: snapshot.rows as u32,
            inv_atlas_size: [1.0 / atlas_size, 1.0 / atlas_size],
        };
        pipeline
            .upload_globals(&self.context, &globals)
            .context("upload_globals failed")?;

        let instance_count = instances.len() as u32;
        drop(instances);

        let atlas = self
            .atlas
            .lock()
            .expect("atlas mutex poisoned before bind_and_draw");
        pipeline.bind_and_draw(&self.context, atlas.atlas_srv(), instance_count);
        drop(atlas);
        drop(pipeline);
        Ok(())
    }

    fn readback_rt(&self) -> Result<Vec<u8>> {
        let width = self.width_px as usize;
        let height = self.height_px as usize;
        let row_bytes = width * 4;
        let mut out = vec![0u8; row_bytes * height];

        unsafe {
            self.context.CopyResource(&self.staging, &self.rt_texture);
        }

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
        }
        .context("Map(staging) failed")?;

        let src_pitch = mapped.RowPitch as usize;
        for y in 0..height {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    (mapped.pData as *const u8).add(src_pitch * y),
                    out.as_mut_ptr().add(row_bytes * y),
                    row_bytes,
                );
            }
        }

        unsafe {
            self.context.Unmap(&self.staging, 0);
        }
        Ok(out)
    }
}

fn create_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut context = None;
    let mut feature_level = D3D_FEATURE_LEVEL_11_0;

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

fn create_rt_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<(ID3D11Texture2D, ID3D11RenderTargetView)> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }
        .context("CreateTexture2D(rt) failed")?;
    let texture = texture.context("rt CreateTexture2D produced no texture")?;

    let rtv_desc = D3D11_RENDER_TARGET_VIEW_DESC {
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
        Anonymous: windows::Win32::Graphics::Direct3D11::D3D11_RENDER_TARGET_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_RTV { MipSlice: 0 },
        },
    };
    let mut rtv = None;
    unsafe { device.CreateRenderTargetView(&texture, Some(&rtv_desc), Some(&mut rtv)) }
        .context("CreateRenderTargetView(rt) failed")?;
    let rtv = rtv.context("CreateRenderTargetView produced no view")?;
    Ok((texture, rtv))
}

fn create_staging_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut texture = None;
    unsafe { device.CreateTexture2D(&desc, None, Some(&mut texture)) }
        .context("CreateTexture2D(staging) failed")?;
    texture.context("staging CreateTexture2D produced no texture")
}
