//! GhosttyView — GPUI element wrapping ghostty's GPU-accelerated Metal terminal.
//!
//! Architecture:
//! - Creates an NSView child within GPUI's window view
//! - Passes the NSView to ghostty (macOS embedded platform)
//! - Ghostty renders via Metal into that view (zero-copy, GPU-accelerated)
//! - GPUI input events are forwarded to ghostty's surface input APIs
//! - Terminal state (title, pwd) arrives via ghostty's action callbacks
//!
//! The NSView is positioned and sized by GPUI's layout engine during paint.

#[cfg(target_os = "macos")]
use std::os::raw::c_void;
use std::sync::Arc;

use con_ghostty::{GhosttyApp, GhosttyTerminal, MouseButton};
use gpui::*;

#[cfg(target_os = "macos")]
use cocoa::base::{id, NO, YES};
#[cfg(target_os = "macos")]
use cocoa::foundation::NSRect;
#[cfg(target_os = "macos")]
use objc::{class, msg_send, sel, sel_impl};
#[cfg(target_os = "macos")]
use raw_window_handle::HasWindowHandle;

/// Emitted when the terminal title changes.
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
    /// The native NSView created for ghostty to render into.
    #[cfg(target_os = "macos")]
    nsview: Option<id>,
    /// Track whether we've initialized the surface.
    initialized: bool,
    /// Last known bounds — used to detect resize.
    last_bounds: Option<Bounds<Pixels>>,
    /// Scale factor for Retina displays.
    scale_factor: f32,
}

impl GhosttyView {
    pub fn new(app: Arc<GhosttyApp>, cx: &mut Context<Self>) -> Self {
        // Tick ghostty's event loop periodically
        let app_for_tick = app.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(8))
                    .await;
                app_for_tick.tick();
                if this
                    .update(cx, |view, cx| {
                        if let Some(ref terminal) = view.terminal {
                            if terminal.take_needs_render() {
                                cx.notify();
                            }
                            if !terminal.is_alive() {
                                cx.emit(GhosttyProcessExited);
                            }
                        }
                        let state = view.app.state().lock();
                        let title = state.title.clone();
                        drop(state);
                        cx.emit(GhosttyTitleChanged(title));
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
            #[cfg(target_os = "macos")]
            nsview: None,
            initialized: false,
            last_bounds: None,
            scale_factor: 1.0,
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

    pub fn send_text(&self, text: &str) {
        if let Some(ref terminal) = self.terminal {
            terminal.send_text(text);
        }
    }

    pub fn selection_text(&self) -> Option<String> {
        self.terminal.as_ref().and_then(|t| t.selection_text())
    }

    /// Create the NSView and ghostty surface on first layout.
    #[cfg(target_os = "macos")]
    fn ensure_initialized(&mut self, bounds: Bounds<Pixels>, window: &mut Window) {
        if self.initialized {
            return;
        }

        self.scale_factor = window.scale_factor();

        // Get the parent NSView from GPUI's window using HasWindowHandle trait
        let raw_handle: raw_window_handle::WindowHandle<'_> =
            match HasWindowHandle::window_handle(window) {
                Ok(handle) => handle,
                Err(_) => return,
            };

        let parent_nsview = match raw_handle.as_raw() {
            raw_window_handle::RawWindowHandle::AppKit(handle) => {
                handle.ns_view.as_ptr() as id
            }
            _ => return,
        };

        let nsview: id = unsafe {
            let frame = NSRect::new(
                cocoa::foundation::NSPoint::new(
                    f64::from(bounds.origin.x),
                    f64::from(bounds.origin.y),
                ),
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
        match self.app.new_surface(nsview as *mut c_void, scale, None) {
            Ok(terminal) => {
                let width_px = (f32::from(bounds.size.width) * self.scale_factor) as u32;
                let height_px =
                    (f32::from(bounds.size.height) * self.scale_factor) as u32;
                terminal.set_size(width_px, height_px);
                terminal.set_content_scale(scale);
                terminal.set_focus(true);
                self.terminal = Some(Arc::new(terminal));
                self.nsview = Some(nsview);
                self.initialized = true;
                self.last_bounds = Some(bounds);
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

    /// Update NSView frame to match GPUI layout bounds.
    #[cfg(target_os = "macos")]
    fn update_frame(&mut self, bounds: Bounds<Pixels>) {
        if self.last_bounds.as_ref() == Some(&bounds) {
            return;
        }
        self.last_bounds = Some(bounds);

        if let Some(nsview) = self.nsview {
            unsafe {
                // GPUI uses top-left origin; NSView uses bottom-left.
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

    /// Called from canvas prepaint to initialize and resize.
    fn on_layout(&mut self, bounds: Bounds<Pixels>, window: &mut Window) {
        #[cfg(target_os = "macos")]
        {
            self.ensure_initialized(bounds, window);
            self.update_frame(bounds);
        }
    }
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

        // Pre-calculate layout by doing init/resize in before_layout via deferred
        // We use an after_layout callback to position the NSView.
        let entity = cx.entity().downgrade();

        div()
            .size_full()
            .track_focus(&focus)
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.focus(&focus, cx);
                    if let Some(ref terminal) = this.terminal {
                        let pos = event.position;
                        terminal.send_mouse_button(true, MouseButton::Left, 0);
                        terminal.send_mouse_pos(
                            f64::from(pos.x) * this.scale_factor as f64,
                            f64::from(pos.y) * this.scale_factor as f64,
                            0,
                        );
                    }
                    cx.emit(GhosttyFocusChanged);
                    cx.notify();
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _window, _cx| {
                    if let Some(ref terminal) = this.terminal {
                        let pos = event.position;
                        terminal.send_mouse_button(false, MouseButton::Left, 0);
                        terminal.send_mouse_pos(
                            f64::from(pos.x) * this.scale_factor as f64,
                            f64::from(pos.y) * this.scale_factor as f64,
                            0,
                        );
                    }
                }),
            )
            .on_mouse_move(cx.listener(
                |this, event: &MouseMoveEvent, _window, _cx| {
                    if let Some(ref terminal) = this.terminal {
                        let pos = event.position;
                        terminal.send_mouse_pos(
                            f64::from(pos.x) * this.scale_factor as f64,
                            f64::from(pos.y) * this.scale_factor as f64,
                            0,
                        );
                    }
                },
            ))
            .on_scroll_wheel(cx.listener(
                |this, event: &ScrollWheelEvent, _window, _cx| {
                    if let Some(ref terminal) = this.terminal {
                        let delta = match event.delta {
                            ScrollDelta::Lines(d) => (f64::from(d.x), f64::from(d.y)),
                            ScrollDelta::Pixels(d) => (f64::from(d.x), f64::from(d.y)),
                        };
                        terminal.send_mouse_scroll(delta.0, delta.1, 0);
                    }
                },
            ))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, _cx| {
                if let Some(ref terminal) = this.terminal {
                    let key = &event.keystroke.key;
                    if !key.is_empty() && !event.keystroke.modifiers.platform {
                        terminal.send_text(key);
                    }
                }
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
