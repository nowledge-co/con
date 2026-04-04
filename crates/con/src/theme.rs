use gpui::App;
use gpui_component::{Theme, ThemeMode, ThemeRegistry};
use std::borrow::Cow;

const CON_DARK_THEME: &str = include_str!("../../../assets/themes/con-dark.json");
const CON_LIGHT_THEME: &str = include_str!("../../../assets/themes/con-light.json");
const CATPPUCCIN_THEME: &str = include_str!("../../../assets/themes/catppuccin-mocha.json");
const TOKYONIGHT_THEME: &str = include_str!("../../../assets/themes/tokyonight.json");

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

    // Load all themes into the registry
    for theme_json in [CON_DARK_THEME, CON_LIGHT_THEME, CATPPUCCIN_THEME, TOKYONIGHT_THEME] {
        ThemeRegistry::global_mut(cx)
            .load_themes_from_str(theme_json)
            .expect("Failed to load theme");
    }

    // Set initial dark/light themes based on the terminal theme
    apply_gpui_theme(terminal_theme, cx);
    let mode = if terminal_theme.contains("light") {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };
    Theme::change(mode, None, cx);
}

/// Switch the GPUI theme to match a terminal theme.
/// Swaps both the active dark/light theme AND the mode.
pub fn sync_gpui_mode(terminal_theme_name: &str, window: &mut gpui::Window, cx: &mut gpui::App) {
    apply_gpui_theme(terminal_theme_name, cx);
    let mode = if terminal_theme_name.contains("light") {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };
    Theme::change(mode, Some(window), cx);
}

/// Map a terminal theme name to the corresponding GPUI theme and set it as active.
fn apply_gpui_theme(terminal_theme_name: &str, cx: &mut gpui::App) {
    let (dark_name, light_name) = match terminal_theme_name {
        "catppuccin-mocha" => ("Catppuccin Mocha", "Con Light"),
        "tokyonight" => ("Tokyo Night", "Con Light"),
        "flexoki-dark" => ("Con Dark", "Con Light"),
        "flexoki-light" => ("Con Dark", "Con Light"),
        name if name.contains("light") => ("Con Dark", "Con Light"),
        _ => ("Con Dark", "Con Light"),
    };

    let dark = ThemeRegistry::global(cx).themes().get(dark_name).cloned();
    let light = ThemeRegistry::global(cx).themes().get(light_name).cloned();

    if let Some(d) = dark {
        Theme::global_mut(cx).dark_theme = d;
    }
    if let Some(l) = light {
        Theme::global_mut(cx).light_theme = l;
    }
}
