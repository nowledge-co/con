//! Public facade: `WindowsGhosttyApp` / `WindowsGhosttyTerminal` that
//! the rest of the workspace consumes through the same `GhosttyApp` /
//! `GhosttyTerminal` type names that the macOS path uses (re-exported
//! from `crate::lib`). This keeps callers in `con-app` shape-identical
//! across platforms.
//!
//! Today this is a thin owner of [`super::host_view::HostView`]; later
//! work fills in update_appearance theming, multi-pane semantics, and
//! the OSC 133 / shell-integration callback wiring.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use super::host_view::HostView;
use super::render::RendererConfig;
use crate::stub::{
    CommandFinishedSignal, CommandRecord, GhosttyConfigPatch, GhosttySplitDirection,
    GhosttySurfaceEvent, MouseButton, SurfaceSize, TerminalColors,
};

/// One per GPUI window. Holds shared, app-wide terminal config.
pub struct WindowsGhosttyApp {
    config: Mutex<RendererConfig>,
}

impl WindowsGhosttyApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        _colors: Option<&TerminalColors>,
        font_family: Option<&str>,
        font_size: Option<f32>,
        _background_opacity: Option<f32>,
        _background_blur: Option<bool>,
        _cursor_style: Option<&str>,
        _background_image: Option<&str>,
        _background_image_opacity: Option<f32>,
        _background_image_position: Option<&str>,
        _background_image_fit: Option<&str>,
        _background_image_repeat: Option<bool>,
    ) -> Result<Self, String> {
        let mut config = RendererConfig::default();
        if let Some(family) = font_family {
            config.font_family = family.to_string();
        }
        if let Some(size) = font_size {
            config.font_size_px = size;
        }
        Ok(Self {
            config: Mutex::new(config),
        })
    }

    pub fn tick(&self) {
        // No periodic work needed yet; the host_view's WM_PAINT loop
        // drives rendering. Reserved for future timer-driven effects
        // (cursor blink, OSC 8 hyperlink invalidation, etc.).
    }

    pub fn update_colors(&self, _colors: &TerminalColors) -> Result<(), String> {
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_appearance(
        &self,
        _colors: &TerminalColors,
        font_family: &str,
        font_size: f32,
        _background_opacity: f32,
        _background_blur: bool,
        _cursor_style: &str,
        _background_image: Option<&str>,
        _background_image_opacity: f32,
        _background_image_position: Option<&str>,
        _background_image_fit: Option<&str>,
        _background_image_repeat: bool,
    ) -> Result<(), String> {
        let mut config = self.config.lock();
        config.font_family = font_family.to_string();
        config.font_size_px = font_size;
        Ok(())
    }

    pub fn update_config(&self, _patch: &GhosttyConfigPatch) -> Result<(), String> {
        Ok(())
    }

    pub fn set_color_scheme(&self, _dark: bool) {}

    /// Snapshot the current renderer config — used by `WindowsTerminalView`
    /// when constructing a new `HostView`.
    pub fn renderer_config(&self) -> RendererConfig {
        self.config.lock().clone()
    }
}

unsafe impl Send for WindowsGhosttyApp {}
unsafe impl Sync for WindowsGhosttyApp {}

/// One per pane. The `HostView` (HWND + renderer + ConPTY + parser) is
/// constructed lazily by the GPUI element when it first lays out (we
/// need the parent HWND from GPUI).
pub struct WindowsGhosttyTerminal {
    inner: Arc<Mutex<Option<HostView>>>,
}

impl WindowsGhosttyTerminal {
    /// Construct an empty terminal handle. The actual HWND/renderer is
    /// installed via [`WindowsGhosttyTerminal::attach`] from the GPUI
    /// view once it has the parent HWND.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Attach a live `HostView`. Idempotent: replaces any existing one.
    pub fn attach(&self, host: HostView) {
        *self.inner.lock() = Some(host);
    }

    /// Has the GPUI element installed a HostView yet?
    pub fn is_attached(&self) -> bool {
        self.inner.lock().is_some()
    }

    pub fn draw(&self) {}
    pub fn refresh(&self) {}
    pub fn set_size(&self, _w: u32, _h: u32) {}

    pub fn size(&self) -> SurfaceSize {
        SurfaceSize {
            columns: 0,
            rows: 0,
            width_px: 0,
            height_px: 0,
            cell_width_px: 0,
            cell_height_px: 0,
        }
    }

    pub fn set_content_scale(&self, _scale: f64) {}
    pub fn set_focus(&self, _focused: bool) {}
    pub fn set_occlusion(&self, _occluded: bool) {}
    pub fn set_color_scheme(&self, _dark: bool) {}
    pub fn perform_binding_action(&self, _action: &str) -> Result<bool, String> {
        Ok(false)
    }
    pub fn clear_screen_and_scrollback(&self) -> Result<(), String> {
        Ok(())
    }
    pub fn request_split(&self, _direction: GhosttySplitDirection) {}
    pub fn update_config(&self, _patch: &GhosttyConfigPatch) -> Result<(), String> {
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_appearance(
        &self,
        _colors: &TerminalColors,
        _font_family: &str,
        _font_size: f32,
        _background_opacity: f32,
        _background_blur: bool,
        _cursor_style: &str,
        _background_image: Option<&str>,
        _background_image_opacity: f32,
        _background_image_position: Option<&str>,
        _background_image_fit: Option<&str>,
        _background_image_repeat: bool,
    ) -> Result<(), String> {
        Ok(())
    }

    pub fn write_to_pty(&self, data: &[u8]) {
        if let Some(host) = self.inner.lock().as_ref() {
            // The HostView's write_input takes a UTF-8 string; we have
            // bytes (which the caller already encoded as UTF-8 — see
            // `terminal_pane::write`). Round-trip through str if valid;
            // drop otherwise (matches macOS behavior of treating the
            // PTY as a UTF-8 byte stream from the host's perspective).
            if let Ok(s) = std::str::from_utf8(data) {
                host.write_input(s);
            }
        }
    }

    pub fn send_text(&self, text: &str) {
        if let Some(host) = self.inner.lock().as_ref() {
            host.write_input(text);
        }
    }

    pub fn send_mouse_button(&self, _pressed: bool, _button: MouseButton, _mods: i32) -> bool {
        false
    }
    pub fn send_mouse_pos(&self, _x: f64, _y: f64, _mods: i32) {}
    pub fn send_mouse_scroll(&self, _x: f64, _y: f64, _mods: i32) {}
    pub fn request_close(&self) {
        // Dropping the inner HostView destroys the HWND, which closes
        // the swapchain and the ConPTY (which terminates the child).
        *self.inner.lock() = None;
    }

    pub fn title(&self) -> Option<String> {
        None
    }
    pub fn current_dir(&self) -> Option<String> {
        None
    }
    pub fn is_alive(&self) -> bool {
        self.is_attached()
    }
    pub fn is_busy(&self) -> bool {
        false
    }
    pub fn command_history(&self) -> Vec<CommandRecord> {
        Vec::new()
    }
    pub fn take_command_finished(&self) -> Option<CommandFinishedSignal> {
        None
    }
    pub fn last_exit_code(&self) -> Option<i32> {
        None
    }
    pub fn last_command_duration(&self) -> Option<Duration> {
        None
    }
    pub fn input_generation(&self) -> u64 {
        0
    }
    pub fn last_command_finished_input_generation(&self) -> u64 {
        0
    }
    pub fn recover_shell_prompt_state(&self) {}
    pub fn has_selection(&self) -> bool {
        false
    }
    pub fn selection_text(&self) -> Option<String> {
        None
    }
    pub fn read_screen_text(&self, _max_lines: usize) -> Vec<String> {
        Vec::new()
    }
    pub fn read_recent_lines(&self, _max_lines: usize) -> Vec<String> {
        Vec::new()
    }
    pub fn search_text(&self, _pattern: &str, _limit: usize) -> Vec<(usize, String)> {
        Vec::new()
    }
    pub fn take_needs_render(&self) -> bool {
        false
    }
    pub fn take_pending_events(&self) -> Vec<GhosttySurfaceEvent> {
        Vec::new()
    }

    /// Internal accessor used by `WindowsTerminalView` to install the
    /// `HostView` once GPUI gives us the parent HWND.
    pub fn inner(&self) -> Arc<Mutex<Option<HostView>>> {
        self.inner.clone()
    }
}

impl Default for WindowsGhosttyTerminal {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Send for WindowsGhosttyTerminal {}
unsafe impl Sync for WindowsGhosttyTerminal {}
