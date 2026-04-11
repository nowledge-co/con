//! GhosttyView — GPUI element wrapping ghostty's GPU-accelerated Metal terminal.
//!
//! Architecture:
//! - Creates an NSView child within GPUI's window view
//! - Passes the NSView to ghostty (macOS embedded platform)
//! - Ghostty renders via Metal into that view (zero-copy, GPU-accelerated)
//! - GPUI input events forwarded via `ghostty_surface_key` / `ghostty_surface_text`
//! - Terminal state (title, pwd) arrives via ghostty's per-surface action callbacks
//!
//! Key input goes through ghostty's key processing pipeline (not raw escape
//! sequences), so application cursor mode, kitty keyboard protocol, and
//! ghostty key bindings all work correctly.

use std::cell::Cell;
#[cfg(target_os = "macos")]
use std::os::raw::c_void;
use std::sync::Arc;
use std::time::{Duration, Instant};

use con_ghostty::ffi;
use con_ghostty::{
    GhosttyApp, GhosttySplitDirection, GhosttySurfaceEvent, GhosttyTerminal, MouseButton,
};
use gpui::*;

// Actions to intercept Tab/Shift-Tab before Root's focus-cycling handler.
actions!(ghostty, [ConsumeTab, ConsumeTabPrev]);

#[cfg(target_os = "macos")]
use cocoa::base::{NO, YES, id};
#[cfg(target_os = "macos")]
use cocoa::foundation::NSRect;
#[cfg(target_os = "macos")]
use objc::{class, msg_send, sel, sel_impl};
#[cfg(target_os = "macos")]
use raw_window_handle::HasWindowHandle;

/// Emitted when the terminal title changes.
#[allow(dead_code)]
pub struct GhosttyTitleChanged(pub Option<String>);

/// Emitted when the terminal process exits.
pub struct GhosttyProcessExited;

/// Emitted when the terminal gains focus.
pub struct GhosttyFocusChanged;
/// Emitted when Ghostty requests a new split from this surface.
pub struct GhosttySplitRequested(pub GhosttySplitDirection);

impl EventEmitter<GhosttyTitleChanged> for GhosttyView {}
impl EventEmitter<GhosttyProcessExited> for GhosttyView {}
impl EventEmitter<GhosttyFocusChanged> for GhosttyView {}
impl EventEmitter<GhosttySplitRequested> for GhosttyView {}

/// GPUI view wrapping a ghostty terminal surface.
pub struct GhosttyView {
    app: Arc<GhosttyApp>,
    terminal: Option<Arc<GhosttyTerminal>>,
    focus_handle: FocusHandle,
    initial_cwd: Option<String>,
    initial_font_size: f32,
    #[cfg(target_os = "macos")]
    nsview: Option<id>,
    initialized: bool,
    last_bounds: Option<Bounds<Pixels>>,
    scale_factor: f32,
    last_title: Option<String>,
    /// Data queued for the PTY before the surface was created.
    /// Flushed once in `ensure_initialized()` after the terminal exists.
    pending_write: Option<Vec<u8>>,
    /// Desired native view visibility, including before the NSView exists.
    native_view_visible: Cell<bool>,
    /// When a surface was created before a real layout pass, keep it hidden until
    /// update_frame commits an actual pane geometry.
    awaiting_first_layout_visibility: bool,
    /// Guard: emit GhosttyProcessExited exactly once, not every 8ms tick.
    process_exit_emitted: bool,
    /// Retry surface creation after transient libghostty initialization failures.
    next_surface_init_retry_at: Option<Instant>,
}

/// Register ghostty key bindings. Call once at startup.
pub fn init(cx: &mut App) {
    // Bind Tab/Shift-Tab to our consume actions within the GhosttyTerminal context.
    // This prevents Root's Tab handler from intercepting Tab when the terminal is focused.
    cx.bind_keys([
        KeyBinding::new("tab", ConsumeTab, Some("GhosttyTerminal")),
        KeyBinding::new("shift-tab", ConsumeTabPrev, Some("GhosttyTerminal")),
    ]);
}

impl GhosttyView {
    pub fn new(
        app: Arc<GhosttyApp>,
        cwd: Option<String>,
        font_size: f32,
        cx: &mut Context<Self>,
    ) -> Self {
        // Tick ghostty periodically. The update() closure runs on the main
        // thread, which is required for ghostty_app_tick (Metal rendering).
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(8))
                    .await;
                if this
                    .update(cx, |view, cx| {
                        view.app.tick();

                        if let Some(ref terminal) = view.terminal {
                            for event in terminal.take_pending_events() {
                                match event {
                                    GhosttySurfaceEvent::SplitRequest(direction) => {
                                        cx.emit(GhosttySplitRequested(direction));
                                    }
                                }
                            }
                            if terminal.take_needs_render() {
                                cx.notify();
                            }
                            if !terminal.is_alive() && !view.process_exit_emitted {
                                view.process_exit_emitted = true;
                                cx.emit(GhosttyProcessExited);
                            }
                            let title = terminal.title();
                            if title != view.last_title {
                                view.last_title = title.clone();
                                cx.emit(GhosttyTitleChanged(title));
                            }
                        } else if !view.initialized
                            && view
                                .next_surface_init_retry_at
                                .is_some_and(|deadline| Instant::now() >= deadline)
                        {
                            view.next_surface_init_retry_at = None;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        Self {
            app,
            terminal: None,
            focus_handle: cx.focus_handle(),
            initial_cwd: cwd,
            initial_font_size: font_size,
            #[cfg(target_os = "macos")]
            nsview: None,
            initialized: false,
            last_bounds: None,
            scale_factor: 1.0,
            last_title: None,
            pending_write: None,
            native_view_visible: Cell::new(true),
            awaiting_first_layout_visibility: false,
            process_exit_emitted: false,
            next_surface_init_retry_at: None,
        }
    }

    pub fn terminal(&self) -> Option<&Arc<GhosttyTerminal>> {
        self.terminal.as_ref()
    }

    /// Queue data to write to the PTY. If the terminal is already initialized,
    /// writes immediately. Otherwise, buffers until `ensure_initialized()` runs.
    pub fn write_or_queue(&mut self, data: &[u8]) {
        if let Some(ref terminal) = self.terminal {
            terminal.write_to_pty(data);
        } else {
            self.pending_write
                .get_or_insert_with(Vec::new)
                .extend_from_slice(data);
        }
    }

    pub fn title(&self) -> Option<String> {
        self.terminal.as_ref().and_then(|t| t.title())
    }

    pub fn current_dir(&self) -> Option<String> {
        self.terminal.as_ref().and_then(|t| t.current_dir())
    }

    pub fn is_alive(&self) -> bool {
        self.terminal.as_ref().is_some_and(|t| t.is_alive())
    }

    pub fn surface_ready(&self) -> bool {
        self.terminal.is_some()
    }

    pub fn has_layout(&self) -> bool {
        self.last_bounds.is_some()
    }

    #[allow(dead_code)]
    pub fn selection_text(&self) -> Option<String> {
        self.terminal.as_ref().and_then(|t| t.selection_text())
    }

    #[cfg(target_os = "macos")]
    fn ensure_initialized(&mut self, bounds: Bounds<Pixels>, window: &mut Window) {
        if self.initialized {
            return;
        }
        if self
            .next_surface_init_retry_at
            .is_some_and(|deadline| Instant::now() < deadline)
        {
            return;
        }

        self.scale_factor = window.scale_factor();

        let raw_handle: raw_window_handle::WindowHandle<'_> =
            match HasWindowHandle::window_handle(window) {
                Ok(handle) => handle,
                Err(_) => return,
            };

        let parent_nsview = match raw_handle.as_raw() {
            raw_window_handle::RawWindowHandle::AppKit(handle) => handle.ns_view.as_ptr() as id,
            _ => return,
        };

        // Create with a zero-origin frame — update_frame() will set the
        // correct flipped position immediately after initialization.
        let nsview: id = unsafe {
            let frame = NSRect::new(
                cocoa::foundation::NSPoint::new(0.0, 0.0),
                cocoa::foundation::NSSize::new(
                    f64::from(bounds.size.width),
                    f64::from(bounds.size.height),
                ),
            );
            let view: id = msg_send![class!(NSView), alloc];
            let view: id = msg_send![view, initWithFrame:frame];
            let _: () = msg_send![view, setWantsLayer:YES];
            let _: () = msg_send![view, setAutoresizesSubviews:NO];
            let _: () = msg_send![parent_nsview, addSubview:view];
            view
        };

        let scale = self.scale_factor as f64;
        match self.app.new_surface(
            nsview as *mut c_void,
            scale,
            self.initial_cwd.as_deref(),
            Some(self.initial_font_size),
        ) {
            Ok(terminal) => {
                let width_px = (f32::from(bounds.size.width) * self.scale_factor) as u32;
                let height_px = (f32::from(bounds.size.height) * self.scale_factor) as u32;
                terminal.set_size(width_px, height_px);
                terminal.set_content_scale(scale);
                terminal.set_focus(true);
                self.terminal = Some(Arc::new(terminal));
                self.nsview = Some(nsview);
                self.initialized = true;
                self.next_surface_init_retry_at = None;
                self.set_visible(
                    self.native_view_visible.get() && !self.awaiting_first_layout_visibility,
                );
                // Don't set last_bounds here — let update_frame() handle
                // the coordinate flip and position the NSView correctly.
                log::info!(
                    "Ghostty surface created: {}x{} px, scale {}",
                    width_px,
                    height_px,
                    scale
                );

                // Flush any data queued before the surface existed.
                if let Some(data) = self.pending_write.take() {
                    self.terminal.as_ref().unwrap().write_to_pty(&data);
                }
            }
            Err(e) => {
                log::error!("Failed to create ghostty surface: {}", e);
                // Discard queued writes — no PTY exists.
                self.pending_write = None;
                self.initialized = false;
                self.next_surface_init_retry_at = Some(Instant::now() + Duration::from_millis(250));
                unsafe {
                    let _: () = msg_send![nsview, removeFromSuperview];
                }
                self.nsview = None;
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub fn ensure_initialized_for_control(&mut self, window: &mut Window) {
        let fallback_bounds = self.last_bounds.unwrap_or_else(|| {
            let window_bounds = window.bounds();
            let width = (window_bounds.size.width.as_f32() * 0.45).clamp(360.0, 900.0);
            let height = (window_bounds.size.height.as_f32() * 0.7).clamp(240.0, 900.0);
            Bounds::new(point(px(0.0), px(0.0)), size(px(width), px(height)))
        });
        let needs_real_layout = self.last_bounds.is_none();
        self.awaiting_first_layout_visibility = needs_real_layout;
        self.ensure_initialized(fallback_bounds, window);
        // Control-created panes may need a live PTY before GPUI has laid them out,
        // but publishing a fallback frame makes the native view visibly jump across
        // the workspace. Keep the surface hidden until the first real on_layout.
        if !needs_real_layout {
            self.update_frame(fallback_bounds);
        }
    }

    #[cfg(target_os = "macos")]
    fn update_frame(&mut self, bounds: Bounds<Pixels>) {
        let bounds_changed = self.last_bounds.as_ref() != Some(&bounds);
        if bounds_changed {
            self.last_bounds = Some(bounds);
            if let Some(ref terminal) = self.terminal {
                let width_px = (f32::from(bounds.size.width) * self.scale_factor) as u32;
                let height_px = (f32::from(bounds.size.height) * self.scale_factor) as u32;
                terminal.set_size(width_px, height_px);
            }
        }

        if let Some(nsview) = self.nsview {
            unsafe {
                let superview: id = msg_send![nsview, superview];
                let super_frame: NSRect = msg_send![superview, frame];
                let flipped_y = super_frame.size.height
                    - f64::from(bounds.origin.y)
                    - f64::from(bounds.size.height);

                let frame = NSRect::new(
                    cocoa::foundation::NSPoint::new(f64::from(bounds.origin.x), flipped_y),
                    cocoa::foundation::NSSize::new(
                        f64::from(bounds.size.width),
                        f64::from(bounds.size.height),
                    ),
                );
                let _: () = msg_send![nsview, setFrame:frame];
            }
        }

        if self.awaiting_first_layout_visibility {
            self.awaiting_first_layout_visibility = false;
            self.set_visible(self.native_view_visible.get());
        }
    }

    fn on_layout(&mut self, bounds: Bounds<Pixels>, window: &mut Window) {
        #[cfg(target_os = "macos")]
        {
            self.ensure_initialized(bounds, window);
            self.update_frame(bounds);
        }
    }

    /// Show or hide the native NSView. Used to manage z-order when
    /// GPUI overlays (settings, command palette) need to appear on top.
    #[cfg(target_os = "macos")]
    pub fn set_visible(&self, visible: bool) {
        self.native_view_visible.set(visible);
        let effective_visible = visible && !self.awaiting_first_layout_visibility;
        if let Some(terminal) = &self.terminal {
            terminal.set_occlusion(!effective_visible);
            if effective_visible {
                terminal.refresh();
            }
        }
        if let Some(nsview) = self.nsview {
            unsafe {
                let hidden = if effective_visible { NO } else { YES };
                let _: () = msg_send![nsview, setHidden:hidden];
            }
        }
    }

    /// Convert GPUI window-global position to view-local logical coordinates.
    /// Ghostty expects logical pixels (it scales internally via content_scale).
    fn view_local_pos(&self, pos: Point<Pixels>) -> (f64, f64) {
        if let Some(ref bounds) = self.last_bounds {
            (
                f64::from(pos.x - bounds.origin.x),
                f64::from(pos.y - bounds.origin.y),
            )
        } else {
            (f64::from(pos.x), f64::from(pos.y))
        }
    }

    /// Handle key input by forwarding to ghostty's key processing pipeline.
    ///
    /// Uses `ghostty_surface_key` with macOS virtual keycodes so ghostty handles
    /// mode-dependent sequences (application cursor mode, kitty protocol, etc.)
    /// correctly. Falls back to `ghostty_surface_text` for composed/IME text
    /// when no keycode mapping exists.
    fn handle_key_down(&self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let terminal = match self.terminal.as_ref() {
            Some(t) => t,
            None => return,
        };

        let keystroke = &event.keystroke;

        if keystroke.modifiers.platform {
            match keystroke.key.as_str() {
                "c" => {
                    if terminal.has_selection() {
                        if let Some(selection) = terminal.selection_text() {
                            cx.write_to_clipboard(ClipboardItem::new_string(selection));
                        }
                    }
                    return;
                }
                "v" => {
                    if let Some(text) = cx
                        .read_from_clipboard()
                        .and_then(|item| item.text().map(|s| s.to_string()))
                    {
                        if !text.is_empty() {
                            terminal.send_text(&text);
                            cx.notify();
                        }
                    }
                    return;
                }
                _ => {}
            }
        }

        // App-level shortcuts — skip forwarding so GPUI action dispatch handles them.
        // All other Cmd/Ctrl combos pass through to ghostty (e.g. cmd-k for clear screen).
        if keystroke.modifiers.platform {
            match keystroke.key.as_str() {
                // Tab management
                "q" | "w" | "t" | "," => return,
                // Splits (cmd-d, cmd-shift-d)
                "d" => return,
                // Agent & input
                "l" | "i" => return,
                // Edit menu (handled by OS)
                "c" | "v" | "x" | "z" | "a" => return,
                // Command palette (cmd-shift-p)
                "p" if keystroke.modifiers.shift => return,
                // Everything else (including cmd-k) passes to terminal
                _ => {}
            }
        }

        // Ctrl+` is reserved for toggle-input-bar (app shortcut).
        if keystroke.modifiers.control && keystroke.key == "`" {
            return;
        }

        let mods = gpui_mods_to_ghostty(&keystroke.modifiers);
        let key_name = keystroke.key.as_str();

        // Try to map GPUI key name to macOS virtual keycode.
        if let Some(keycode) = gpui_key_to_keycode(key_name) {
            // Build the text field: the character this key produces (if printable).
            // For non-printable keys (arrows, F-keys), text is null.
            let text_string = keystroke.key_char.as_deref().or_else(|| {
                if key_name.len() == 1 {
                    Some(key_name)
                } else {
                    None
                }
            });
            let cstr = text_string.and_then(|s| std::ffi::CString::new(s).ok());
            let text_ptr = cstr
                .as_ref()
                .map(|c| c.as_ptr())
                .unwrap_or(std::ptr::null());

            // unshifted_codepoint: the unicode codepoint of the key without shift.
            // GPUI's `key` field is the unshifted key label.
            let unshifted_codepoint = if key_name.len() == 1 {
                key_name.chars().next().unwrap() as u32
            } else {
                0
            };

            let key_event = ffi::ghostty_input_key_s {
                action: ffi::ghostty_input_action_e::GHOSTTY_ACTION_PRESS,
                mods,
                consumed_mods: 0,
                keycode,
                text: text_ptr,
                unshifted_codepoint,
                composing: false,
            };

            terminal.send_key(key_event);
            return;
        }

        // No keycode mapping — fall back to text input.
        // This handles unusual keys and compose sequences.
        if let Some(ref key_char) = keystroke.key_char {
            if !key_char.is_empty() {
                terminal.send_text(key_char);
                return;
            }
        }
        if key_name.len() == 1 {
            terminal.send_text(key_name);
        }
    }
}

// ── GPUI → ghostty modifier mapping ─────────────────────────

fn gpui_mods_to_ghostty(mods: &Modifiers) -> i32 {
    let mut m: i32 = 0;
    if mods.shift {
        m |= ffi::GHOSTTY_MODS_SHIFT;
    }
    if mods.control {
        m |= ffi::GHOSTTY_MODS_CTRL;
    }
    if mods.alt {
        m |= ffi::GHOSTTY_MODS_ALT;
    }
    if mods.platform {
        m |= ffi::GHOSTTY_MODS_SUPER;
    }
    m
}

// ── GPUI key name → macOS virtual keycode mapping ────────────
//
// These are the kVK_* constants from Carbon/HIToolbox/Events.h.
// GPUI gives us key names (strings); ghostty needs raw keycodes
// so it can process them through its key binding and terminal
// mode pipeline (DECCKM, kitty keyboard protocol, etc.)

fn gpui_key_to_keycode(key: &str) -> Option<u32> {
    Some(match key {
        // Letters (macOS kVK_ANSI_* — NOT sequential, based on QWERTY position)
        "a" => 0x00,
        "s" => 0x01,
        "d" => 0x02,
        "f" => 0x03,
        "h" => 0x04,
        "g" => 0x05,
        "z" => 0x06,
        "x" => 0x07,
        "c" => 0x08,
        "v" => 0x09,
        "b" => 0x0B,
        "q" => 0x0C,
        "w" => 0x0D,
        "e" => 0x0E,
        "r" => 0x0F,
        "y" => 0x10,
        "t" => 0x11,
        "o" => 0x1F,
        "u" => 0x20,
        "i" => 0x22,
        "p" => 0x23,
        "l" => 0x25,
        "j" => 0x26,
        "k" => 0x28,
        "n" => 0x2D,
        "m" => 0x2E,
        // Numbers
        "1" => 0x12,
        "2" => 0x13,
        "3" => 0x14,
        "4" => 0x15,
        "5" => 0x17,
        "6" => 0x16,
        "7" => 0x1A,
        "8" => 0x1C,
        "9" => 0x19,
        "0" => 0x1D,
        // Punctuation
        "-" => 0x1B,
        "=" => 0x18,
        "[" => 0x21,
        "]" => 0x1E,
        "\\" => 0x2A,
        ";" => 0x29,
        "'" => 0x27,
        "`" => 0x32,
        "," => 0x2B,
        "." => 0x2F,
        "/" => 0x2C,
        // Special keys
        "enter" | "return" => 0x24,
        "tab" => 0x30,
        "space" => 0x31,
        "backspace" => 0x33,
        "escape" => 0x35,
        "delete" => 0x75,
        "home" => 0x73,
        "end" => 0x77,
        "pageup" => 0x74,
        "pagedown" => 0x79,
        "up" => 0x7E,
        "down" => 0x7D,
        "left" => 0x7B,
        "right" => 0x7C,
        // Function keys
        "f1" => 0x7A,
        "f2" => 0x78,
        "f3" => 0x63,
        "f4" => 0x76,
        "f5" => 0x60,
        "f6" => 0x61,
        "f7" => 0x62,
        "f8" => 0x64,
        "f9" => 0x65,
        "f10" => 0x6D,
        "f11" => 0x67,
        "f12" => 0x6F,
        _ => return None,
    })
}

impl Drop for GhosttyView {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if let Some(nsview) = self.nsview.take() {
            unsafe {
                let _: () = msg_send![nsview, removeFromSuperview];
            }
        }
    }
}

impl Focusable for GhosttyView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for GhosttyView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = self.focus_handle.clone();
        let entity = cx.entity().downgrade();

        div()
            .size_full()
            .key_context("GhosttyTerminal")
            .track_focus(&focus)
            // Consume Tab/Shift-Tab so Root's focus cycling doesn't intercept them.
            // The actual Tab key is forwarded to ghostty via on_key_down below.
            .on_action(cx.listener(|this, _: &ConsumeTab, _window, _cx| {
                // Send Tab to terminal
                if let Some(terminal) = &this.terminal {
                    let key_event = ffi::ghostty_input_key_s {
                        action: ffi::ghostty_input_action_e::GHOSTTY_ACTION_PRESS,
                        mods: 0,
                        consumed_mods: 0,
                        keycode: 0x30, // Tab
                        text: b"\t\0".as_ptr() as *const _,
                        unshifted_codepoint: '\t' as u32,
                        composing: false,
                    };
                    terminal.send_key(key_event);
                }
            }))
            .on_action(cx.listener(|this, _: &ConsumeTabPrev, _window, _cx| {
                // Send Shift-Tab (backtab) to terminal
                if let Some(terminal) = &this.terminal {
                    let key_event = ffi::ghostty_input_key_s {
                        action: ffi::ghostty_input_action_e::GHOSTTY_ACTION_PRESS,
                        mods: ffi::GHOSTTY_MODS_SHIFT,
                        consumed_mods: 0,
                        keycode: 0x30, // Tab
                        text: std::ptr::null(),
                        unshifted_codepoint: '\t' as u32,
                        composing: false,
                    };
                    terminal.send_key(key_event);
                }
            }))
            .on_action(cx.listener(|this, _: &crate::Copy, _window, cx| {
                let Some(terminal) = &this.terminal else {
                    return;
                };
                if !terminal.has_selection() {
                    return;
                }
                if let Some(selection) = terminal.selection_text() {
                    cx.write_to_clipboard(ClipboardItem::new_string(selection));
                }
            }))
            .on_action(cx.listener(|this, _: &crate::Paste, _window, cx| {
                let Some(terminal) = &this.terminal else {
                    return;
                };
                let Some(text) = cx
                    .read_from_clipboard()
                    .and_then(|item| item.text().map(|s| s.to_string()))
                else {
                    return;
                };
                if text.is_empty() {
                    return;
                }
                terminal.send_text(&text);
                cx.notify();
            }))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.focus(&focus, cx);
                    if let Some(ref terminal) = this.terminal {
                        let (x, y) = this.view_local_pos(event.position);
                        terminal.send_mouse_pos(x, y, 0);
                        terminal.send_mouse_button(true, MouseButton::Left, 0);
                    }
                    cx.emit(GhosttyFocusChanged);
                    cx.notify();
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _window, _cx| {
                    if let Some(ref terminal) = this.terminal {
                        let (x, y) = this.view_local_pos(event.position);
                        terminal.send_mouse_pos(x, y, 0);
                        terminal.send_mouse_button(false, MouseButton::Left, 0);
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, _cx| {
                if let Some(ref terminal) = this.terminal {
                    let (x, y) = this.view_local_pos(event.position);
                    terminal.send_mouse_pos(x, y, 0);
                }
            }))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, _cx| {
                if let Some(ref terminal) = this.terminal {
                    let delta = match event.delta {
                        ScrollDelta::Lines(d) => (f64::from(d.x), f64::from(d.y)),
                        ScrollDelta::Pixels(d) => {
                            // GPUI gives physical pixel deltas on Retina;
                            // normalize to logical coordinates for ghostty.
                            let scale = this.scale_factor as f64;
                            (f64::from(d.x) / scale, f64::from(d.y) / scale)
                        }
                    };
                    terminal.send_mouse_scroll(delta.0, delta.1, 0);
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                this.handle_key_down(event, cx);
                // Force repaint — some ghostty key bindings (e.g. cmd-k clear screen)
                // modify the terminal without emitting GHOSTTY_ACTION_RENDER.
                cx.notify();
                cx.emit(GhosttyFocusChanged);
            }))
            .child(
                canvas(
                    {
                        let entity = entity.clone();
                        move |bounds, window, cx| {
                            let _ = entity.update(cx, |view: &mut GhosttyView, _cx| {
                                view.on_layout(bounds, window);
                            });
                        }
                    },
                    |_bounds, _state, _window, _cx| {},
                )
                .size_full(),
            )
    }
}
