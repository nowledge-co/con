use std::ffi::{CStr, CString};
use std::os::raw::c_void;

use crate::ffi;

/// A ghostty terminal surface for headless rendering.
///
/// Wraps the ghostty C API surface and provides safe Rust access
/// to terminal state, rendering, and input.
pub struct GhosttyTerminal {
    surface: ffi::ghostty_surface_t,
    _userdata: Box<Userdata>,
}

struct Userdata {
    wakeup_callback: Option<Box<dyn Fn() + Send + Sync>>,
}

impl GhosttyTerminal {
    /// Get the IOSurfaceRef from the last-presented render target.
    /// Returns None if no frame has been presented yet.
    ///
    /// The returned pointer is an IOSurfaceRef (macOS) — caller must NOT release it.
    /// It remains valid until the next draw() call.
    #[cfg(target_os = "macos")]
    pub fn iosurface(&self) -> Option<*mut c_void> {
        let ptr = unsafe { ffi::ghostty_surface_iosurface(self.surface) };
        if ptr.is_null() {
            None
        } else {
            Some(ptr)
        }
    }

    /// Trigger a render. The result is available via iosurface().
    pub fn draw(&self) {
        unsafe { ffi::ghostty_surface_draw(self.surface) }
    }

    /// Set the size in pixels.
    pub fn set_size(&self, width_px: u32, height_px: u32) {
        unsafe { ffi::ghostty_surface_set_size(self.surface, width_px, height_px) }
    }

    /// Get the current size (columns, rows, pixel dimensions, cell dimensions).
    pub fn size(&self) -> SurfaceSize {
        let s = unsafe { ffi::ghostty_surface_size(self.surface) };
        SurfaceSize {
            columns: s.columns,
            rows: s.rows,
            width_px: s.width_px,
            height_px: s.height_px,
            cell_width_px: s.cell_width_px,
            cell_height_px: s.cell_height_px,
        }
    }

    /// Send UTF-8 text input to the terminal.
    pub fn send_text(&self, text: &str) {
        if let Ok(cstr) = CString::new(text) {
            unsafe { ffi::ghostty_surface_text(self.surface, cstr.as_ptr()) }
        }
    }

    /// Set focus state.
    pub fn set_focus(&self, focused: bool) {
        unsafe { ffi::ghostty_surface_set_focus(self.surface, focused) }
    }

    /// Set color scheme (light/dark).
    pub fn set_color_scheme(&self, dark: bool) {
        let scheme = if dark {
            ffi::ghostty_color_scheme_e::GHOSTTY_COLOR_SCHEME_DARK
        } else {
            ffi::ghostty_color_scheme_e::GHOSTTY_COLOR_SCHEME_LIGHT
        };
        unsafe { ffi::ghostty_surface_set_color_scheme(self.surface, scheme) }
    }

    /// Set content scale (e.g., 2.0 for Retina).
    pub fn set_content_scale(&self, scale: f64) {
        unsafe { ffi::ghostty_surface_set_content_scale(self.surface, scale, scale) }
    }

    // ── Agent State APIs ──────────────────────────────────────

    /// Get the terminal title (from OSC 0/1/2).
    pub fn title(&self) -> Option<String> {
        let ptr = unsafe { ffi::ghostty_surface_get_title(self.surface) };
        if ptr.is_null() {
            return None;
        }
        unsafe { CStr::from_ptr(ptr) }
            .to_str()
            .ok()
            .map(String::from)
    }

    /// Get the working directory (from OSC 7).
    pub fn current_dir(&self) -> Option<String> {
        let ptr = unsafe { ffi::ghostty_surface_get_pwd(self.surface) };
        let len = unsafe { ffi::ghostty_surface_get_pwd_len(self.surface) };
        if ptr.is_null() || len == 0 {
            return None;
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
        std::str::from_utf8(bytes).ok().map(String::from)
    }

    /// Returns true if the terminal is in alternate screen mode (TUI apps like vim, btop).
    pub fn is_alt_screen(&self) -> bool {
        unsafe { ffi::ghostty_surface_is_alt_screen(self.surface) }
    }

    /// Get cursor position (column, row) in the active screen.
    pub fn cursor_pos(&self) -> (u16, u16) {
        let mut col: u16 = 0;
        let mut row: u16 = 0;
        unsafe { ffi::ghostty_surface_cursor_pos(self.surface, &mut col, &mut row) }
        (col, row)
    }

    /// Dump the visible screen text.
    pub fn screen_text(&self) -> String {
        // First call with null buf to get size
        let needed = unsafe { ffi::ghostty_surface_screen_text(self.surface, std::ptr::null_mut(), 0) };
        if needed == 0 {
            return String::new();
        }
        let mut buf = vec![0u8; needed as usize];
        let written = unsafe {
            ffi::ghostty_surface_screen_text(
                self.surface,
                buf.as_mut_ptr() as *mut _,
                needed,
            )
        };
        buf.truncate(written as usize);
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// Returns true if the child process has exited.
    pub fn is_alive(&self) -> bool {
        !unsafe { ffi::ghostty_surface_process_exited(self.surface) }
    }

    /// Mouse button event.
    pub fn send_mouse_button(
        &self,
        pressed: bool,
        button: MouseButton,
        mods: i32,
    ) {
        let action = if pressed {
            ffi::ghostty_input_action_e::GHOSTTY_ACTION_PRESS
        } else {
            ffi::ghostty_input_action_e::GHOSTTY_ACTION_RELEASE
        };
        let btn = match button {
            MouseButton::Left => ffi::ghostty_mouse_button_e::GHOSTTY_MOUSE_LEFT,
            MouseButton::Right => ffi::ghostty_mouse_button_e::GHOSTTY_MOUSE_RIGHT,
            MouseButton::Middle => ffi::ghostty_mouse_button_e::GHOSTTY_MOUSE_MIDDLE,
        };
        unsafe { ffi::ghostty_surface_mouse_button(self.surface, action, btn, mods) }
    }

    /// Mouse position event.
    pub fn send_mouse_pos(&self, x: f64, y: f64) {
        unsafe { ffi::ghostty_surface_mouse_pos(self.surface, x, y) }
    }

    /// Mouse scroll event.
    pub fn send_mouse_scroll(&self, x: f64, y: f64, mods: i32) {
        unsafe { ffi::ghostty_surface_mouse_scroll(self.surface, x, y, mods) }
    }

    /// Get the raw FFI surface handle (for advanced use).
    pub fn raw_surface(&self) -> ffi::ghostty_surface_t {
        self.surface
    }
}

impl Drop for GhosttyTerminal {
    fn drop(&mut self) {
        unsafe { ffi::ghostty_surface_free(self.surface) }
    }
}

// SAFETY: The ghostty surface is thread-safe — all state access is mutex-protected.
unsafe impl Send for GhosttyTerminal {}
unsafe impl Sync for GhosttyTerminal {}

#[derive(Debug, Clone, Copy)]
pub struct SurfaceSize {
    pub columns: u16,
    pub rows: u16,
    pub width_px: u32,
    pub height_px: u32,
    pub cell_width_px: u32,
    pub cell_height_px: u32,
}

#[derive(Debug, Clone, Copy)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}
