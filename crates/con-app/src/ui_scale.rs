use gpui::{Pixels, px};
use gpui_component::Theme;

const DEFAULT_UI_FONT_SIZE: f32 = 16.0;
const DEFAULT_MONO_FONT_SIZE: f32 = 13.0;
const MIN_FONT_SCALE: f32 = 0.85;
const MAX_FONT_SCALE: f32 = 1.70;

pub(crate) fn ui_font_scale(theme: &Theme) -> f32 {
    font_scale(theme.font_size.as_f32(), DEFAULT_UI_FONT_SIZE)
}

pub(crate) fn mono_font_scale(theme: &Theme) -> f32 {
    font_scale(theme.mono_font_size.as_f32(), DEFAULT_MONO_FONT_SIZE)
}

pub(crate) fn ui_px(theme: &Theme, base_px: f32) -> Pixels {
    px(base_px * ui_font_scale(theme))
}

pub(crate) fn mono_px(theme: &Theme, base_px: f32) -> Pixels {
    px(base_px * mono_font_scale(theme))
}

fn font_scale(current_px: f32, default_px: f32) -> f32 {
    if !current_px.is_finite() || !default_px.is_finite() || default_px <= 0.0 {
        return 1.0;
    }

    (current_px / default_px).clamp(MIN_FONT_SCALE, MAX_FONT_SCALE)
}

#[cfg(test)]
mod tests {
    use super::font_scale;

    #[test]
    fn font_scale_preserves_default_and_clamps_extremes() {
        assert_eq!(font_scale(16.0, 16.0), 1.0);
        assert_eq!(font_scale(1.0, 16.0), 0.85);
        assert_eq!(font_scale(200.0, 16.0), 1.70);
    }

    #[test]
    fn font_scale_ignores_invalid_values() {
        assert_eq!(font_scale(f32::NAN, 16.0), 1.0);
        assert_eq!(font_scale(16.0, 0.0), 1.0);
    }
}
