use gpui::App;
use gpui_component::{Theme, ThemeMode, ThemeRegistry};
use std::borrow::Cow;

const CON_DARK_THEME: &str = include_str!("../../../assets/themes/con-dark.json");
const CON_LIGHT_THEME: &str = include_str!("../../../assets/themes/con-light.json");

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
/// Registers IoskeleyMono fonts, loads both Flexoki-based themes (Light default,
/// Dark available), and activates the mode matching the terminal theme. CJK
/// characters fall back to system fonts automatically via GPUI's font-kit backend.
pub fn init_theme(cx: &mut App, terminal_theme: &str) {
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

    // Load both dark and light themes into the registry
    ThemeRegistry::global_mut(cx)
        .load_themes_from_str(CON_DARK_THEME)
        .expect("Failed to load con dark theme");
    ThemeRegistry::global_mut(cx)
        .load_themes_from_str(CON_LIGHT_THEME)
        .expect("Failed to load con light theme");

    // Set our themes as the active dark/light themes
    // Clone out of registry before taking mutable borrow on Theme
    let con_dark = ThemeRegistry::global(cx).themes().get("Con Dark").cloned();
    let con_light = ThemeRegistry::global(cx).themes().get("Con Light").cloned();
    if let Some(dark) = con_dark {
        Theme::global_mut(cx).dark_theme = dark;
    }
    if let Some(light) = con_light {
        Theme::global_mut(cx).light_theme = light;
    }

    // Activate mode matching the terminal theme
    let mode = if terminal_theme.contains("light") {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };
    Theme::change(mode, None, cx);
}

/// Switch the GPUI theme mode to match a terminal theme.
/// Call this when the user selects a terminal theme to keep UI and terminal in sync.
pub fn sync_gpui_mode(terminal_theme_name: &str, window: &mut gpui::Window, cx: &mut gpui::App) {
    let mode = if terminal_theme_name.contains("light") {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };
    Theme::change(mode, Some(window), cx);
}
