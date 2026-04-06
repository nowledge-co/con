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

#[cfg(target_os = "macos")]
use std::os::raw::c_void;
use std::sync::Arc;

use con_ghostty::ffi;
use con_ghostty::{GhosttyApp, GhosttyTerminal, MouseButton};
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

impl EventEmitter<GhosttyTitleChanged> for GhosttyView {}
impl EventEmitter<GhosttyProcessExited> for GhosttyView {}
impl EventEmitter<GhosttyFocusChanged> for GhosttyView {}

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
                            if terminal.take_needs_render() {
                                cx.notify();
                            }
                            if !terminal.is_alive() {
                                cx.emit(GhosttyProcessExited);
                            }
                            let title = terminal.title();
                            if title != view.last_title {
                                view.last_title = title.clone();
                                cx.emit(GhosttyTitleChanged(title));
                            }
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
        }
    }

    pub fn terminal(&self) -> Option<&Arc<GhosttyTerminal>> {
        self.terminal.as_ref()
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

    #[allow(dead_code)]
    pub fn selection_text(&self) -> Option<String> {
        self.terminal.as_ref().and_then(|t| t.selection_text())
    }

    #[cfg(target_os = "macos")]
    fn ensure_initialized(&mut self, bounds: Bounds<Pixels>, window: &mut Window) {
        if self.initialized {
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
                // Don't set last_bounds here — let update_frame() handle
                // the coordinate flip and position the NSView correctly.
                log::info!(
                    "Ghostty surface created: {}x{} px, scale {}",
                    width_px,
                    height_px,
                    scale
                );
            }
            Err(e) => {
                log::error!("Failed to create ghostty surface: {}", e);
                unsafe {
                    let _: () = msg_send![nsview, removeFromSuperview];
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn update_frame(&mut self, bounds: Bounds<Pixels>) {
        if self.last_bounds.as_ref() == Some(&bounds) {
            return;
        }
        self.last_bounds = Some(bounds);

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

        if let Some(ref terminal) = self.terminal {
            let width_px = (f32::from(bounds.size.width) * self.scale_factor) as u32;
            let height_px = (f32::from(bounds.size.height) * self.scale_factor) as u32;
            terminal.set_size(width_px, height_px);
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
        if let Some(nsview) = self.nsview {
            unsafe {
                let hidden = if visible { NO } else { YES };
                let _: () = msg_send![nsview, setHidden:hidden];
            }
        }
    }

    /// Convert GPUI window-global position to view-local pixel coordinates.
    fn view_local_px(&self, pos: Point<Pixels>) -> (f64, f64) {
        let scale = self.scale_factor as f64;
        if let Some(ref bounds) = self.last_bounds {
            (
                f64::from(pos.x - bounds.origin.x) * scale,
                f64::from(pos.y - bounds.origin.y) * scale,
            )
        } else {
            (f64::from(pos.x) * scale, f64::from(pos.y) * scale)
        }
    }

    /// Handle key input by forwarding to ghostty's key processing pipeline.
    ///
    /// Uses `ghostty_surface_key` with macOS virtual keycodes so ghostty handles
    /// mode-dependent sequences (application cursor mode, kitty protocol, etc.)
    /// correctly. Falls back to `ghostty_surface_text` for composed/IME text
    /// when no keycode mapping exists.
    fn handle_key_down(&self, event: &KeyDownEvent) {
        let terminal = match self.terminal.as_ref() {
            Some(t) => t,
            None => return,
        };

        let keystroke = &event.keystroke;

        // Cmd (platform) keys are reserved for app shortcuts.
        if keystroke.modifiers.platform {
            return;
        }

        // Ctrl+` is reserved for toggle-input-bar (app shortcut).
        // Don't forward to ghostty so GPUI's action dispatch handles it.
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
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.focus(&focus, cx);
                    if let Some(ref terminal) = this.terminal {
                        let (x, y) = this.view_local_px(event.position);
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
                        let (x, y) = this.view_local_px(event.position);
                        terminal.send_mouse_pos(x, y, 0);
                        terminal.send_mouse_button(false, MouseButton::Left, 0);
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, _cx| {
                if let Some(ref terminal) = this.terminal {
                    let (x, y) = this.view_local_px(event.position);
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
                this.handle_key_down(event);
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
