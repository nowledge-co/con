//! Glyph atlas.
//!
//! `BGRA8_UNORM` texture with skyline packing (`etagere`). Glyphs are
//! rasterized via Direct2D `DrawText` onto a `ID2D1RenderTarget` that
//! aliases the atlas texture through a DXGI surface. Grayscale
//! antialiasing (`D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE`).

use std::collections::HashMap;

use anyhow::{Context, Result};
use etagere::{size2, AllocId, AtlasAllocator};
use windows::core::Interface;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_OPTIONS,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1Factory,
    ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::Direct3D::D3D11_SRV_DIMENSION_TEXTURE2D;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_SHADER_RESOURCE_VIEW_DESC,
    D3D11_TEX2D_SRV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Device,
    ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11Texture2D,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_TEXT_METRICS, IDWriteFactory, IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGISurface;

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // offset_x/offset_y are wired in Phase 3b-2 (glyph bearing).
pub struct GlyphRect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    pub offset_x: i16,
    pub offset_y: i16,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct GlyphKey {
    pub codepoint: u32,
    pub bold: bool,
    pub italic: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct CellMetrics {
    pub cell_width_px: u32,
    pub cell_height_px: u32,
    pub baseline_px: u32,
}

pub struct GlyphCache {
    device: ID3D11Device,
    _context: ID3D11DeviceContext,
    dwrite: IDWriteFactory,
    _d2d_factory: ID2D1Factory,

    atlas_size: u32,
    _atlas_texture: ID3D11Texture2D,
    atlas_srv: ID3D11ShaderResourceView,
    d2d_rt: ID2D1RenderTarget,
    white_brush: ID2D1SolidColorBrush,

    allocator: AtlasAllocator,
    entries: HashMap<GlyphKey, (AllocId, GlyphRect)>,

    text_format_regular: IDWriteTextFormat,
    text_format_bold: IDWriteTextFormat,
    text_format_italic: IDWriteTextFormat,
    text_format_bold_italic: IDWriteTextFormat,

    metrics: CellMetrics,
    font_size_px: f32,
    font_family: String,
}

impl GlyphCache {
    pub fn new(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        dwrite: &IDWriteFactory,
        font_family: &str,
        font_size_px: f32,
        atlas_size: u32,
    ) -> Result<Self> {
        let text_format_regular =
            make_text_format(dwrite, font_family, font_size_px, false, false)?;
        let text_format_bold =
            make_text_format(dwrite, font_family, font_size_px, true, false)?;
        let text_format_italic =
            make_text_format(dwrite, font_family, font_size_px, false, true)?;
        let text_format_bold_italic =
            make_text_format(dwrite, font_family, font_size_px, true, true)?;

        let metrics = measure_cell(dwrite, &text_format_regular, font_size_px)?;

        let atlas_desc = D3D11_TEXTURE2D_DESC {
            Width: atlas_size,
            Height: atlas_size,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            // D2D's CreateDxgiSurfaceRenderTarget requires the backing
            // texture to be a render target — D2D composites glyph
            // rasterization into it. We also need SHADER_RESOURCE so
            // the D3D11 pixel shader can sample the same atlas.
            BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let mut atlas_texture: Option<ID3D11Texture2D> = None;
        // SAFETY: desc is stack-local; out param owned by us.
        unsafe { device.CreateTexture2D(&atlas_desc, None, Some(&mut atlas_texture)) }
            .context("CreateTexture2D failed for atlas")?;
        let atlas_texture = atlas_texture.context("atlas CreateTexture2D produced no texture")?;

        let srv_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            ViewDimension: D3D11_SRV_DIMENSION_TEXTURE2D,
            Anonymous: windows::Win32::Graphics::Direct3D11::D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_SRV {
                    MostDetailedMip: 0,
                    MipLevels: 1,
                },
            },
        };
        let mut atlas_srv: Option<ID3D11ShaderResourceView> = None;
        // SAFETY: descriptor valid; texture owned.
        unsafe {
            device.CreateShaderResourceView(&atlas_texture, Some(&srv_desc), Some(&mut atlas_srv))
        }
        .context("CreateShaderResourceView failed for atlas")?;
        let atlas_srv = atlas_srv.context("atlas CreateShaderResourceView produced no view")?;

        let factory_options = D2D1_FACTORY_OPTIONS::default();
        // 0.61's D2D1CreateFactory is generic over the factory interface.
        // SAFETY: options are stack-local; generic return is owned.
        let d2d_factory: ID2D1Factory = unsafe {
            D2D1CreateFactory::<ID2D1Factory>(
                D2D1_FACTORY_TYPE_SINGLE_THREADED,
                Some(&factory_options),
            )
        }
        .context("D2D1CreateFactory failed")?;

        let dxgi_surface: IDXGISurface = atlas_texture
            .cast()
            .context("atlas texture -> IDXGISurface cast failed")?;
        let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        // SAFETY: surface + props valid.
        let d2d_rt = unsafe {
            d2d_factory.CreateDxgiSurfaceRenderTarget(&dxgi_surface, &rt_props)
        }
        .context("CreateDxgiSurfaceRenderTarget failed")?;

        // SAFETY: grayscale AA — portable across rotated panels / OLED.
        unsafe {
            d2d_rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        }

        let color = D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        };
        // SAFETY: color is stack-local; brush owned by us.
        let white_brush: ID2D1SolidColorBrush =
            unsafe { d2d_rt.CreateSolidColorBrush(&color, None) }
                .context("CreateSolidColorBrush failed")?;

        let allocator = AtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32));

        Ok(Self {
            device: device.clone(),
            _context: context.clone(),
            dwrite: dwrite.clone(),
            _d2d_factory: d2d_factory,
            atlas_size,
            _atlas_texture: atlas_texture,
            atlas_srv,
            d2d_rt,
            white_brush,
            allocator,
            entries: HashMap::with_capacity(1024),
            text_format_regular,
            text_format_bold,
            text_format_italic,
            text_format_bold_italic,
            metrics,
            font_size_px,
            font_family: font_family.to_string(),
        })
    }

    pub fn metrics(&self) -> CellMetrics {
        self.metrics
    }
    pub fn atlas_srv(&self) -> &ID3D11ShaderResourceView {
        &self.atlas_srv
    }
    #[allow(dead_code)] // used once atlas-grow lands.
    pub fn atlas_size(&self) -> u32 {
        self.atlas_size
    }

    /// Return the glyph rect for a (codepoint, style) key, rasterizing
    /// on first sight. Returns `None` if the atlas is full.
    pub fn get_or_rasterize(&mut self, key: GlyphKey) -> Option<GlyphRect> {
        if let Some((_, rect)) = self.entries.get(&key).copied() {
            return Some(rect);
        }

        let cell_w = self.metrics.cell_width_px as i32;
        let cell_h = self.metrics.cell_height_px as i32;
        let alloc = self.allocator.allocate(size2(cell_w, cell_h))?;
        let rect = alloc.rectangle;

        let ch = char::from_u32(key.codepoint).unwrap_or('\u{FFFD}');
        let mut utf16 = [0u16; 2];
        let utf16_slice = ch.encode_utf16(&mut utf16);

        let format = match (key.bold, key.italic) {
            (true, true) => &self.text_format_bold_italic,
            (true, false) => &self.text_format_bold,
            (false, true) => &self.text_format_italic,
            (false, false) => &self.text_format_regular,
        };

        let draw_rect = D2D_RECT_F {
            left: rect.min.x as f32,
            top: rect.min.y as f32,
            right: rect.max.x as f32,
            bottom: rect.max.y as f32,
        };

        // SAFETY: d2d_rt, format, brush owned by self. BeginDraw / EndDraw
        // bracket a valid D2D scene; DrawText writes into the atlas via the
        // DXGI-backed RT.
        unsafe {
            self.d2d_rt.BeginDraw();
            self.d2d_rt.DrawText(
                utf16_slice,
                format,
                &draw_rect,
                &self.white_brush,
                D2D1_DRAW_TEXT_OPTIONS_NONE,
                DWRITE_MEASURING_MODE_NATURAL,
            );
            let _ = self.d2d_rt.EndDraw(None, None);
        }

        let glyph_rect = GlyphRect {
            x: rect.min.x as u16,
            y: rect.min.y as u16,
            w: cell_w as u16,
            h: cell_h as u16,
            offset_x: 0,
            offset_y: 0,
        };
        self.entries.insert(key, (alloc.id, glyph_rect));
        Some(glyph_rect)
    }

    pub fn rebuild(&mut self, font_size_px: f32) -> Result<()> {
        self.font_size_px = font_size_px;
        self.entries.clear();
        self.allocator =
            AtlasAllocator::new(size2(self.atlas_size as i32, self.atlas_size as i32));
        self.text_format_regular =
            make_text_format(&self.dwrite, &self.font_family, font_size_px, false, false)?;
        self.text_format_bold =
            make_text_format(&self.dwrite, &self.font_family, font_size_px, true, false)?;
        self.text_format_italic =
            make_text_format(&self.dwrite, &self.font_family, font_size_px, false, true)?;
        self.text_format_bold_italic =
            make_text_format(&self.dwrite, &self.font_family, font_size_px, true, true)?;
        self.metrics = measure_cell(&self.dwrite, &self.text_format_regular, font_size_px)?;
        // TODO(phase-3b+): clear the atlas texture via a render-target
        // Clear, or we'll see stale pixels from the previous size.
        let _ = &self.device; // placeholder use so `device` field isn't dead
        Ok(())
    }
}

fn make_text_format(
    dwrite: &IDWriteFactory,
    family: &str,
    size_px: f32,
    bold: bool,
    italic: bool,
) -> Result<IDWriteTextFormat> {
    let family_w: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
    let locale_w: Vec<u16> = "en-us".encode_utf16().chain(std::iter::once(0)).collect();

    let weight = if bold {
        DWRITE_FONT_WEIGHT_BOLD
    } else {
        DWRITE_FONT_WEIGHT_NORMAL
    };
    let style = if italic {
        DWRITE_FONT_STYLE_ITALIC
    } else {
        DWRITE_FONT_STYLE_NORMAL
    };

    // SAFETY: both buffers are NUL-terminated wide strings.
    let format = unsafe {
        dwrite.CreateTextFormat(
            windows::core::PCWSTR(family_w.as_ptr()),
            None,
            weight,
            style,
            DWRITE_FONT_STRETCH_NORMAL,
            size_px,
            windows::core::PCWSTR(locale_w.as_ptr()),
        )
    }
    .context("CreateTextFormat failed")?;
    Ok(format)
}

fn measure_cell(
    dwrite: &IDWriteFactory,
    format: &IDWriteTextFormat,
    font_size_px: f32,
) -> Result<CellMetrics> {
    // Measure a single 'M' via IDWriteTextLayout. For a monospace font
    // every glyph has the same advance, so one glyph is representative.
    // TextMetrics.width returns the exact advance (in DIPs at the
    // format's font size) and .height returns the full line height
    // (ascent + descent + lineGap) — both in the same DIP space we pass
    // to D2D, so no unit conversion is needed.
    //
    // The previous `fs * 0.6` heuristic undercounted the advance on
    // Consolas / Cascadia / Ioskeley, leaving every cell ~1px narrow and
    // manifesting as visible kerning gaps ("W ndows", "M trosoft") in
    // the pwsh banner.
    let text: Vec<u16> = "M".encode_utf16().collect();
    // SAFETY: text outlives the layout; format is owned by caller.
    let layout = unsafe {
        dwrite.CreateTextLayout(&text, format, 4096.0, 4096.0)
    }
    .context("IDWriteFactory::CreateTextLayout failed")?;

    let mut tm = DWRITE_TEXT_METRICS::default();
    // SAFETY: out param sized per windows-rs binding.
    unsafe { layout.GetMetrics(&mut tm) }
        .context("IDWriteTextLayout::GetMetrics failed")?;

    let cell_width_px = tm.width.ceil().max(1.0) as u32;
    let cell_height_px = tm.height.ceil().max(1.0) as u32;
    // Baseline isn't exposed on DWRITE_TEXT_METRICS; use a conservative
    // ascent estimate. Not currently consumed by the pipeline, but we
    // return something sensible so future callers don't get 0.
    let baseline_px = (font_size_px * 0.8).ceil() as u32;

    log::info!(
        "measure_cell: font_size_px={} -> cell {}x{} (DWRITE width={} height={})",
        font_size_px,
        cell_width_px,
        cell_height_px,
        tm.width,
        tm.height,
    );

    Ok(CellMetrics {
        cell_width_px,
        cell_height_px,
        baseline_px,
    })
}
