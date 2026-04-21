//! Glyph atlas.
//!
//! `BGRA8_UNORM` texture with skyline packing (`etagere`). Glyphs are
//! rasterized via Direct2D `DrawText` onto a `ID2D1RenderTarget` that
//! aliases the atlas texture through a DXGI surface. ClearType
//! antialiasing writes RGB subpixel coverage into the atlas (one
//! coverage value per channel); the pixel shader lerps fg→bg per
//! channel to produce the final subpixel result. We fall back to
//! grayscale if `SetTextAntialiasMode(CLEARTYPE)` isn't supported on
//! this GPU / display (rare: only shows up in RDP / forced-grayscale
//! accessibility modes).

use std::collections::HashMap;

use anyhow::{Context, Result};
use etagere::{size2, AllocId, AtlasAllocator};
use windows::core::Interface;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_OPTIONS, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, ID2D1Factory,
    ID2D1RenderTarget, ID2D1SolidColorBrush,
};
use windows_numerics::Matrix3x2;
use windows::Win32::Graphics::Direct3D::D3D11_SRV_DIMENSION_TEXTURE2D;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_SHADER_RESOURCE_VIEW_DESC,
    D3D11_TEX2D_SRV, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Device,
    ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11Texture2D,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FONT_METRICS, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_ITALIC,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_BOLD, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_GLYPH_METRICS, DWRITE_MEASURING_MODE_NATURAL, DWRITE_PIXEL_GEOMETRY_RGB,
    DWRITE_RENDERING_MODE_NATURAL, IDWriteFactory, IDWriteFontCollection, IDWriteFontFace,
    IDWriteFontFallback, IDWriteRenderingParams, IDWriteTextFormat, IDWriteTextFormat1,
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
struct GlyphMetricsPx {
    /// Natural ink-box width (advance − |lsb| − |rsb| in design units
    /// → pixels). Used to decide whether to widen the atlas slot.
    ink_px: f32,
    /// Left-side bearing in physical pixels. Negative values mean the
    /// ink box extends leftward past the advance origin — we shift the
    /// draw rect right by `-lsb_px` so that overhang fits in the slot.
    lsb_px: f32,
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
    /// Bundled private collection (IoskeleyMono). `None` when the
    /// runtime didn't support IDWriteFactory5; CreateTextFormat then
    /// resolves through the system collection.
    bundled_collection: Option<IDWriteFontCollection>,
    /// System font-fallback cascade. Attached to each
    /// `IDWriteTextFormat1` so DirectWrite transparently swaps in
    /// Segoe UI Emoji / Symbol / CJK fonts for codepoints the bundled
    /// IoskeleyMono lacks. `None` on pre-Win8.1 hosts or if
    /// `GetSystemFontFallback` fails at init.
    font_fallback: Option<IDWriteFontFallback>,
    _d2d_factory: ID2D1Factory,

    atlas_size: u32,
    _atlas_texture: ID3D11Texture2D,
    atlas_srv: ID3D11ShaderResourceView,
    d2d_rt: ID2D1RenderTarget,
    white_brush: ID2D1SolidColorBrush,
    /// Opaque-black brush. Used to clear each slot before `DrawText` so
    /// any stale pixels — from a neighbouring scaled-PUA glyph that bled
    /// past its own slot via ClearType AA fringe or wide layoutRect —
    /// don't show through as speckles on thin glyphs (hyphens,
    /// box-drawing).
    black_brush: ID2D1SolidColorBrush,

    allocator: AtlasAllocator,
    entries: HashMap<GlyphKey, (AllocId, GlyphRect)>,

    text_format_regular: IDWriteTextFormat,
    text_format_bold: IDWriteTextFormat,
    text_format_italic: IDWriteTextFormat,
    text_format_bold_italic: IDWriteTextFormat,

    /// Regular-weight face of the primary family, kept around so we can
    /// query per-glyph design metrics at rasterize time and scale wide
    /// Nerd-Font icons to fit a single cell. `None` means the family
    /// couldn't be resolved (logged at init) — we then skip scaling and
    /// DrawText renders glyphs as-is (legacy behavior).
    primary_face: Option<IDWriteFontFace>,
    /// Design units per em for `primary_face`. Cached to avoid calling
    /// `GetMetrics` on every rasterize. Only meaningful when
    /// `primary_face.is_some()`.
    primary_upm: f32,

    metrics: CellMetrics,
    font_size_px: f32,
    font_family: String,
}

impl GlyphCache {
    pub fn new(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        dwrite: &IDWriteFactory,
        bundled_collection: Option<IDWriteFontCollection>,
        font_family: &str,
        font_size_px: f32,
        atlas_size: u32,
    ) -> Result<Self> {
        // OS-default fallback cascade (emoji / symbol / CJK). Built
        // once here and shared across all four text-format weights.
        let font_fallback = super::font_loader::system_font_fallback(dwrite);

        // Resolve the family name against the bundled collection up
        // front. GPUI / config use the display form "Ioskeley Mono"
        // (with a space), but the bundled TTFs advertise the
        // PostScript-style one-word name "IoskeleyMono". DirectWrite
        // does strict family-name matching, so asking for
        // "Ioskeley Mono" misses the bundled collection entirely and
        // cascades to Segoe UI for both metrics AND glyph rasterization
        // — cells sized for a 9/10-em Segoe 'M' advance, and glyphs
        // drawn from Segoe UI rather than IoskeleyMono. Use the name
        // that actually resolves in our bundled collection.
        let resolved_family =
            resolve_bundled_family(bundled_collection.as_ref(), font_family);

        let text_format_regular = make_text_format(
            dwrite,
            bundled_collection.as_ref(),
            font_fallback.as_ref(),
            &resolved_family,
            font_size_px,
            false,
            false,
        )?;
        let text_format_bold = make_text_format(
            dwrite,
            bundled_collection.as_ref(),
            font_fallback.as_ref(),
            &resolved_family,
            font_size_px,
            true,
            false,
        )?;
        let text_format_italic = make_text_format(
            dwrite,
            bundled_collection.as_ref(),
            font_fallback.as_ref(),
            &resolved_family,
            font_size_px,
            false,
            true,
        )?;
        let text_format_bold_italic = make_text_format(
            dwrite,
            bundled_collection.as_ref(),
            font_fallback.as_ref(),
            &resolved_family,
            font_size_px,
            true,
            true,
        )?;

        let metrics = measure_cell(
            dwrite,
            bundled_collection.as_ref(),
            &resolved_family,
            font_size_px,
        )?;

        // Resolve a face for per-glyph design-metrics lookups. The face
        // comes from the same collection `measure_cell` walked, so
        // bundled > system > last-resort Segoe. We intentionally ignore
        // errors here — on failure `primary_face` stays `None` and the
        // rasterize path skips scale-to-fit, matching pre-fix behavior.
        let (primary_face, primary_upm) =
            match resolve_font_face(dwrite, bundled_collection.as_ref(), &resolved_family) {
                Ok((face, _src)) => {
                    let mut fm = DWRITE_FONT_METRICS::default();
                    // SAFETY: windows-rs writes through the out pointer.
                    unsafe { face.GetMetrics(&mut fm) };
                    (Some(face), fm.designUnitsPerEm as f32)
                }
                Err(err) => {
                    log::warn!(
                        "GlyphCache::new: primary face resolution failed ({err:?}); \
                         wide Nerd-Font icons will render clipped at the cell edge"
                    );
                    (None, 1.0)
                }
            };

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
        // ClearType requires an opaque-alpha render target: D2D composes
        // the RGB subpixel coverage against the pre-painted surface
        // directly. `ALPHA_MODE_IGNORE` leaves the alpha channel unused
        // (we'll always sample BGRA8 with the 3 RGB channels carrying
        // per-subpixel coverage; alpha stays 1.0).
        let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_IGNORE,
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

        // Custom rendering params give us consistent ClearType output
        // across machines regardless of the user's Control-Panel
        // ClearType Tuner settings. Values mirror Windows Terminal's
        // TextureAtlas (see microsoft/terminal renderer/atlas/...).
        //
        // If CreateCustomRenderingParams fails (very rare — it's a pure
        // parameter validator) we leave the default params in place.
        // SAFETY: constants are valid for IDWriteFactory.
        let custom_params: Option<IDWriteRenderingParams> = unsafe {
            dwrite
                .CreateCustomRenderingParams(
                    1.8,                         // gamma
                    0.5,                         // enhanced contrast
                    1.0,                         // ClearType level
                    DWRITE_PIXEL_GEOMETRY_RGB,   // subpixel layout
                    DWRITE_RENDERING_MODE_NATURAL,
                )
                .ok()
        };
        // SAFETY: ClearType AA. Fall back to grayscale if the display
        // pipeline forces it (screen readers, RDP, color-filter modes)
        // — setting the mode is always cheap; the D2D runtime picks the
        // closest supported mode, so this is fire-and-forget.
        unsafe {
            d2d_rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_CLEARTYPE);
            if let Some(params) = custom_params.as_ref() {
                d2d_rt.SetTextRenderingParams(params);
            }
        }
        let _ = D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE; // keep import live for docs

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

        let black = D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        // SAFETY: color is stack-local; brush owned by us.
        let black_brush: ID2D1SolidColorBrush =
            unsafe { d2d_rt.CreateSolidColorBrush(&black, None) }
                .context("CreateSolidColorBrush(black) failed")?;

        // Seed the atlas with black so ClearType has a defined blend
        // target for the first DrawText. D3D11 zero-inits the texture
        // but the 0-alpha of (0,0,0,0) can confuse D2D's internal state
        // assertions in some drivers; an explicit Clear sidesteps that.
        // SAFETY: d2d_rt owned by us and targets the atlas texture.
        unsafe {
            d2d_rt.BeginDraw();
            d2d_rt.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }));
            let _ = d2d_rt.EndDraw(None, None);
        }

        let allocator = AtlasAllocator::new(size2(atlas_size as i32, atlas_size as i32));

        Ok(Self {
            device: device.clone(),
            _context: context.clone(),
            dwrite: dwrite.clone(),
            bundled_collection,
            font_fallback,
            _d2d_factory: d2d_factory,
            atlas_size,
            _atlas_texture: atlas_texture,
            atlas_srv,
            d2d_rt,
            white_brush,
            black_brush,
            allocator,
            entries: HashMap::with_capacity(1024),
            text_format_regular,
            text_format_bold,
            text_format_italic,
            text_format_bold_italic,
            primary_face,
            primary_upm,
            metrics,
            font_size_px,
            // Store the RESOLVED family — downstream callers (rebuild,
            // font-size changes) go through the same make_text_format /
            // measure_cell path and must keep using the bundled name.
            font_family: resolved_family,
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

        // Nerd-Font PUA icons (U+E000..U+F8FF) are authored with an
        // advance of one grid cell but an ink box that routinely
        // extends well past the cell on both sides (folder U+F07B,
        // github U+F09B, branch U+E0A0, …). Rendering them at 1:1 in
        // a cell_w × cell_h slot clips whichever side spills.
        //
        // Previous attempts — widen the atlas slot to 2 cells + shift
        // the pen — still failed on screen: the next column's
        // instance paints its own `bg` rect of cell_w and overdraws
        // the widened glyph's right half. Users saw "half icons".
        //
        // Scale-to-fit: always allocate a 1-cell slot, then apply a
        // uniform D2D transform that shrinks the glyph so its natural
        // ink bounding box fits inside the cell with 1 px breathing
        // room. No cross-cell overdraw, no clipping.
        //
        // NB: Powerline "extra symbols" (U+E0A0..U+E0D4) — arrows,
        // separators — are authored flush-to-advance with ink that
        // already fits the cell. Scaling them would leave tell-tale
        // gaps between consecutive separator glyphs. Exempt the block
        // and draw at 1:1 with CLIP.
        let codepoint = key.codepoint;
        let is_scalable_pua = matches!(codepoint, 0xE000..=0xF8FF)
            && !matches!(codepoint, 0xE0A0..=0xE0D4);
        let metrics = if is_scalable_pua {
            self.primary_glyph_metrics_px(codepoint)
        } else {
            None
        };

        let glyph_w = cell_w;
        let alloc = self.allocator.allocate(size2(glyph_w, cell_h))?;
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

        let slot_rect = D2D_RECT_F {
            left: rect.min.x as f32,
            top: rect.min.y as f32,
            right: rect.max.x as f32,
            bottom: rect.max.y as f32,
        };

        // Compute a scale-to-fit transform for wide PUA glyphs. For
        // glyphs that already fit inside the cell we skip the
        // transform entirely — normal letters take the fast path.
        let transform = metrics.and_then(|m| {
            let lsb_overhang = (-m.lsb_px).max(0.0);
            let natural_w = m.ink_px + lsb_overhang;
            if natural_w <= (cell_w as f32) + 1.0 {
                return None;
            }
            // Uniform scale: shrink just enough to fit natural_w into
            // (cell_w - 2) so there is 1 px of breathing room each
            // side. Floor at 0.5 so outrageously oversized glyphs
            // don't collapse to specks.
            let target_w = (cell_w as f32 - 2.0).max(1.0);
            let scale = (target_w / natural_w).clamp(0.5, 1.0);

            // Compose: scale around slot centre, then shift so the
            // ink's LEFT edge lands at `slot.left + left_pad`. Pen
            // origin (LEADING) sits at slot.left; ink's pre-scale
            // left edge is at slot.left - lsb_overhang. After
            // scale-around-centre `cx`, that point maps to
            //   cx + (slot.left - lsb_overhang - cx) * scale
            // We add an `dx` translation that puts the ink left edge
            // at `slot.left + left_pad`.
            let cx = (slot_rect.left + slot_rect.right) * 0.5;
            let cy = (slot_rect.top + slot_rect.bottom) * 0.5;
            let left_pad = ((cell_w as f32) - natural_w * scale) * 0.5;
            let scaled_ink_left = cx + (slot_rect.left - lsb_overhang - cx) * scale;
            let want_ink_left = slot_rect.left + left_pad;
            let dx = want_ink_left - scaled_ink_left;

            // x' = scale*x + (1-scale)*cx + dx
            // y' = scale*y + (1-scale)*cy
            let tf = Matrix3x2 {
                M11: scale,
                M12: 0.0,
                M21: 0.0,
                M22: scale,
                M31: (1.0 - scale) * cx + dx,
                M32: (1.0 - scale) * cy,
            };
            log::info!(
                "atlas: PUA U+{:04X} ink={:.1}px lsb={:.1}px \
                 natural_w={:.1}px cell={} → scale={:.3}",
                codepoint,
                m.ink_px,
                m.lsb_px,
                natural_w,
                cell_w,
                scale,
            );
            Some(tf)
        });

        let identity = Matrix3x2 {
            M11: 1.0,
            M12: 0.0,
            M21: 0.0,
            M22: 1.0,
            M31: 0.0,
            M32: 0.0,
        };

        // SAFETY: d2d_rt, format, brushes owned by self. BeginDraw /
        // EndDraw bracket a valid D2D scene; DrawText writes into the
        // atlas via the DXGI-backed RT. PushAxisAlignedClip pins all
        // writes to `slot_rect` so (a) ClearType AA fringe at the slot
        // edges can't leak into adjacent atlas entries, and (b) the
        // scaled-PUA path — which uses an oversized layoutRect so
        // DirectWrite doesn't prematurely clip the natural ink — can't
        // overshoot into a neighbour's slot either. FillRectangle with
        // opaque black pre-clears any stale pixels (either zero-init
        // or left over from a previous glyph that bled in before we
        // added this clip).
        unsafe {
            self.d2d_rt.BeginDraw();
            self.d2d_rt
                .PushAxisAlignedClip(&slot_rect, D2D1_ANTIALIAS_MODE_ALIASED);
            self.d2d_rt.FillRectangle(&slot_rect, &self.black_brush);
            if let Some(tf) = transform {
                // Over-sized layout rect so DirectWrite's internal
                // text-layout clip (built from the layoutRect) doesn't
                // cut off the natural ink before our scale transform
                // maps it back into the cell. The outer PushAxisAlignedClip
                // keeps any overshoot contained to this slot.
                let layout = D2D_RECT_F {
                    left: slot_rect.left - (cell_w as f32),
                    top: slot_rect.top - (cell_h as f32),
                    right: slot_rect.right + (cell_w as f32),
                    bottom: slot_rect.bottom + (cell_h as f32),
                };
                self.d2d_rt.SetTransform(&tf);
                self.d2d_rt.DrawText(
                    utf16_slice,
                    format,
                    &layout,
                    &self.white_brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
                self.d2d_rt.SetTransform(&identity);
            } else {
                self.d2d_rt.DrawText(
                    utf16_slice,
                    format,
                    &slot_rect,
                    &self.white_brush,
                    D2D1_DRAW_TEXT_OPTIONS_CLIP,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
            self.d2d_rt.PopAxisAlignedClip();
            let _ = self.d2d_rt.EndDraw(None, None);
        }

        let glyph_rect = GlyphRect {
            x: rect.min.x as u16,
            y: rect.min.y as u16,
            w: glyph_w as u16,
            h: cell_h as u16,
            offset_x: 0,
            offset_y: 0,
        };
        self.entries.insert(key, (alloc.id, glyph_rect));
        Some(glyph_rect)
    }

    /// Glyph metrics in physical pixels for `codepoint` in the primary
    /// face, or `None` when we can't measure (face absent, glyph not in
    /// the primary face so DWrite fallback will handle it, or metrics
    /// call failed). Used at rasterize time to widen the atlas slot and
    /// offset the draw origin for overflow glyphs like Nerd-Font PUA
    /// icons whose ink boxes extend past the advance.
    fn primary_glyph_metrics_px(&self, codepoint: u32) -> Option<GlyphMetricsPx> {
        let face = self.primary_face.as_ref()?;
        let mut indices = [0u16; 1];
        let cps = [codepoint];
        // SAFETY: inputs sized 1; writes through out-pointer.
        let hr = unsafe {
            face.GetGlyphIndices(cps.as_ptr(), 1, indices.as_mut_ptr())
        };
        if hr.is_err() || indices[0] == 0 {
            return None;
        }
        let mut gm = [DWRITE_GLYPH_METRICS::default(); 1];
        // SAFETY: gid + gm both length 1; horizontal mode.
        let hr = unsafe {
            face.GetDesignGlyphMetrics(indices.as_ptr(), 1, gm.as_mut_ptr(), false)
        };
        if hr.is_err() {
            return None;
        }
        let m = gm[0];
        if self.primary_upm <= 0.0 {
            return None;
        }
        let du_to_px = self.font_size_px / self.primary_upm;
        let ink_du = (m.advanceWidth as i32 - m.leftSideBearing - m.rightSideBearing) as f32;
        if ink_du <= 0.0 {
            return None;
        }
        Some(GlyphMetricsPx {
            ink_px: ink_du * du_to_px,
            lsb_px: (m.leftSideBearing as f32) * du_to_px,
        })
    }

    /// Evict every cached glyph and reset the skyline allocator without
    /// touching the text formats. Used by the renderer when a frame's
    /// glyph set exceeds the atlas capacity: drop the old set, try
    /// again. Re-rasterizing the live frame is O(cells) and cheap
    /// compared to the D3D stall a texture recreate would cause.
    pub fn purge(&mut self) {
        self.entries.clear();
        self.allocator =
            AtlasAllocator::new(size2(self.atlas_size as i32, self.atlas_size as i32));
        // SAFETY: d2d_rt owned by self and aliases the atlas texture.
        // Re-clear so stale subpixel coverage doesn't bleed into freshly-
        // allocated slots.
        unsafe {
            self.d2d_rt.BeginDraw();
            self.d2d_rt.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            }));
            let _ = self.d2d_rt.EndDraw(None, None);
        }
    }

    pub fn rebuild(&mut self, font_size_px: f32) -> Result<()> {
        self.font_size_px = font_size_px;
        self.entries.clear();
        self.allocator =
            AtlasAllocator::new(size2(self.atlas_size as i32, self.atlas_size as i32));
        self.text_format_regular = make_text_format(
            &self.dwrite,
            self.bundled_collection.as_ref(),
            self.font_fallback.as_ref(),
            &self.font_family,
            font_size_px,
            false,
            false,
        )?;
        self.text_format_bold = make_text_format(
            &self.dwrite,
            self.bundled_collection.as_ref(),
            self.font_fallback.as_ref(),
            &self.font_family,
            font_size_px,
            true,
            false,
        )?;
        self.text_format_italic = make_text_format(
            &self.dwrite,
            self.bundled_collection.as_ref(),
            self.font_fallback.as_ref(),
            &self.font_family,
            font_size_px,
            false,
            true,
        )?;
        self.text_format_bold_italic = make_text_format(
            &self.dwrite,
            self.bundled_collection.as_ref(),
            self.font_fallback.as_ref(),
            &self.font_family,
            font_size_px,
            true,
            true,
        )?;
        self.metrics = measure_cell(
            &self.dwrite,
            self.bundled_collection.as_ref(),
            &self.font_family,
            font_size_px,
        )?;
        // Refresh the primary face + upm so per-glyph scale-to-fit
        // works after a font-size change (the face itself doesn't
        // depend on size, but re-resolving keeps the field in lockstep
        // with the current collection / family).
        match resolve_font_face(
            &self.dwrite,
            self.bundled_collection.as_ref(),
            &self.font_family,
        ) {
            Ok((face, _src)) => {
                let mut fm = DWRITE_FONT_METRICS::default();
                // SAFETY: windows-rs writes through the out pointer.
                unsafe { face.GetMetrics(&mut fm) };
                self.primary_upm = fm.designUnitsPerEm as f32;
                self.primary_face = Some(face);
            }
            Err(err) => {
                log::warn!(
                    "GlyphCache::rebuild: primary face resolution failed ({err:?}); \
                     wide Nerd-Font icons will render clipped at the cell edge"
                );
                self.primary_face = None;
                self.primary_upm = 1.0;
            }
        }
        // Clear the atlas texture so fresh rasterization doesn't blend
        // with stale pixels from the previous size. BeginDraw/Clear/End
        // is a single D2D op — the DXGI-backed RT aliases the same
        // texture the D3D11 pixel shader samples, so the clear is
        // visible to the next frame.
        //
        // Black (not transparent) — ClearType blends against the RT's
        // current pixels. Starting from (0,0,0) + drawing with a white
        // brush yields atlas values equal to per-subpixel coverage,
        // which is what the PS expects.
        // SAFETY: d2d_rt is owned by self and targets the atlas texture.
        unsafe {
            self.d2d_rt.BeginDraw();
            let black = D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            };
            self.d2d_rt.Clear(Some(&black));
            let _ = self.d2d_rt.EndDraw(None, None);
        }
        let _ = &self.device; // placeholder use so `device` field isn't dead
        Ok(())
    }
}

fn make_text_format(
    dwrite: &IDWriteFactory,
    collection: Option<&IDWriteFontCollection>,
    fallback: Option<&IDWriteFontFallback>,
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

    // Pass the bundled collection when we have one so DWrite resolves
    // the family name against our IoskeleyMono blobs instead of the
    // system font table. `None` falls back to the system collection.
    // SAFETY: both buffers are NUL-terminated wide strings.
    let format = unsafe {
        dwrite.CreateTextFormat(
            windows::core::PCWSTR(family_w.as_ptr()),
            collection,
            weight,
            style,
            DWRITE_FONT_STRETCH_NORMAL,
            size_px,
            windows::core::PCWSTR(locale_w.as_ptr()),
        )
    }
    .context("CreateTextFormat failed")?;

    // Attach the system fallback cascade via IDWriteTextFormat1 (Win8.1+).
    // DirectWrite then substitutes Segoe UI Emoji / Symbol / CJK for
    // codepoints IoskeleyMono lacks — no per-glyph retry needed; the
    // D2D DrawText at rasterize time picks the fallback transparently.
    //
    // Cast failures fall back silently: the format without a fallback
    // still draws primary-font glyphs correctly; only the missing-glyph
    // boxes stay visible.
    if let Some(fb) = fallback {
        if let Ok(fmt1) = format.cast::<IDWriteTextFormat1>() {
            // SAFETY: fmt1 is a valid IDWriteTextFormat1 we own; fb is
            // a live COM reference from GetSystemFontFallback.
            let _ = unsafe { fmt1.SetFontFallback(fb) };
        }
    }

    Ok(format)
}

fn measure_cell(
    dwrite: &IDWriteFactory,
    bundled: Option<&IDWriteFontCollection>,
    family: &str,
    font_size_px: f32,
) -> Result<CellMetrics> {
    // Resolve the advance directly from the font's design metrics.
    //
    // We previously used IDWriteTextLayout::GetMetrics, which returns
    // layout-rounded dimensions from whichever font DWrite's shaper
    // eventually picked. When our bundled IoskeleyMono collection
    // silently fails to match the requested family name (e.g. a
    // mis-patched `name` table or unsupported cast path), DWrite
    // happily falls back to the system collection and returns the
    // width of Segoe UI's 'M' glyph — that's ~28 px at font size 28,
    // roughly 1.6× IoskeleyMono's actual advance, and the whole grid
    // stretches visibly ("W i n d o w s  PowerShell" instead of
    // tightly packed).
    //
    // Design metrics bypass the layout engine entirely: we get the
    // literal advanceWidth stored in the TTF 'hmtx' table, converted
    // from design units to pixels via the font's UPM. For a strict
    // monospace font every glyph has the same advance, so measuring
    // 'M' is representative; a missing-glyph fallback doesn't factor
    // in.
    let (face, resolved) = resolve_font_face(dwrite, bundled, family)?;

    // Font-level design metrics (UPM, ascent, descent, lineGap).
    // SAFETY: windows-rs signature takes a raw out pointer; we pass a
    // sized stack slot.
    let mut fm = DWRITE_FONT_METRICS::default();
    unsafe { face.GetMetrics(&mut fm) };
    let upm = fm.designUnitsPerEm as f32;

    // Resolve 'M' to its glyph index in this face.
    let codepoints: [u32; 1] = ['M' as u32];
    let mut indices: [u16; 1] = [0];
    // SAFETY: both arrays sized 1; DWrite writes one glyph index.
    unsafe { face.GetGlyphIndices(codepoints.as_ptr(), 1, indices.as_mut_ptr()) }
        .context("IDWriteFontFace::GetGlyphIndices failed")?;

    let mut gm = [DWRITE_GLYPH_METRICS::default(); 1];
    // SAFETY: indices and gm are both length 1; issideways=false picks
    // horizontal advance.
    unsafe { face.GetDesignGlyphMetrics(indices.as_ptr(), 1, gm.as_mut_ptr(), false) }
        .context("IDWriteFontFace::GetDesignGlyphMetrics failed")?;

    let advance_du = gm[0].advanceWidth as f32;
    let cell_width_px = ((advance_du * font_size_px) / upm).ceil().max(1.0) as u32;
    // Height from ascent + descent + lineGap, converted to pixels.
    // lineGap can legitimately be negative on some fonts; clamp below.
    let line_metric_du = fm.ascent as f32 + fm.descent as f32 + fm.lineGap as f32;
    let cell_height_px = ((line_metric_du * font_size_px) / upm).ceil().max(1.0) as u32;
    let baseline_px = ((fm.ascent as f32 * font_size_px) / upm).ceil() as u32;

    log::info!(
        "measure_cell: resolved='{}' upm={} M_advance_du={} glyph_idx={} \
         -> cell {}x{} baseline={} @ {}px",
        resolved,
        upm as u32,
        advance_du as u32,
        indices[0],
        cell_width_px,
        cell_height_px,
        baseline_px,
        font_size_px,
    );

    Ok(CellMetrics {
        cell_width_px,
        cell_height_px,
        baseline_px,
    })
}

/// Resolve `family` to a concrete [`IDWriteFontFace`] for measurement.
///
/// Order:
/// 1. Bundled collection (our IoskeleyMono blobs).
/// 2. System collection (for unbundled hosts).
/// 3. System collection → "Segoe UI" (last-resort so we return *some*
///    metrics instead of bailing the entire render setup).
///
/// Returns the face plus a human-readable "where it came from" string
/// so the log line makes the fallback path obvious when we're debugging
/// "cells are twice as wide as they should be" issues.
fn resolve_font_face(
    dwrite: &IDWriteFactory,
    bundled: Option<&IDWriteFontCollection>,
    family: &str,
) -> Result<(IDWriteFontFace, String)> {
    if let Some(coll) = bundled {
        if let Some(face) = find_face_in_collection(coll, family)? {
            return Ok((face, format!("{family} (bundled)")));
        }
        log::warn!(
            "measure_cell: '{family}' not found in bundled collection; \
             falling back to system collection"
        );
    }

    let mut sys: Option<IDWriteFontCollection> = None;
    // SAFETY: out param owned by us; `checkforupdates=false` is the
    // cheap path — we don't care if the system font list changed.
    unsafe { dwrite.GetSystemFontCollection(&mut sys, false) }
        .context("IDWriteFactory::GetSystemFontCollection failed")?;
    let sys = sys.context("GetSystemFontCollection returned None")?;

    if let Some(face) = find_face_in_collection(&sys, family)? {
        return Ok((face, format!("{family} (system)")));
    }

    log::warn!(
        "measure_cell: '{family}' not found in system collection either; \
         using Segoe UI for metrics (cells will be sized for Segoe UI, not {family})"
    );
    if let Some(face) = find_face_in_collection(&sys, "Segoe UI")? {
        return Ok((face, "Segoe UI (last-resort fallback)".to_string()));
    }

    anyhow::bail!(
        "measure_cell: neither '{family}' nor 'Segoe UI' resolved in any font collection"
    );
}

fn find_face_in_collection(
    collection: &IDWriteFontCollection,
    family: &str,
) -> Result<Option<IDWriteFontFace>> {
    let family_w: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
    let mut index: u32 = 0;
    let mut exists = windows::core::BOOL(0);
    // SAFETY: buffers sized above; out params are stack-local.
    unsafe {
        collection.FindFamilyName(
            windows::core::PCWSTR(family_w.as_ptr()),
            &mut index,
            &mut exists,
        )
    }
    .context("IDWriteFontCollection::FindFamilyName failed")?;
    if !exists.as_bool() {
        return Ok(None);
    }

    // SAFETY: index is valid per DWrite contract when exists=TRUE.
    let family_obj =
        unsafe { collection.GetFontFamily(index) }.context("GetFontFamily failed")?;
    // SAFETY: we want regular/normal for the measurement probe — bold
    // and italic have the same advance on a monospace face, so the
    // regular weight is representative and always present.
    let font = unsafe {
        family_obj.GetFirstMatchingFont(
            DWRITE_FONT_WEIGHT_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            DWRITE_FONT_STYLE_NORMAL,
        )
    }
    .context("GetFirstMatchingFont failed")?;
    // SAFETY: font owned above.
    let face = unsafe { font.CreateFontFace() }.context("IDWriteFont::CreateFontFace failed")?;
    Ok(Some(face))
}

/// Pick a family name that actually exists in the bundled collection.
///
/// The GPUI / config layer uses "Ioskeley Mono" (display form), but the
/// bundled TTF `name` table advertises "IoskeleyMono" (one word). Try
/// the verbatim name first, then the whitespace-stripped variant, then
/// give up and return the input — the downstream code will hit the
/// system-cascade fallback path and log a warning.
fn resolve_bundled_family(bundled: Option<&IDWriteFontCollection>, family: &str) -> String {
    let Some(coll) = bundled else {
        return family.to_string();
    };
    if family_exists_in(coll, family) {
        log::info!("resolve_bundled_family: '{family}' matched bundled collection verbatim");
        return family.to_string();
    }
    let stripped: String = family.chars().filter(|c| !c.is_whitespace()).collect();
    if stripped != family && family_exists_in(coll, &stripped) {
        log::info!(
            "resolve_bundled_family: '{family}' missed; '{stripped}' matched bundled \
             collection — using that for DWrite lookups"
        );
        return stripped;
    }
    log::warn!(
        "resolve_bundled_family: neither '{family}' nor its no-space form resolved \
         in the bundled collection; downstream make_text_format / measure_cell will \
         fall back to the system cascade"
    );
    family.to_string()
}

fn family_exists_in(collection: &IDWriteFontCollection, family: &str) -> bool {
    let family_w: Vec<u16> = family.encode_utf16().chain(std::iter::once(0)).collect();
    let mut index: u32 = 0;
    let mut exists = windows::core::BOOL(0);
    // SAFETY: buffer is NUL-terminated; out params are stack-local. A
    // non-Ok HRESULT here is treated as "not found"; we don't want to
    // propagate it because the caller has a well-defined fallback.
    let hr = unsafe {
        collection.FindFamilyName(
            windows::core::PCWSTR(family_w.as_ptr()),
            &mut index,
            &mut exists,
        )
    };
    hr.is_ok() && exists.as_bool()
}
