//! Safe wrapper around ghostty's embedded C API.
//!
//! Uses the macOS platform: creates an NSView for ghostty to render into
//! via its GPU-accelerated Metal renderer. State (title, pwd) is received
//! through action callbacks, not polling.
//!
//! Design invariants:
//! - `ghostty_init` is called exactly once per process (via `std::sync::Once`)
//! - Each `GhosttyTerminal` has its own `TerminalState` via per-surface userdata
//! - Clipboard callbacks are always set (ghostty dereferences them without null checks)
//! - The GhosttyApp must be ticked from the main thread (Metal rendering)

use std::ffi::{CStr, CString};
use std::os::raw::c_void;
use std::sync::{Arc, Once};

use parking_lot::Mutex;

use crate::ffi;

// ── Per-surface state updated by action callbacks ───────────

/// Terminal state received via ghostty action callbacks.
/// Each GhosttyTerminal has its own instance, stored as surface userdata.
#[derive(Default)]
pub struct TerminalState {
    pub title: Option<String>,
    pub pwd: Option<String>,
    pub needs_render: bool,
    pub child_exited: bool,
}

pub type StateRef = Arc<Mutex<TerminalState>>;

// ── One-time global init ────────────────────────────────────

static GHOSTTY_INIT: Once = Once::new();
static mut GHOSTTY_INIT_RESULT: i32 = -1;

fn ensure_ghostty_init() -> Result<(), String> {
    GHOSTTY_INIT.call_once(|| {
        let ret = unsafe { ffi::ghostty_init(0, std::ptr::null_mut()) };
        unsafe { GHOSTTY_INIT_RESULT = ret };
    });
    let ret = unsafe { GHOSTTY_INIT_RESULT };
    if ret != 0 {
        Err(format!("ghostty_init failed with code {}", ret))
    } else {
        Ok(())
    }
}

// ── GhosttyApp — singleton managing all surfaces ────────────

/// Global ghostty application context. One per process.
///
/// Must be ticked from the main thread — ghostty's Metal renderer
/// requires main thread access on macOS.
pub struct GhosttyApp {
    app: ffi::ghostty_app_t,
    // Box prevents the runtime_config from being moved while ghostty holds a pointer.
    _runtime_config: Box<ffi::ghostty_runtime_config_s>,
}

impl GhosttyApp {
    /// Create a new ghostty app with default config.
    pub fn new() -> Result<Self, String> {
        ensure_ghostty_init()?;

        // Create and finalize config
        let config = unsafe { ffi::ghostty_config_new() };
        if config.is_null() {
            return Err("ghostty_config_new returned null".into());
        }
        unsafe {
            ffi::ghostty_config_load_default_files(config);
            ffi::ghostty_config_finalize(config);
        }

        // App-level userdata is not used for per-surface state.
        // We keep it null — per-surface state is on surface userdata.
        let runtime_config = Box::new(ffi::ghostty_runtime_config_s {
            userdata: std::ptr::null_mut(),
            supports_selection_clipboard: false,
            wakeup_cb: Some(wakeup_callback),
            action_cb: Some(action_callback),
            // These MUST be Some — ghostty dereferences them without null checks.
            read_clipboard_cb: Some(read_clipboard_callback),
            confirm_read_clipboard_cb: Some(confirm_read_clipboard_callback),
            write_clipboard_cb: Some(write_clipboard_callback),
            close_surface_cb: Some(close_surface_callback),
        });

        let app =
            unsafe { ffi::ghostty_app_new(&*runtime_config as *const _, config) };

        // Ghostty clones the config — we must free the original.
        unsafe { ffi::ghostty_config_free(config) };

        if app.is_null() {
            return Err("ghostty_app_new returned null".into());
        }

        Ok(Self {
            app,
            _runtime_config: runtime_config,
        })
    }

    /// Drive the ghostty event loop.
    ///
    /// **Must be called from the main thread** — ghostty's Metal renderer
    /// and AppKit operations require it.
    pub fn tick(&self) {
        unsafe { ffi::ghostty_app_tick(self.app) }
    }

    /// Set the global color scheme.
    pub fn set_color_scheme(&self, dark: bool) {
        let scheme = if dark {
            ffi::ghostty_color_scheme_e::GHOSTTY_COLOR_SCHEME_DARK
        } else {
            ffi::ghostty_color_scheme_e::GHOSTTY_COLOR_SCHEME_LIGHT
        };
        unsafe { ffi::ghostty_app_set_color_scheme(self.app, scheme) }
    }

    /// Create a new terminal surface. The `nsview` must be a valid NSView pointer
    /// that ghostty will attach its Metal IOSurfaceLayer to.
    ///
    /// Each surface gets its own `TerminalState` for independent title/pwd tracking.
    #[cfg(target_os = "macos")]
    pub fn new_surface(
        &self,
        nsview: *mut c_void,
        scale_factor: f64,
        cwd: Option<&str>,
    ) -> Result<GhosttyTerminal, String> {
        // Per-surface state — stored as surface userdata so callbacks can
        // update the correct terminal's state.
        let state: StateRef = Arc::new(Mutex::new(TerminalState::default()));
        let surface_userdata = Box::into_raw(Box::new(state.clone())) as *mut c_void;

        let mut config = unsafe { ffi::ghostty_surface_config_new() };
        config.platform_tag =
            ffi::ghostty_platform_e::GHOSTTY_PLATFORM_MACOS as std::os::raw::c_int;
        config.platform = ffi::ghostty_platform_u {
            macos: ffi::ghostty_platform_macos_s { nsview },
        };
        config.userdata = surface_userdata;
        config.scale_factor = scale_factor;
        config.context = ffi::ghostty_surface_context_e::GHOSTTY_SURFACE_CONTEXT_TAB;

        let cwd_cstr = cwd.and_then(|s| CString::new(s).ok());
        if let Some(ref s) = cwd_cstr {
            config.working_directory = s.as_ptr();
        }

        let surface =
            unsafe { ffi::ghostty_surface_new(self.app, &config as *const _) };
        if surface.is_null() {
            // Clean up the userdata we allocated
            unsafe { drop(Box::from_raw(surface_userdata as *mut StateRef)) };
            return Err("ghostty_surface_new returned null".into());
        }

        Ok(GhosttyTerminal {
            surface,
            state,
            userdata_ptr: surface_userdata,
        })
    }

    /// Raw app handle.
    pub fn raw(&self) -> ffi::ghostty_app_t {
        self.app
    }
}

impl Drop for GhosttyApp {
    fn drop(&mut self) {
        // Free the app first — this allows ghostty to run any final cleanup
        // and fire callbacks while userdata is still valid.
        unsafe { ffi::ghostty_app_free(self.app) }
    }
}

// SAFETY: GhosttyApp's C-side state is protected by ghostty's internal
// synchronization. The Rust-side runtime_config is heap-pinned and read-only.
unsafe impl Send for GhosttyApp {}
unsafe impl Sync for GhosttyApp {}

// ── GhosttyTerminal — a single terminal surface ─────────────

/// A single ghostty terminal surface backed by GPU-accelerated Metal rendering.
///
/// Ghostty renders directly into the NSView provided at creation time.
/// Input is forwarded via the ghostty_surface_* APIs. State updates
/// (title, pwd) arrive via the per-surface action callback.
pub struct GhosttyTerminal {
    surface: ffi::ghostty_surface_t,
    state: StateRef,
    /// Raw pointer to the Box<StateRef> we allocated for surface userdata.
    /// Must be recovered and freed when the terminal is dropped.
    userdata_ptr: *mut c_void,
}

impl GhosttyTerminal {
    /// Trigger a draw (ghostty renders into its Metal layer).
    pub fn draw(&self) {
        unsafe { ffi::ghostty_surface_draw(self.surface) }
    }

    /// Request a refresh (marks the surface as needing redraw).
    pub fn refresh(&self) {
        unsafe { ffi::ghostty_surface_refresh(self.surface) }
    }

    /// Set the surface size in pixels.
    pub fn set_size(&self, width_px: u32, height_px: u32) {
        unsafe { ffi::ghostty_surface_set_size(self.surface, width_px, height_px) }
    }

    /// Get the current size (columns, rows, pixel dimensions, cell size).
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

    /// Set content scale (e.g., 2.0 for Retina).
    pub fn set_content_scale(&self, scale: f64) {
        unsafe { ffi::ghostty_surface_set_content_scale(self.surface, scale, scale) }
    }

    /// Set focus state.
    pub fn set_focus(&self, focused: bool) {
        unsafe { ffi::ghostty_surface_set_focus(self.surface, focused) }
    }

    /// Set occlusion state (hidden behind other windows).
    pub fn set_occlusion(&self, occluded: bool) {
        unsafe { ffi::ghostty_surface_set_occlusion(self.surface, occluded) }
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

    /// Send a key event to the terminal. Returns true if ghostty consumed it.
    pub fn send_key(&self, key: ffi::ghostty_input_key_s) -> bool {
        unsafe { ffi::ghostty_surface_key(self.surface, key) }
    }

    /// Send UTF-8 text input to the terminal (for composed/IME text).
    pub fn send_text(&self, text: &str) {
        if let Ok(cstr) = CString::new(text) {
            let len = cstr.as_bytes().len(); // excludes NUL, matches original text
            unsafe { ffi::ghostty_surface_text(self.surface, cstr.as_ptr(), len) }
        }
        // If text contains NUL bytes, we silently drop it — this matches
        // terminal semantics where NUL in text input is meaningless.
    }

    /// Send a mouse button event.
    pub fn send_mouse_button(
        &self,
        pressed: bool,
        button: MouseButton,
        mods: i32,
    ) -> bool {
        let state = if pressed {
            ffi::ghostty_input_mouse_state_e::GHOSTTY_MOUSE_PRESS
        } else {
            ffi::ghostty_input_mouse_state_e::GHOSTTY_MOUSE_RELEASE
        };
        let btn = match button {
            MouseButton::Left => ffi::ghostty_input_mouse_button_e::GHOSTTY_MOUSE_LEFT,
            MouseButton::Right => {
                ffi::ghostty_input_mouse_button_e::GHOSTTY_MOUSE_RIGHT
            }
            MouseButton::Middle => {
                ffi::ghostty_input_mouse_button_e::GHOSTTY_MOUSE_MIDDLE
            }
        };
        unsafe { ffi::ghostty_surface_mouse_button(self.surface, state, btn, mods) }
    }

    /// Send mouse position event.
    pub fn send_mouse_pos(&self, x: f64, y: f64, mods: i32) {
        unsafe { ffi::ghostty_surface_mouse_pos(self.surface, x, y, mods) }
    }

    /// Send mouse scroll event.
    pub fn send_mouse_scroll(&self, x: f64, y: f64, mods: i32) {
        unsafe { ffi::ghostty_surface_mouse_scroll(self.surface, x, y, mods) }
    }

    /// Request close (sends signal to child process).
    pub fn request_close(&self) {
        unsafe { ffi::ghostty_surface_request_close(self.surface) }
    }

    // ── State queries ───────────────────────────────────────

    /// Terminal title (from per-surface action callback, set by OSC 0/1/2).
    pub fn title(&self) -> Option<String> {
        self.state.lock().title.clone()
    }

    /// Working directory (from per-surface action callback, set by OSC 7).
    pub fn current_dir(&self) -> Option<String> {
        self.state.lock().pwd.clone()
    }

    /// Whether the child process has exited.
    pub fn is_alive(&self) -> bool {
        !unsafe { ffi::ghostty_surface_process_exited(self.surface) }
    }

    /// Whether the terminal has a text selection.
    pub fn has_selection(&self) -> bool {
        unsafe { ffi::ghostty_surface_has_selection(self.surface) }
    }

    /// Read the current selection text. Returns None if no selection.
    pub fn selection_text(&self) -> Option<String> {
        let mut text = ffi::ghostty_text_s {
            tl_px_x: 0.0,
            tl_px_y: 0.0,
            offset_start: 0,
            offset_len: 0,
            text: std::ptr::null(),
            text_len: 0,
        };
        let ok =
            unsafe { ffi::ghostty_surface_read_selection(self.surface, &mut text) };
        if !ok || text.text.is_null() || text.text_len == 0 {
            return None;
        }
        let result = unsafe {
            let bytes =
                std::slice::from_raw_parts(text.text as *const u8, text.text_len);
            String::from_utf8_lossy(bytes).into_owned()
        };
        unsafe { ffi::ghostty_surface_free_text(self.surface, &mut text) };
        Some(result)
    }

    /// Read text from a specific screen region.
    pub fn read_text(&self, selection: ffi::ghostty_selection_s) -> Option<String> {
        let mut text = ffi::ghostty_text_s {
            tl_px_x: 0.0,
            tl_px_y: 0.0,
            offset_start: 0,
            offset_len: 0,
            text: std::ptr::null(),
            text_len: 0,
        };
        let ok = unsafe {
            ffi::ghostty_surface_read_text(self.surface, selection, &mut text)
        };
        if !ok || text.text.is_null() || text.text_len == 0 {
            return None;
        }
        let result = unsafe {
            let bytes =
                std::slice::from_raw_parts(text.text as *const u8, text.text_len);
            String::from_utf8_lossy(bytes).into_owned()
        };
        unsafe { ffi::ghostty_surface_free_text(self.surface, &mut text) };
        Some(result)
    }

    /// Check and clear the needs_render flag.
    pub fn take_needs_render(&self) -> bool {
        let mut state = self.state.lock();
        let r = state.needs_render;
        state.needs_render = false;
        r
    }

    /// Access the per-surface state.
    pub fn state(&self) -> &StateRef {
        &self.state
    }

    /// Raw FFI surface handle.
    pub fn raw_surface(&self) -> ffi::ghostty_surface_t {
        self.surface
    }
}

impl Drop for GhosttyTerminal {
    fn drop(&mut self) {
        // Free the surface first — ghostty may fire callbacks during cleanup
        // while userdata is still valid.
        unsafe { ffi::ghostty_surface_free(self.surface) };
        // Recover the Box<StateRef> we allocated in new_surface.
        // After surface_free, ghostty no longer references this pointer.
        if !self.userdata_ptr.is_null() {
            unsafe { drop(Box::from_raw(self.userdata_ptr as *mut StateRef)) };
        }
    }
}

// SAFETY: The ghostty surface is thread-safe — all state access is mutex-protected.
unsafe impl Send for GhosttyTerminal {}
unsafe impl Sync for GhosttyTerminal {}

// ── Public types ────────────────────────────────────────────

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

// ── C callback implementations ──────────────────────────────

/// Resolve per-surface state from a ghostty_target_s.
/// For SURFACE-targeted actions, reads the surface's userdata.
/// Returns None if the target is app-level or has no userdata.
unsafe fn resolve_surface_state(
    target: &ffi::ghostty_target_s,
) -> Option<StateRef> {
    unsafe {
        if target.tag != ffi::ghostty_target_tag_e::GHOSTTY_TARGET_SURFACE {
            return None;
        }
        let surface = target.target.surface;
        if surface.is_null() {
            return None;
        }
        let userdata = ffi::ghostty_surface_userdata(surface);
        if userdata.is_null() {
            return None;
        }
        let state_ref = &*(userdata as *const StateRef);
        Some(state_ref.clone())
    }
}

unsafe extern "C" fn wakeup_callback(_userdata: *mut c_void) {
    // The wakeup callback signals that ghostty has work to do.
    // The GPUI integration layer (ghostty_view.rs) uses on_next_frame
    // to call tick() on the main thread. This callback is a signal
    // that we should schedule that tick sooner rather than later.
    //
    // TODO: integrate with GPUI's event loop for lower-latency wakeup.
    // For now, the 8ms timer in ghostty_view.rs provides adequate refresh.
}

unsafe extern "C" fn action_callback(
    _app: ffi::ghostty_app_t,
    target: ffi::ghostty_target_s,
    action: ffi::ghostty_action_s,
) -> bool {
    unsafe {
        let state = match resolve_surface_state(&target) {
            Some(s) => s,
            None => return false,
        };

        match action.tag {
            ffi::ghostty_action_tag_e::GHOSTTY_ACTION_SET_TITLE => {
                let title_ptr = action.action.set_title.title;
                if !title_ptr.is_null() {
                    let title =
                        CStr::from_ptr(title_ptr).to_string_lossy().into_owned();
                    state.lock().title = Some(title);
                }
                true
            }
            ffi::ghostty_action_tag_e::GHOSTTY_ACTION_PWD => {
                let pwd_ptr = action.action.pwd.pwd;
                if !pwd_ptr.is_null() {
                    let pwd =
                        CStr::from_ptr(pwd_ptr).to_string_lossy().into_owned();
                    state.lock().pwd = Some(pwd);
                }
                true
            }
            ffi::ghostty_action_tag_e::GHOSTTY_ACTION_RENDER => {
                state.lock().needs_render = true;
                true
            }
            _ => false,
        }
    }
}

/// Clipboard read stub — returns false (clipboard not available).
/// Ghostty calls this without null-checking, so it must always be set.
unsafe extern "C" fn read_clipboard_callback(
    _userdata: *mut c_void,
    _clipboard: ffi::ghostty_clipboard_e,
    _request: *mut c_void,
) -> bool {
    // TODO: integrate with GPUI/macOS clipboard for paste support
    false
}

/// Clipboard confirmation stub — does nothing.
/// Ghostty calls this without null-checking, so it must always be set.
unsafe extern "C" fn confirm_read_clipboard_callback(
    _userdata: *mut c_void,
    _text: *const std::os::raw::c_char,
    _request: *mut c_void,
    _request_type: ffi::ghostty_clipboard_request_e,
) {
    // TODO: implement clipboard confirmation UI
}

/// Clipboard write stub — does nothing.
/// Ghostty calls this without null-checking, so it must always be set.
unsafe extern "C" fn write_clipboard_callback(
    _userdata: *mut c_void,
    _clipboard: ffi::ghostty_clipboard_e,
    _content: *const ffi::ghostty_clipboard_content_s,
    _content_count: usize,
    _confirm: bool,
) {
    // TODO: integrate with GPUI/macOS clipboard for copy support
}

unsafe extern "C" fn close_surface_callback(
    userdata: *mut c_void,
    _process_alive: bool,
) {
    // userdata here is the surface's userdata (per-surface StateRef)
    if userdata.is_null() {
        return;
    }
    let state = unsafe { &*(userdata as *const StateRef) };
    state.lock().child_exited = true;
}
