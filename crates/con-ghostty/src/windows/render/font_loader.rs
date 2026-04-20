//! Bundle IoskeleyMono into the DirectWrite font collection.
//!
//! On Windows we can't assume the user has IoskeleyMono installed
//! system-wide. We embed the TTFs at compile time (same blobs GPUI
//! registers for UI text — `crates/con-app/src/theme.rs`) and build a
//! custom `IDWriteFontCollection` via `IDWriteFactory5`'s in-memory
//! loader. The glyph atlas consumes this collection for all
//! `CreateTextFormat` calls so we get the designed terminal font
//! regardless of install state.
//!
//! Returns `None` if the host runtime lacks `IDWriteFactory5` (pre-
//! Windows 10 1607). The caller falls back to the system collection,
//! which on unbundled machines resolves "IoskeleyMono" to a default
//! system font (Segoe / Consolas). We log a warning in that case.

use anyhow::{Context, Result};
use windows::core::Interface;
use windows::Win32::Graphics::DirectWrite::{
    IDWriteFactory, IDWriteFactory2, IDWriteFactory5, IDWriteFontCollection, IDWriteFontFallback,
    IDWriteFontFile, IDWriteFontSet, IDWriteFontSetBuilder1, IDWriteInMemoryFontFileLoader,
};

/// Family name the bundled TTFs advertise (must match the `name` table's
/// family-name record inside the TTF). `IoskeleyMono-*.ttf` follow the
/// Iosevka convention: concatenated family name with no space.
pub const BUNDLED_FONT_FAMILY: &str = "IoskeleyMono";

const FONT_REGULAR: &[u8] =
    include_bytes!("../../../../../assets/fonts/IoskeleyMono-Regular.ttf");
const FONT_BOLD: &[u8] =
    include_bytes!("../../../../../assets/fonts/IoskeleyMono-Bold.ttf");
const FONT_ITALIC: &[u8] =
    include_bytes!("../../../../../assets/fonts/IoskeleyMono-Italic.ttf");
const FONT_BOLD_ITALIC: &[u8] =
    include_bytes!("../../../../../assets/fonts/IoskeleyMono-BoldItalic.ttf");

/// Build a private `IDWriteFontCollection` containing the bundled
/// IoskeleyMono weights. Returns `Ok(None)` when the runtime doesn't
/// support `IDWriteFactory5` (loader API added in Windows 10 1607).
pub fn build_bundled_collection(
    dwrite: &IDWriteFactory,
) -> Result<Option<IDWriteFontCollection>> {
    // Cast up: IDWriteFactory → IDWriteFactory5. The shared factory
    // returned by DWriteCreateFactory on Windows 10+ implements this
    // interface; on older hosts the cast fails and we fall back.
    let factory5: IDWriteFactory5 = match dwrite.cast() {
        Ok(f) => f,
        Err(err) => {
            log::warn!(
                "IoskeleyMono bundling skipped: IDWriteFactory5 not \
                 available ({err:?}); falling back to system font \
                 collection"
            );
            return Ok(None);
        }
    };

    // SAFETY: factory5 owned above; the returned loader is retained by
    // us (and by the factory via RegisterFontFileLoader) for the life
    // of the process.
    let loader: IDWriteInMemoryFontFileLoader =
        unsafe { factory5.CreateInMemoryFontFileLoader() }
            .context("CreateInMemoryFontFileLoader failed")?;
    // SAFETY: loader COM-refcount is bumped by Register; safe to hand
    // the same reference.
    unsafe { factory5.RegisterFontFileLoader(&loader) }
        .context("RegisterFontFileLoader failed")?;

    // SAFETY: factory5 owns the font-set builder. `CreateFontSetBuilder`
    // on `IDWriteFactory5` returns the `...1` flavour in the Win10+ SDK
    // we pin; it inherits from `IDWriteFontSetBuilder`, but windows-rs
    // binds the concrete type so we accept it here.
    let builder: IDWriteFontSetBuilder1 = unsafe { factory5.CreateFontSetBuilder() }
        .context("CreateFontSetBuilder failed")?;

    for (label, bytes) in [
        ("regular", FONT_REGULAR),
        ("bold", FONT_BOLD),
        ("italic", FONT_ITALIC),
        ("bold_italic", FONT_BOLD_ITALIC),
    ] {
        // SAFETY: bytes are `&'static` (from `include_bytes!`), so the
        // pointer stays valid for the process lifetime. `None` owner
        // is fine when the caller guarantees the data outlives the
        // font file reference.
        let file: IDWriteFontFile = unsafe {
            loader.CreateInMemoryFontFileReference(
                dwrite,
                bytes.as_ptr() as *const _,
                bytes.len() as u32,
                None,
            )
        }
        .with_context(|| format!("CreateInMemoryFontFileReference({label}) failed"))?;

        // SAFETY: `file` owned here; `AddFontFile` refcounts internally.
        unsafe { builder.AddFontFile(&file) }
            .with_context(|| format!("FontSetBuilder::AddFontFile({label}) failed"))?;
    }

    // SAFETY: builder valid; CreateFontSet is the terminal op.
    let set: IDWriteFontSet =
        unsafe { builder.CreateFontSet() }.context("CreateFontSet failed")?;

    // SAFETY: set valid. CreateFontCollectionFromFontSet returns an
    // IDWriteFontCollection1 which inherits IDWriteFontCollection.
    let collection = unsafe { factory5.CreateFontCollectionFromFontSet(&set) }
        .context("CreateFontCollectionFromFontSet failed")?;

    log::info!(
        "IoskeleyMono bundled font collection ready: 4 font files \
         (regular/bold/italic/bold-italic)"
    );

    Ok(Some(collection.cast::<IDWriteFontCollection>().unwrap_or_else(
        |_| {
            // This can't fail — IDWriteFontCollection1 inherits from
            // IDWriteFontCollection — but use unwrap_or_else to avoid
            // introducing an Err path.
            unreachable!("IDWriteFontCollection1 → IDWriteFontCollection cast")
        },
    )))
}

/// Return the OS-default [`IDWriteFontFallback`]. The system fallback
/// already knows to cascade through Segoe UI Emoji, Segoe UI Symbol,
/// Segoe UI (Han + Hiragana + Hangul), and the default sans-serif for
/// the active locale — it's the single biggest win for "missing glyph
/// box" bugs and costs zero extra font bytes.
///
/// Returns `None` on pre-Windows-8.1 hosts where `IDWriteFactory2`
/// isn't available — the caller keeps using the bundled-only format
/// and the fallback boxes stay visible. `log::warn` surfaces that so
/// the regression is obvious in logs.
///
/// Nerd-Font-specific glyphs (private-use-area icons used by oh-my-
/// posh / Starship themes) are **not** covered — Windows ships no
/// Nerd Font by default. A follow-up can add a custom fallback builder
/// that prepends a user-installed NF when present.
pub fn system_font_fallback(
    dwrite: &IDWriteFactory,
) -> Option<IDWriteFontFallback> {
    let factory2: IDWriteFactory2 = match dwrite.cast() {
        Ok(f) => f,
        Err(err) => {
            log::warn!(
                "system_font_fallback: IDWriteFactory2 not available \
                 ({err:?}); missing glyphs will render as boxes"
            );
            return None;
        }
    };
    // SAFETY: factory2 owned here; the returned fallback is a COM
    // reference we own for the life of the GlyphCache.
    match unsafe { factory2.GetSystemFontFallback() } {
        Ok(fb) => {
            log::info!("system_font_fallback: installed OS default cascade");
            Some(fb)
        }
        Err(err) => {
            log::warn!(
                "GetSystemFontFallback failed ({err:?}); missing \
                 glyphs will render as boxes"
            );
            None
        }
    }
}
