use gpui::App;
use gpui_component::{Theme, ThemeMode, ThemeRegistry};
use std::borrow::Cow;

const CON_THEME: &str = include_str!("../../../assets/themes/con-dark.json");

// Embed IoskeleyMono font files at compile time.
// Only essential weights: Regular, Bold, Italic, BoldItalic for terminal,
// plus Medium and SemiBold for UI labels.
const FONT_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/IoskeleyMono-Regular.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../../../assets/fonts/IoskeleyMono-Bold.ttf");
const FONT_ITALIC: &[u8] = include_bytes!("../../../assets/fonts/IoskeleyMono-Italic.ttf");
const FONT_BOLD_ITALIC: &[u8] = include_bytes!("../../../assets/fonts/IoskeleyMono-BoldItalic.ttf");
const FONT_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/IoskeleyMono-Medium.ttf");
const FONT_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/IoskeleyMono-SemiBold.ttf");

/// Initialize the con theme system.
///
/// Registers IoskeleyMono fonts, loads the Flexoki-based con dark theme,
/// and activates dark mode. CJK characters fall back to system fonts
/// automatically via GPUI's font-kit backend.
pub fn init_theme(cx: &mut App) {
    // Register embedded fonts with the text system
    cx.text_system()
        .add_fonts(vec![
            Cow::Borrowed(FONT_REGULAR),
            Cow::Borrowed(FONT_BOLD),
            Cow::Borrowed(FONT_ITALIC),
            Cow::Borrowed(FONT_BOLD_ITALIC),
            Cow::Borrowed(FONT_MEDIUM),
            Cow::Borrowed(FONT_SEMIBOLD),
        ])
        .expect("Failed to register IoskeleyMono fonts");

    // Load con theme into the registry
    ThemeRegistry::global_mut(cx)
        .load_themes_from_str(CON_THEME)
        .expect("Failed to load con theme");

    // Set our dark theme as the active dark theme
    if let Some(con_dark) = ThemeRegistry::global(cx)
        .themes()
        .get("Con Dark")
        .cloned()
    {
        Theme::global_mut(cx).dark_theme = con_dark;
    }

    // Activate dark mode
    Theme::change(ThemeMode::Dark, None, cx);
}
