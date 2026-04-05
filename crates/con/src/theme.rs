use con_terminal::{Color, TerminalTheme};
use gpui::App;
use gpui_component::{Theme, ThemeMode, ThemeRegistry};
use std::borrow::Cow;

const CON_DARK_THEME: &str = include_str!("../../../assets/themes/con-dark.json");
const CON_LIGHT_THEME: &str = include_str!("../../../assets/themes/con-light.json");
const CATPPUCCIN_THEME: &str = include_str!("../../../assets/themes/catppuccin-mocha.json");
const TOKYONIGHT_THEME: &str = include_str!("../../../assets/themes/tokyonight.json");

// Embed IoskeleyMono font files at compile time.
const FONT_REGULAR: &[u8] = include_bytes!("../../../assets/fonts/IoskeleyMono-Regular.ttf");
const FONT_BOLD: &[u8] = include_bytes!("../../../assets/fonts/IoskeleyMono-Bold.ttf");
const FONT_ITALIC: &[u8] = include_bytes!("../../../assets/fonts/IoskeleyMono-Italic.ttf");
const FONT_BOLD_ITALIC: &[u8] = include_bytes!("../../../assets/fonts/IoskeleyMono-BoldItalic.ttf");
const FONT_MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/IoskeleyMono-Medium.ttf");
const FONT_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/IoskeleyMono-SemiBold.ttf");

/// Initialize the con theme system.
///
/// Registers IoskeleyMono fonts, loads built-in themes, and activates the
/// mode matching the terminal theme.
pub fn init_theme(cx: &mut App, terminal_theme: &str) {
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

    for theme_json in [
        CON_DARK_THEME,
        CON_LIGHT_THEME,
        CATPPUCCIN_THEME,
        TOKYONIGHT_THEME,
    ] {
        ThemeRegistry::global_mut(cx)
            .load_themes_from_str(theme_json)
            .expect("Failed to load theme");
    }

    // For init, generate from the resolved terminal theme
    if let Some(tt) = TerminalTheme::by_name(terminal_theme) {
        apply_dynamic_theme(&tt, cx);
    } else {
        apply_gpui_theme_by_name(terminal_theme, cx);
    }
    let mode = if terminal_theme.contains("light") {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };
    Theme::change(mode, None, cx);
}

/// Switch the GPUI theme to match a terminal theme.
/// Generates a dynamic GPUI theme from the terminal theme's colors.
pub fn sync_gpui_theme(
    terminal_theme: &TerminalTheme,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) {
    apply_dynamic_theme(terminal_theme, cx);
    let mode = if terminal_theme.name.contains("light") {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };
    Theme::change(mode, Some(window), cx);
}

/// Generate a GPUI theme dynamically from terminal ANSI colors and register it.
fn apply_dynamic_theme(tt: &TerminalTheme, cx: &mut App) {
    let json = generate_gpui_theme_json(tt);
    // Register (or re-register) the dynamic theme
    // ThemeRegistry skips duplicates, so we directly set the theme config
    match serde_json::from_str::<serde_json::Value>(&json) {
        Ok(val) => {
            if let Ok(theme_set) = serde_json::from_value::<gpui_component::ThemeSet>(val) {
                for theme_config in theme_set.themes {
                    let rc = std::rc::Rc::new(theme_config);
                    if tt.name.contains("light") {
                        Theme::global_mut(cx).light_theme = rc;
                    } else {
                        Theme::global_mut(cx).dark_theme = rc;
                    }
                }
            }
        }
        Err(e) => {
            log::error!("Failed to parse generated theme JSON: {e}");
            apply_gpui_theme_by_name(&tt.name, cx);
        }
    }
}

/// Fallback: map terminal theme name to a pre-registered GPUI theme.
fn apply_gpui_theme_by_name(terminal_theme_name: &str, cx: &mut App) {
    let (dark_name, light_name) = match terminal_theme_name {
        "catppuccin-mocha" => ("Catppuccin Mocha", "Con Light"),
        "tokyonight" => ("Tokyo Night", "Con Light"),
        _ => ("Con Dark", "Con Light"),
    };
    if let Some(d) = ThemeRegistry::global(cx).themes().get(dark_name).cloned() {
        Theme::global_mut(cx).dark_theme = d;
    }
    if let Some(l) = ThemeRegistry::global(cx).themes().get(light_name).cloned() {
        Theme::global_mut(cx).light_theme = l;
    }
}

/// Generate a complete GPUI theme JSON string from terminal theme colors.
///
/// Maps terminal ANSI palette to GPUI semantic colors:
/// - primary = cyan (ansi[6]) — the accent color
/// - danger = red (ansi[1])
/// - success = green (ansi[2])
/// - warning = yellow (ansi[3])
/// - info = blue (ansi[4])
/// - Surface colors derived from bg/fg with blending
fn generate_gpui_theme_json(tt: &TerminalTheme) -> String {
    let bg = tt.background;
    let fg = tt.foreground;
    let red = tt.ansi[1];
    let green = tt.ansi[2];
    let yellow = tt.ansi[3];
    let blue = tt.ansi[4];
    let magenta = tt.ansi[5];
    let cyan = tt.ansi[6];

    let is_dark = is_dark_color(bg);
    let mode = if is_dark { "dark" } else { "light" };

    // Generate surface colors by blending bg toward fg
    let surface1 = blend(bg, fg, 0.06);
    let surface2 = blend(bg, fg, 0.12);
    let surface3 = blend(bg, fg, 0.18);
    let muted_fg = blend(fg, bg, 0.45);

    // Primary contrast — use bg as text on primary buttons for max contrast
    let primary_fg = bg;
    let primary_hover = if is_dark {
        darken(cyan, 0.15)
    } else {
        darken(cyan, 0.12)
    };
    let primary_active = darken(cyan, 0.25);

    // Danger hover/active
    let danger_hover = if is_dark {
        lighten(red, 0.1)
    } else {
        darken(red, 0.1)
    };
    let danger_active = darken(red, 0.2);

    let theme_name = format!("con-gen-{}", tt.name);

    format!(
        r#"{{
  "name": "{theme_name}",
  "author": "con (generated)",
  "themes": [
    {{
      "name": "{theme_name}",
      "mode": "{mode}",
      "is_default": false,
      "font.size": 14,
      "font.family": ".SystemUIFont",
      "mono_font.family": "Ioskeley Mono",
      "mono_font.size": 14,
      "radius": 8,
      "radius_lg": 12,
      "shadow": false,
      "colors": {{
        "background": "{bg_hex}",
        "foreground": "{fg_hex}",
        "border": "{border}",
        "input.border": "{border}",
        "caret": "{cyan_hex}",
        "ring": "{cyan_hex}",

        "primary.background": "{cyan_hex}",
        "primary.foreground": "{primary_fg_hex}",
        "primary.hover.background": "{primary_hover_hex}",
        "primary.active.background": "{primary_active_hex}",

        "secondary.background": "{surface1_hex}",
        "secondary.foreground": "{fg_hex}",
        "secondary.hover.background": "{surface2_hex}",
        "secondary.active.background": "{surface3_hex}",

        "muted.background": "{surface1_hex}",
        "muted.foreground": "{muted_fg_hex}",

        "accent.background": "{surface3_hex}",
        "accent.foreground": "{fg_hex}",

        "success.background": "{green_hex}",
        "success.foreground": "{primary_fg_hex}",

        "danger.background": "{red_hex}",
        "danger.foreground": "{primary_fg_hex}",
        "danger.hover.background": "{danger_hover_hex}",
        "danger.active.background": "{danger_active_hex}",

        "warning.background": "{yellow_hex}",
        "warning.foreground": "{primary_fg_hex}",

        "info.background": "{blue_hex}",
        "info.foreground": "{primary_fg_hex}",

        "title_bar.background": "{surface1_hex}",
        "title_bar.border": "{border}",

        "sidebar.background": "{surface1_hex}",
        "sidebar.foreground": "{fg_hex}",
        "sidebar.border": "{border}",

        "list.active.background": "{list_active}",

        "selection.background": "{selection}",

        "scrollbar.thumb.background": "{scrollbar}",
        "scrollbar.thumb.hover.background": "{surface3_hex}",

        "base.red": "{red_hex}",
        "base.orange": "{orange_hex}",
        "base.yellow": "{yellow_hex}",
        "base.green": "{green_hex}",
        "base.cyan": "{cyan_hex}",
        "base.blue": "{blue_hex}",
        "base.purple": "{purple_hex}",
        "base.magenta": "{magenta_hex}"
      }},
      "highlight": {{
        "editor.foreground": "{fg_hex}",
        "editor.background": "{bg_hex}",
        "editor.active_line.background": "{surface1_hex}",
        "editor.line_number": "{muted_fg_hex}",
        "editor.active_line_number": "{cyan_hex}",
        "editor.invisible": "{muted_invisible}",
        "conflict": "{yellow_hex}",
        "created": "{green_hex}",
        "hidden": "{muted_fg_hex}",
        "hint": "{muted_fg_hex}",
        "modified": "{orange_hex}",
        "predictive": "{muted_fg_hex}",
        "warning": "{yellow_hex}",
        "syntax": {{
          "attribute": {{ "color": "{blue_hex}" }},
          "boolean": {{ "color": "{yellow_hex}" }},
          "comment": {{ "color": "{muted_fg_hex}" }},
          "comment.doc": {{ "color": "{muted_fg_hex}" }},
          "constant": {{ "color": "{orange_hex}" }},
          "constructor": {{ "color": "{blue_hex}" }},
          "emphasis": {{ "color": "{cyan_hex}", "font_style": "italic" }},
          "emphasis.strong": {{ "color": "{cyan_hex}", "font_weight": 700 }},
          "enum": {{ "color": "{yellow_hex}" }},
          "function": {{ "color": "{orange_hex}" }},
          "hint": {{ "color": "{muted_fg_hex}" }},
          "keyword": {{ "color": "{green_hex}" }},
          "label": {{ "color": "{blue_hex}" }},
          "link_text": {{ "color": "{cyan_hex}" }},
          "link_uri": {{ "color": "{cyan_hex}" }},
          "number": {{ "color": "{purple_hex}" }},
          "operator": {{ "color": "{muted_fg_hex}" }},
          "predictive": {{ "color": "{muted_fg_hex}" }},
          "preproc": {{ "color": "{magenta_hex}" }},
          "primary": {{ "color": "{cyan_hex}" }},
          "property": {{ "color": "{orange_hex}" }},
          "punctuation": {{ "color": "{muted_fg_hex}" }},
          "punctuation.bracket": {{ "color": "{muted_fg_hex}" }},
          "punctuation.delimiter": {{ "color": "{muted_fg_hex}" }},
          "string": {{ "color": "{cyan_hex}" }},
          "string.escape": {{ "color": "{cyan_hex}" }},
          "string.regex": {{ "color": "{cyan_hex}" }},
          "string.special": {{ "color": "{cyan_hex}" }},
          "tag": {{ "color": "{blue_hex}" }},
          "text.literal": {{ "color": "{cyan_hex}" }},
          "title": {{ "color": "{yellow_hex}" }},
          "type": {{ "color": "{yellow_hex}" }},
          "variable": {{ "color": "{blue_hex}" }},
          "variable.special": {{ "color": "{blue_hex}" }},
          "variant": {{ "color": "{cyan_hex}" }}
        }}
      }}
    }}
  ]
}}"#,
        bg_hex = hex(bg),
        fg_hex = hex(fg),
        border = hex(surface2),
        cyan_hex = hex(cyan),
        primary_fg_hex = hex(primary_fg),
        primary_hover_hex = hex(primary_hover),
        primary_active_hex = hex(primary_active),
        surface1_hex = hex(surface1),
        surface2_hex = hex(surface2),
        surface3_hex = hex(surface3),
        muted_fg_hex = hex(muted_fg),
        red_hex = hex(red),
        green_hex = hex(green),
        yellow_hex = hex(yellow),
        blue_hex = hex(blue),
        magenta_hex = hex(magenta),
        danger_hover_hex = hex(danger_hover),
        danger_active_hex = hex(danger_active),
        orange_hex = hex_rgb(
            tt.ansi[9].r.max(tt.ansi[3].r),
            tt.ansi[9].g.min(tt.ansi[3].g),
            tt.ansi[9].b.min(tt.ansi[3].b)
        ),
        purple_hex = hex(tt.ansi[13]),
        list_active = hex_alpha(cyan, 0x18),
        selection = hex_alpha(cyan, 0x28),
        scrollbar = hex_alpha(surface2, 0x80),
        muted_invisible = hex_alpha(muted_fg, 0x66),
    )
}

// ── Color helpers ──────────────────────────────────────────────

fn hex(c: Color) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

fn hex_alpha(c: Color, alpha: u8) -> String {
    format!("#{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, alpha)
}

#[allow(clippy::many_single_char_names)]
fn hex_rgb(r: u8, g: u8, b: u8) -> String {
    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

fn is_dark_color(c: Color) -> bool {
    let luma = 0.299 * c.r as f64 + 0.587 * c.g as f64 + 0.114 * c.b as f64;
    luma < 128.0
}

fn blend(base: Color, target: Color, amount: f64) -> Color {
    let r = (base.r as f64 + (target.r as f64 - base.r as f64) * amount) as u8;
    let g = (base.g as f64 + (target.g as f64 - base.g as f64) * amount) as u8;
    let b = (base.b as f64 + (target.b as f64 - base.b as f64) * amount) as u8;
    Color::rgb(r, g, b)
}

fn darken(c: Color, amount: f64) -> Color {
    let factor = 1.0 - amount;
    Color::rgb(
        (c.r as f64 * factor) as u8,
        (c.g as f64 * factor) as u8,
        (c.b as f64 * factor) as u8,
    )
}

fn lighten(c: Color, amount: f64) -> Color {
    Color::rgb(
        (c.r as f64 + (255.0 - c.r as f64) * amount) as u8,
        (c.g as f64 + (255.0 - c.g as f64) * amount) as u8,
        (c.b as f64 + (255.0 - c.b as f64) * amount) as u8,
    )
}
