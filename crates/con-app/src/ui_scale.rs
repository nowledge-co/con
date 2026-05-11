use con_core::config::{MAX_UI_FONT_SIZE, MIN_UI_FONT_SIZE};
use gpui::{Pixels, px};
use gpui_component::Theme;

const DEFAULT_UI_FONT_SIZE: f32 = 16.0;
const DEFAULT_MONO_FONT_SIZE: f32 = 13.0;
const MIN_DENSITY_SCALE: f32 = 0.92;
const MAX_DENSITY_SCALE: f32 = 1.25;
const DENSITY_SCALE_WEIGHT: f32 = 0.45;

pub(crate) fn ui_font_scale(theme: &Theme) -> f32 {
    font_scale(
        theme.font_size.as_f32(),
        DEFAULT_UI_FONT_SIZE,
        MIN_UI_FONT_SIZE / DEFAULT_UI_FONT_SIZE,
        MAX_UI_FONT_SIZE / DEFAULT_UI_FONT_SIZE,
    )
}

pub(crate) fn mono_font_scale(theme: &Theme) -> f32 {
    font_scale(
        theme.mono_font_size.as_f32(),
        DEFAULT_MONO_FONT_SIZE,
        (MIN_UI_FONT_SIZE - 1.0) / DEFAULT_MONO_FONT_SIZE,
        (MAX_UI_FONT_SIZE - 3.0) / DEFAULT_MONO_FONT_SIZE,
    )
}

pub(crate) fn ui_density_scale(theme: &Theme) -> f32 {
    density_scale(ui_font_scale(theme))
}

pub(crate) fn mono_density_scale(theme: &Theme) -> f32 {
    density_scale(mono_font_scale(theme))
}

pub(crate) fn ui_px(theme: &Theme, base_px: f32) -> Pixels {
    px(base_px * ui_font_scale(theme))
}

pub(crate) fn mono_px(theme: &Theme, base_px: f32) -> Pixels {
    px(base_px * mono_font_scale(theme))
}

pub(crate) fn ui_space_px(theme: &Theme, base_px: f32) -> Pixels {
    px(base_px * ui_density_scale(theme))
}

pub(crate) fn mono_space_px(theme: &Theme, base_px: f32) -> Pixels {
    px(base_px * mono_density_scale(theme))
}

fn font_scale(current_px: f32, default_px: f32, min_scale: f32, max_scale: f32) -> f32 {
    if !current_px.is_finite()
        || !default_px.is_finite()
        || !min_scale.is_finite()
        || !max_scale.is_finite()
        || default_px <= 0.0
        || min_scale > max_scale
    {
        return 1.0;
    }

    (current_px / default_px).clamp(min_scale, max_scale)
}

fn density_scale(font_scale: f32) -> f32 {
    if !font_scale.is_finite() {
        return 1.0;
    }

    (1.0 + (font_scale - 1.0) * DENSITY_SCALE_WEIGHT).clamp(MIN_DENSITY_SCALE, MAX_DENSITY_SCALE)
}

#[cfg(test)]
mod tests {
    use super::{density_scale, font_scale};

    #[test]
    fn font_scale_preserves_default_and_clamps_extremes() {
        assert_eq!(font_scale(16.0, 16.0, 0.75, 1.5), 1.0);
        assert_eq!(font_scale(1.0, 16.0, 0.75, 1.5), 0.75);
        assert_eq!(font_scale(200.0, 16.0, 0.75, 1.5), 1.5);
    }

    #[test]
    fn font_scale_ignores_invalid_values() {
        assert_eq!(font_scale(f32::NAN, 16.0, 0.75, 1.5), 1.0);
        assert_eq!(font_scale(16.0, 0.0, 0.75, 1.5), 1.0);
        assert_eq!(font_scale(16.0, 16.0, 1.5, 0.75), 1.0);
    }

    #[test]
    fn density_scale_grows_slower_than_text() {
        assert_eq!(density_scale(1.0), 1.0);
        assert!(density_scale(1.5) < 1.5);
        assert_eq!(density_scale(1.7), 1.25);
    }
}
