use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;

use super::pty::{LinuxPtyOptions, LinuxPtySession};
use crate::stub::{
    CommandFinishedSignal, CommandRecord, GhosttyConfigPatch, GhosttySplitDirection,
    GhosttySurfaceEvent, MouseButton, SurfaceSize, TerminalColors,
};
use crate::vt::ScreenSnapshot;

#[derive(Debug, Clone, Default)]
pub struct LinuxBackendConfig {
    pub shell_program: Option<String>,
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    pub colors: Option<TerminalColors>,
}

/// One per GPUI window. Holds Linux backend configuration that future
/// PTY and renderer setup can consume.
pub struct LinuxGhosttyApp {
    config: Mutex<LinuxBackendConfig>,
    wake_generation: Arc<AtomicU64>,
}

impl LinuxGhosttyApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        colors: Option<&TerminalColors>,
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
        Ok(Self {
            config: Mutex::new(LinuxBackendConfig {
                shell_program: default_linux_shell_program(),
                font_family: font_family.map(ToOwned::to_owned),
                font_size,
                colors: colors.cloned(),
            }),
            wake_generation: Arc::new(AtomicU64::new(1)),
        })
    }

    pub fn tick(&self) {}

    pub fn wake_generation(&self) -> u64 {
        self.wake_generation.load(Ordering::Acquire)
    }

    pub fn update_colors(&self, colors: &TerminalColors) -> Result<(), String> {
        let mut config = self.config.lock();
        config.colors = Some(colors.clone());
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_appearance(
        &self,
        colors: &TerminalColors,
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
        config.font_family = Some(font_family.to_string());
        config.font_size = Some(font_size);
        config.colors = Some(colors.clone());
        Ok(())
    }

    pub fn update_config(&self, _patch: &GhosttyConfigPatch) -> Result<(), String> {
        Ok(())
    }

    pub fn set_color_scheme(&self, _dark: bool) {}

    pub fn backend_config(&self) -> LinuxBackendConfig {
        self.config.lock().clone()
    }

    pub fn default_pty_options(&self, cwd: Option<&str>) -> LinuxPtyOptions {
        let config = self.backend_config();
        LinuxPtyOptions {
            cwd: cwd.map(PathBuf::from),
            program: config.shell_program,
            wake_generation: Some(self.wake_generation.clone()),
            theme: config.colors,
            ..LinuxPtyOptions::default()
        }
    }
}

unsafe impl Send for LinuxGhosttyApp {}
unsafe impl Sync for LinuxGhosttyApp {}

fn default_linux_shell_program() -> Option<String> {
    if let Some(shell) = std::env::var("CON_LINUX_SHELL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Some(shell);
    }

    if let Some(shell) = std::env::var("SHELL")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return Some(shell);
    }

    for candidate in ["/bin/bash", "/usr/bin/bash", "/bin/sh", "/usr/bin/sh"] {
        if PathBuf::from(candidate).exists() {
            return Some(candidate.to_string());
        }
    }
    None
}

/// One per pane. The GPUI view attaches the PTY + VT session lazily
/// once the pane has real bounds.
pub struct LinuxGhosttyTerminal {
    inner: Arc<Mutex<Option<LinuxPtySession>>>,
}

impl LinuxGhosttyTerminal {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    pub fn attach(&self, session: LinuxPtySession) {
        *self.inner.lock() = Some(session);
    }

    pub fn is_attached(&self) -> bool {
        self.inner.lock().is_some()
    }

    pub fn spawn_with_options(&self, options: LinuxPtyOptions) -> Result<(), String> {
        let session = LinuxPtySession::spawn(options).map_err(|err| err.to_string())?;
        self.attach(session);
        Ok(())
    }

    pub fn inner(&self) -> Arc<Mutex<Option<LinuxPtySession>>> {
        self.inner.clone()
    }

    pub fn draw(&self) {}

    pub fn refresh(&self) {}

    pub fn set_size(&self, width_px: u32, height_px: u32) {
        if let Some(session) = self.inner.lock().as_ref() {
            if let Err(err) = session.set_pixel_size(width_px, height_px) {
                log::debug!("linux pty pixel resize failed: {err:#}");
            }
        }
    }

    pub fn resize_surface(&self, size: SurfaceSize) {
        if let Some(session) = self.inner.lock().as_ref() {
            if let Err(err) = session.resize(size) {
                log::debug!("linux pty resize failed: {err:#}");
            }
        }
    }

    pub fn size(&self) -> SurfaceSize {
        self.inner
            .lock()
            .as_ref()
            .map(LinuxPtySession::size)
            .unwrap_or(SurfaceSize {
                columns: 0,
                rows: 0,
                width_px: 0,
                height_px: 0,
                cell_width_px: 0,
                cell_height_px: 0,
            })
    }

    pub fn set_content_scale(&self, _scale: f64) {}
    pub fn set_focus(&self, _focused: bool) {}
    pub fn set_occlusion(&self, _occluded: bool) {}
    pub fn set_color_scheme(&self, dark: bool) {
        if let Some(session) = self.inner.lock().as_ref() {
            session.set_dark_mode(dark);
        }
    }

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
        colors: &TerminalColors,
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
        if let Some(session) = self.inner.lock().as_ref() {
            session.set_theme(colors);
        }
        Ok(())
    }

    /// Returns the current libghostty-vt screen snapshot, if a PTY
    /// session has been spawned. Used by `con-app/src/linux_view.rs`
    /// to drive the styled-cell paint path.
    pub fn snapshot(&self) -> Option<ScreenSnapshot> {
        self.inner.lock().as_ref().map(LinuxPtySession::snapshot)
    }

    pub fn write_to_pty(&self, data: &[u8]) {
        if let Some(session) = self.inner.lock().as_ref() {
            if let Err(err) = session.write_input(data) {
                log::debug!("linux pty write failed: {err:#}");
            }
        }
    }

    pub fn send_text(&self, text: &str) {
        self.write_to_pty(text.as_bytes());
    }

    pub fn is_bracketed_paste(&self) -> bool {
        self.inner
            .lock()
            .as_ref()
            .is_some_and(LinuxPtySession::is_bracketed_paste)
    }

    pub fn is_decckm(&self) -> bool {
        self.inner
            .lock()
            .as_ref()
            .is_some_and(LinuxPtySession::is_decckm)
    }

    pub fn send_mouse_button(&self, _pressed: bool, _button: MouseButton, _mods: i32) -> bool {
        false
    }

    pub fn send_mouse_pos(&self, _x: f64, _y: f64, _mods: i32) {}
    pub fn send_mouse_scroll(&self, _x: f64, _y: f64, _mods: i32) {}

    pub fn request_close(&self) {
        *self.inner.lock() = None;
    }

    pub fn title(&self) -> Option<String> {
        self.inner.lock().as_ref().and_then(LinuxPtySession::title)
    }

    pub fn current_dir(&self) -> Option<String> {
        self.inner
            .lock()
            .as_ref()
            .and_then(LinuxPtySession::current_dir)
    }

    pub fn is_alive(&self) -> bool {
        self.inner
            .lock()
            .as_ref()
            .is_some_and(LinuxPtySession::is_alive)
    }

    pub fn is_busy(&self) -> bool {
        false
    }

    pub fn command_history(&self) -> Vec<CommandRecord> {
        Vec::new()
    }

    pub fn take_command_finished(&self) -> Option<CommandFinishedSignal> {
        self.inner
            .lock()
            .as_ref()
            .and_then(LinuxPtySession::take_command_finished)
    }

    pub fn last_exit_code(&self) -> Option<i32> {
        self.inner
            .lock()
            .as_ref()
            .and_then(LinuxPtySession::last_exit_code)
    }

    pub fn last_command_duration(&self) -> Option<Duration> {
        self.inner
            .lock()
            .as_ref()
            .and_then(LinuxPtySession::last_command_duration)
    }

    pub fn input_generation(&self) -> u64 {
        self.inner
            .lock()
            .as_ref()
            .map(LinuxPtySession::input_generation)
            .unwrap_or(0)
    }

    pub fn last_command_finished_input_generation(&self) -> u64 {
        self.input_generation()
    }

    pub fn recover_shell_prompt_state(&self) {}

    pub fn has_selection(&self) -> bool {
        false
    }

    pub fn selection_text(&self) -> Option<String> {
        None
    }

    pub fn read_screen_text(&self, max_lines: usize) -> Vec<String> {
        self.inner
            .lock()
            .as_ref()
            .map(|session| session.read_screen_text(max_lines))
            .unwrap_or_default()
    }

    pub fn read_recent_lines(&self, max_lines: usize) -> Vec<String> {
        self.inner
            .lock()
            .as_ref()
            .map(|session| session.read_recent_lines(max_lines))
            .unwrap_or_default()
    }

    pub fn search_text(&self, pattern: &str, limit: usize) -> Vec<(usize, String)> {
        self.inner
            .lock()
            .as_ref()
            .map(|session| session.search_text(pattern, limit))
            .unwrap_or_default()
    }

    pub fn take_needs_render(&self) -> bool {
        self.inner
            .lock()
            .as_ref()
            .is_some_and(LinuxPtySession::take_needs_render)
    }

    pub fn take_pending_events(&self) -> Vec<GhosttySurfaceEvent> {
        Vec::new()
    }
}

impl Default for LinuxGhosttyTerminal {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl Send for LinuxGhosttyTerminal {}
unsafe impl Sync for LinuxGhosttyTerminal {}
