use gpui::*;

/// Catppuccin Mocha theme colors
pub struct Theme;

impl Theme {
    // Base colors
    pub fn base() -> u32 {
        0x1e1e2e
    }
    pub fn mantle() -> u32 {
        0x181825
    }
    pub fn crust() -> u32 {
        0x11111b
    }
    pub fn surface0() -> u32 {
        0x313244
    }
    pub fn surface1() -> u32 {
        0x45475a
    }
    pub fn surface2() -> u32 {
        0x585b70
    }

    // Text colors
    pub fn text() -> u32 {
        0xcdd6f4
    }
    pub fn subtext0() -> u32 {
        0xa6adc8
    }
    pub fn subtext1() -> u32 {
        0xbac2de
    }
    pub fn overlay0() -> u32 {
        0x6c7086
    }

    // Accent colors
    pub fn blue() -> u32 {
        0x89b4fa
    }
    pub fn green() -> u32 {
        0xa6e3a1
    }
    pub fn red() -> u32 {
        0xf38ba8
    }
    pub fn yellow() -> u32 {
        0xf9e2af
    }
    pub fn mauve() -> u32 {
        0xcba6f7
    }
    pub fn teal() -> u32 {
        0x94e2d5
    }
    pub fn peach() -> u32 {
        0xfab387
    }
    pub fn lavender() -> u32 {
        0xb4befe
    }
}
