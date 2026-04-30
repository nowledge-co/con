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
use std::ops::Range;
#[cfg(target_os = "macos")]
use std::os::raw::c_void;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use con_ghostty::ffi;
use con_ghostty::{
    GhosttyApp, GhosttySplitDirection, GhosttySurfaceEvent, GhosttyTerminal, MouseButton,
};
use gpui::{prelude::FluentBuilder as _, *};
use gpui_component::ActiveTheme;

use crate::terminal_paste::{
    TerminalPastePayload, payload_from_clipboard, payload_from_external_paths,
};

// Actions to intercept Tab/Shift-Tab before Root's focus-cycling handler.
actions!(ghostty, [ConsumeTab, ConsumeTabPrev]);

#[cfg(target_os = "macos")]
use cocoa::appkit::NSWindowOrderingMode;
#[cfg(target_os = "macos")]
use cocoa::base::{NO, YES, id};
#[cfg(target_os = "macos")]
use cocoa::foundation::NSRect;
#[cfg(target_os = "macos")]
use objc::{class, msg_send, sel, sel_impl};
#[cfg(target_os = "macos")]
use raw_window_handle::HasWindowHandle;

#[cfg(target_os = "macos")]
const NS_VIEW_WIDTH_SIZABLE: usize = 1 << 1;
#[cfg(target_os = "macos")]
const NS_VIEW_HEIGHT_SIZABLE: usize = 1 << 4;
#[cfg(target_os = "macos")]
const NS_VIEW_LAYER_CONTENTS_REDRAW_DURING_VIEW_RESIZE: isize = 2;

fn perf_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("CON_GHOSTTY_PROFILE").is_some_and(|v| !v.is_empty() && v != "0")
    })
}

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
    host_view: Option<id>,
    #[cfg(target_os = "macos")]
    document_view: Option<id>,
    #[cfg(target_os = "macos")]
    nsview: Option<id>,
    #[cfg(target_os = "macos")]
    native_scroll_document_frame: Option<(f64, f64)>,
    #[cfg(target_os = "macos")]
    native_scroll_y: Option<f64>,
    #[cfg(target_os = "macos")]
    native_scroll_surface_frame: Option<(f64, f64, f64, f64)>,
    #[cfg(target_os = "macos")]
    native_backing_color: Option<(u8, u8, u8, u8)>,
    #[cfg(target_os = "macos")]
    pending_native_layout: bool,
    initialized: bool,
    last_bounds: Option<Bounds<Pixels>>,
    scale_factor: f32,
    last_title: Option<String>,
    /// Data queued for the PTY before the surface was created.
    /// Flushed once in `ensure_initialized()` after the terminal exists.
    pending_write: Option<Vec<u8>>,
    /// Desired native view visibility, including before the NSView exists.
    native_view_visible: Cell<bool>,
    /// Desired Ghostty focus state. This is independent from GPUI keyboard
    /// focus so broadcast/custom pane scopes can light multiple cursors.
    surface_focused: Cell<bool>,
    /// When a surface was created before a real layout pass, keep it hidden until
    /// update_frame commits an actual pane geometry.
    awaiting_first_layout_visibility: bool,
    /// Guard: emit GhosttyProcessExited exactly once, not every 8ms tick.
    process_exit_emitted: bool,
    /// Retry surface creation after transient libghostty initialization failures.
    next_surface_init_retry_at: Option<Instant>,
    /// Last mouse position seen by GPUI. Ghostty link hover state depends on
    /// both position and modifiers, but macOS sends modifier changes without a
    /// mouse-move event when the pointer is stationary.
    #[cfg(target_os = "macos")]
    last_mouse_position: Option<Point<Pixels>>,
    ime_marked_text: Option<String>,
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
        Self {
            app,
            terminal: None,
            focus_handle: cx.focus_handle(),
            initial_cwd: cwd,
            initial_font_size: font_size,
            #[cfg(target_os = "macos")]
            host_view: None,
            #[cfg(target_os = "macos")]
            document_view: None,
            #[cfg(target_os = "macos")]
            nsview: None,
            #[cfg(target_os = "macos")]
            native_scroll_document_frame: None,
            #[cfg(target_os = "macos")]
            native_scroll_y: None,
            #[cfg(target_os = "macos")]
            native_scroll_surface_frame: None,
            #[cfg(target_os = "macos")]
            native_backing_color: None,
            #[cfg(target_os = "macos")]
            pending_native_layout: false,
            initialized: false,
            last_bounds: None,
            scale_factor: 1.0,
            last_title: None,
            pending_write: None,
            native_view_visible: Cell::new(true),
            surface_focused: Cell::new(true),
            awaiting_first_layout_visibility: false,
            process_exit_emitted: false,
            next_surface_init_retry_at: None,
            #[cfg(target_os = "macos")]
            last_mouse_position: None,
            ime_marked_text: None,
        }
    }

    pub fn drain_surface_state(
        &mut self,
        sync_native_scroll: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(ref terminal) = self.terminal else {
            return false;
        };

        let started = perf_trace_enabled().then(Instant::now);
        let mut changed = false;
        for event in terminal.take_pending_events() {
            changed = true;
            match event {
                GhosttySurfaceEvent::SplitRequest(direction) => {
                    cx.emit(GhosttySplitRequested(direction));
                }
                GhosttySurfaceEvent::OpenUrl(url) => {
                    cx.open_url(&url);
                }
            }
        }

        let _ = terminal.take_needs_render();

        if !terminal.is_alive() && !self.process_exit_emitted {
            self.process_exit_emitted = true;
            changed = true;
            cx.emit(GhosttyProcessExited);
        }

        let title = terminal.title();
        if title != self.last_title {
            self.last_title = title.clone();
            changed = true;
            cx.emit(GhosttyTitleChanged(title));
        }

        #[cfg(target_os = "macos")]
        if sync_native_scroll {
            self.sync_native_scroll_view();
        }

        if let Some(started) = started {
            if changed {
                log::info!(
                    target: "con::perf",
                    "drain_surface_state changed=1 elapsed_ms={:.3}",
                    started.elapsed().as_secs_f64() * 1000.0
                );
            }
        }

        changed
    }

    fn send_terminal_paste_payload(&self, payload: TerminalPastePayload) -> bool {
        let Some(terminal) = &self.terminal else {
            return false;
        };

        match payload {
            TerminalPastePayload::Text(text) if !text.is_empty() => {
                terminal.send_text(&text);
                true
            }
            TerminalPastePayload::ForwardCtrlV => {
                terminal.send_key(ffi::ghostty_input_key_s {
                    action: ffi::ghostty_input_action_e::GHOSTTY_ACTION_PRESS,
                    mods: ffi::GHOSTTY_MODS_CTRL,
                    consumed_mods: 0,
                    keycode: 0x09, // kVK_ANSI_V
                    text: std::ptr::null(),
                    unshifted_codepoint: 'v' as u32,
                    composing: false,
                });
                true
            }
            TerminalPastePayload::Text(_) => false,
        }
    }

    fn paste_from_clipboard(&self, cx: &mut Context<Self>) -> bool {
        let Some(payload) = cx
            .read_from_clipboard()
            .and_then(|item| payload_from_clipboard(&item))
        else {
            return false;
        };

        self.send_terminal_paste_payload(payload)
    }

    pub fn pump_deferred_work(&mut self, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        let mut changed = false;

        if !self.initialized
            && self
                .next_surface_init_retry_at
                .is_some_and(|deadline| now >= deadline)
        {
            self.next_surface_init_retry_at = None;
            changed = true;
            cx.notify();
        }

        changed
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

    fn show_layout_fallback(&self) -> bool {
        self.terminal.is_none()
            || self.awaiting_first_layout_visibility
            || self.pending_native_layout
    }

    #[allow(dead_code)]
    pub fn selection_text(&self) -> Option<String> {
        self.terminal.as_ref().and_then(|t| t.selection_text())
    }

    #[cfg(target_os = "macos")]
    fn detach_host_view(&mut self) {
        if let Some(host_view) = self.host_view.take() {
            unsafe {
                let superview: id = msg_send![host_view, superview];
                if !superview.is_null() {
                    let _: () = msg_send![host_view, removeFromSuperview];
                }
            }
        }
        self.document_view = None;
        self.nsview = None;
        self.native_scroll_document_frame = None;
        self.native_scroll_y = None;
        self.native_scroll_surface_frame = None;
        self.native_backing_color = None;
        self.pending_native_layout = false;
    }

    #[cfg(not(target_os = "macos"))]
    fn detach_host_view(&mut self) {}

    pub fn shutdown_surface(&mut self) {
        self.native_view_visible.set(false);

        if let Some(ref terminal) = self.terminal {
            terminal.set_focus(false);
            terminal.set_occlusion(true);
        }

        self.detach_host_view();
        self.terminal = None;
        self.initialized = false;
        self.awaiting_first_layout_visibility = false;
        self.last_bounds = None;
        self.last_title = None;
        self.pending_write = None;
        self.next_surface_init_retry_at = None;
        #[cfg(target_os = "macos")]
        {
            self.last_mouse_position = None;
            self.native_scroll_document_frame = None;
            self.native_scroll_y = None;
            self.native_scroll_surface_frame = None;
            self.pending_native_layout = false;
        }
        self.ime_marked_text = None;
    }

    pub fn set_surface_focus_state(&mut self, focused: bool) {
        let changed = self.surface_focused.replace(focused) != focused;
        if !changed {
            return;
        }

        if let Some(ref terminal) = self.terminal {
            terminal.set_focus(focused);
            terminal.refresh();
        }
    }

    #[cfg(target_os = "macos")]
    fn ensure_initialized(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
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

        let gpui_nsview = match raw_handle.as_raw() {
            raw_window_handle::RawWindowHandle::AppKit(handle) => handle.ns_view.as_ptr() as id,
            _ => return,
        };

        let parent_nsview: id = unsafe {
            let superview: id = msg_send![gpui_nsview, superview];
            if superview.is_null() {
                gpui_nsview
            } else {
                superview
            }
        };

        // Create with a zero-origin frame — update_frame() will set the
        // correct flipped position immediately after initialization.
        let (host_view, document_view, nsview): (id, id, id) = unsafe {
            let frame = NSRect::new(
                cocoa::foundation::NSPoint::new(0.0, 0.0),
                cocoa::foundation::NSSize::new(
                    f64::from(bounds.size.width),
                    f64::from(bounds.size.height),
                ),
            );
            let host: id = msg_send![class!(NSScrollView), alloc];
            let host: id = msg_send![host, initWithFrame:frame];
            let _: () = msg_send![host, setDrawsBackground:NO];
            let _: () = msg_send![host, setHasVerticalScroller:NO];
            let _: () = msg_send![host, setHasHorizontalScroller:NO];
            let _: () = msg_send![host, setAutohidesScrollers:NO];
            let _: () = msg_send![host, setUsesPredominantAxisScrolling:YES];
            let _: () =
                msg_send![host, setAutoresizingMask:NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE];
            let _: () = msg_send![host, setAutoresizesSubviews:YES];
            let _: () = msg_send![
                host,
                setLayerContentsRedrawPolicy:NS_VIEW_LAYER_CONTENTS_REDRAW_DURING_VIEW_RESIZE
            ];
            let content_view: id = msg_send![host, contentView];
            let _: () = msg_send![content_view, setClipsToBounds:YES];
            let _: () = msg_send![host, setHidden:YES];
            let _: () = msg_send![
                parent_nsview,
                addSubview: host
                positioned: NSWindowOrderingMode::NSWindowBelow
                relativeTo: gpui_nsview
            ];

            let document: id = msg_send![class!(NSView), alloc];
            let document: id = msg_send![document, initWithFrame:frame];
            let _: () = msg_send![host, setDocumentView:document];

            let surface: id = msg_send![class!(NSView), alloc];
            let surface: id = msg_send![surface, initWithFrame:frame];
            let _: () = msg_send![surface, setWantsLayer:YES];
            let _: () = msg_send![
                surface,
                setAutoresizingMask:NS_VIEW_WIDTH_SIZABLE | NS_VIEW_HEIGHT_SIZABLE
            ];
            let _: () = msg_send![
                surface,
                setLayerContentsRedrawPolicy:NS_VIEW_LAYER_CONTENTS_REDRAW_DURING_VIEW_RESIZE
            ];
            let _: () = msg_send![document, addSubview:surface];
            (host, document, surface)
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
                terminal.set_focus(self.surface_focused.get());
                self.terminal = Some(Arc::new(terminal));
                self.host_view = Some(host_view);
                self.document_view = Some(document_view);
                self.nsview = Some(nsview);
                self.initialized = true;
                self.next_surface_init_retry_at = None;
                self.sync_native_backing_background();
                self.sync_window_background_blur();
                // Force update_frame() to position the newly-created host.
                // If a previous layout recorded the same bounds while the
                // surface was still pending, an early-return here leaves the
                // NSView at its bootstrap origin until a manual divider resize.
                self.last_bounds = None;
                self.reset_native_scroll_layout_cache();
                log::info!(
                    "Ghostty surface created: {}x{} px, scale {}",
                    width_px,
                    height_px,
                    scale
                );
                self.sync_native_scroll_view();

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
                self.host_view = Some(host_view);
                self.document_view = Some(document_view);
                self.nsview = Some(nsview);
                self.detach_host_view();
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn native_backing_rgba(&self) -> Option<(u8, u8, u8, u8)> {
        let rgb = self.app.background_rgb()?;
        let alpha = self.app.background_opacity().unwrap_or(1.0).clamp(0.0, 1.0);
        Some((rgb[0], rgb[1], rgb[2], (alpha * 255.0).round() as u8))
    }

    #[cfg(target_os = "macos")]
    fn apply_native_backing_color(view: id, rgba: (u8, u8, u8, u8)) {
        if view.is_null() {
            return;
        }

        unsafe {
            let _: () = msg_send![view, setWantsLayer:YES];
            let layer: id = msg_send![view, layer];
            if layer.is_null() {
                return;
            }

            let color: id = msg_send![
                class!(NSColor),
                colorWithSRGBRed:f64::from(rgba.0) / 255.0
                green:f64::from(rgba.1) / 255.0
                blue:f64::from(rgba.2) / 255.0
                alpha:f64::from(rgba.3) / 255.0
            ];
            let cg_color: id = msg_send![color, CGColor];
            let _: () = msg_send![layer, setBackgroundColor:cg_color];
        }
    }

    #[cfg(target_os = "macos")]
    fn sync_native_backing_background(&mut self) {
        let Some(rgba) = self.native_backing_rgba() else {
            return;
        };
        if self.native_backing_color == Some(rgba) {
            return;
        }

        if let Some(host_view) = self.host_view {
            Self::apply_native_backing_color(host_view, rgba);
            unsafe {
                let content_view: id = msg_send![host_view, contentView];
                Self::apply_native_backing_color(content_view, rgba);
            }
        }
        if let Some(document_view) = self.document_view {
            Self::apply_native_backing_color(document_view, rgba);
        }
        if let Some(nsview) = self.nsview {
            Self::apply_native_backing_color(nsview, rgba);
        }
        self.native_backing_color = Some(rgba);
    }

    fn reset_native_scroll_layout_cache(&mut self) {
        self.native_scroll_document_frame = None;
        self.native_scroll_y = None;
        self.native_scroll_surface_frame = None;
    }

    #[cfg(target_os = "macos")]
    pub fn mark_native_layout_pending(&mut self, cx: &mut Context<Self>) {
        if self.terminal.is_none() && self.host_view.is_none() {
            return;
        }

        self.pending_native_layout = true;
        self.apply_native_visibility();
        cx.notify();
    }

    #[cfg(target_os = "macos")]
    pub fn ensure_initialized_for_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let showed_layout_fallback = self.show_layout_fallback();
        let Some(bounds) = self.last_bounds else {
            // Never create a native NSView from estimated bounds. Nested split
            // and zoom transitions move panes between flex subtrees; a bootstrap
            // frame can survive long enough to paint in the wrong pane until a
            // manual divider resize corrects it. The GPUI fallback matte covers
            // this pane until the canvas supplies authoritative layout bounds.
            self.awaiting_first_layout_visibility = true;
            cx.notify();
            return;
        };

        self.ensure_initialized(bounds, window, cx);
        self.update_frame(bounds);
        if showed_layout_fallback != self.show_layout_fallback() {
            cx.notify();
        }
    }

    #[cfg(target_os = "macos")]
    fn update_frame(&mut self, bounds: Bounds<Pixels>) {
        if self.last_bounds.as_ref() == Some(&bounds)
            && !self.awaiting_first_layout_visibility
            && !self.pending_native_layout
        {
            return;
        }
        let started = perf_trace_enabled().then(Instant::now);
        self.last_bounds = Some(bounds);
        self.sync_native_backing_background();

        if let Some(host_view) = self.host_view {
            unsafe {
                let superview: id = msg_send![host_view, superview];
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
                let _: () = msg_send![host_view, setFrame:frame];
            }
        }

        self.commit_surface_resize(bounds);
        self.reset_native_scroll_layout_cache();
        self.sync_native_scroll_view();

        if let Some(started) = started {
            let in_live_resize = if let Some(host_view) = self.host_view {
                unsafe {
                    let nswindow: id = msg_send![host_view, window];
                    if nswindow.is_null() {
                        false
                    } else {
                        let live: cocoa::base::BOOL = msg_send![nswindow, inLiveResize];
                        live == YES
                    }
                }
            } else {
                false
            };
            log::info!(
                target: "con::perf",
                "update_frame logical_pt={:.1}x{:.1} in_live_resize={} elapsed_ms={:.3}",
                bounds.size.width.as_f32(),
                bounds.size.height.as_f32(),
                in_live_resize,
                started.elapsed().as_secs_f64() * 1000.0
            );
        }

        if self.awaiting_first_layout_visibility {
            self.awaiting_first_layout_visibility = false;
        }
        self.pending_native_layout = false;
        self.apply_native_visibility();
    }

    #[cfg(target_os = "macos")]
    fn sync_native_scroll_view(&mut self) {
        let (Some(scroll_view), Some(document_view), Some(nsview), Some(bounds), Some(terminal)) = (
            self.host_view,
            self.document_view,
            self.nsview,
            self.last_bounds,
            self.terminal.as_ref(),
        ) else {
            return;
        };

        let visible_width = f64::from(bounds.size.width.as_f32().max(1.0));
        let visible_height = f64::from(bounds.size.height.as_f32().max(1.0));
        let size = terminal.size();
        let cell_height = if size.cell_height_px > 0 && self.scale_factor > 0.0 {
            f64::from(size.cell_height_px) / f64::from(self.scale_factor)
        } else {
            0.0
        };
        let scrollbar = terminal.scrollbar();
        let document_height = if let Some(scrollbar) = scrollbar {
            let total_rows = scrollbar.total.max(scrollbar.len).max(1);
            let visible_rows = scrollbar.len.max(1).min(total_rows);
            let padding = if cell_height > 0.0 {
                (visible_height - (visible_rows as f64 * cell_height)).max(0.0)
            } else {
                0.0
            };
            if cell_height > 0.0 {
                (total_rows as f64 * cell_height + padding).max(visible_height)
            } else {
                visible_height
            }
        } else {
            visible_height
        };

        unsafe {
            let document_frame_key = (visible_width, document_height);
            let document_frame_changed =
                self.native_scroll_document_frame != Some(document_frame_key);
            if document_frame_changed {
                let document_frame = NSRect::new(
                    cocoa::foundation::NSPoint::new(0.0, 0.0),
                    cocoa::foundation::NSSize::new(visible_width, document_height),
                );
                let _: () = msg_send![document_view, setFrame:document_frame];
                self.native_scroll_document_frame = Some(document_frame_key);
            }

            let content_view: id = msg_send![scroll_view, contentView];
            let mut scroll_changed = false;
            if let Some(scrollbar) = scrollbar {
                let total_rows = scrollbar.total.max(scrollbar.len).max(1);
                let visible_rows = scrollbar.len.max(1).min(total_rows);
                let offset_from_bottom = if cell_height > 0.0 {
                    (total_rows
                        .saturating_sub(scrollbar.offset.min(total_rows))
                        .saturating_sub(visible_rows)) as f64
                        * cell_height
                } else {
                    0.0
                };
                let scroll_y =
                    offset_from_bottom.clamp(0.0, (document_height - visible_height).max(0.0));
                if self.native_scroll_y != Some(scroll_y) {
                    let _: () = msg_send![
                        content_view,
                        scrollToPoint:cocoa::foundation::NSPoint::new(0.0, scroll_y)
                    ];
                    self.native_scroll_y = Some(scroll_y);
                    scroll_changed = true;
                }
            } else {
                self.native_scroll_y = None;
            }
            if document_frame_changed || scroll_changed {
                let _: () = msg_send![scroll_view, reflectScrolledClipView:content_view];
            }

            let visible_rect: NSRect = msg_send![content_view, documentVisibleRect];
            let surface_frame_key = (
                visible_rect.origin.x,
                visible_rect.origin.y,
                visible_width,
                visible_height,
            );
            if self.native_scroll_surface_frame == Some(surface_frame_key) {
                return;
            }
            let surface_frame = NSRect::new(
                visible_rect.origin,
                cocoa::foundation::NSSize::new(visible_width, visible_height),
            );
            let _: () = msg_send![nsview, setFrame:surface_frame];
            self.native_scroll_surface_frame = Some(surface_frame_key);
        }
    }

    fn on_layout(&mut self, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        let showed_layout_fallback = self.show_layout_fallback();
        #[cfg(target_os = "macos")]
        {
            self.ensure_initialized(bounds, window, cx);
            self.update_frame(bounds);
        }
        if showed_layout_fallback != self.show_layout_fallback() {
            cx.notify();
        }
    }

    #[cfg(target_os = "macos")]
    fn surface_size_in_backing_pixels(&self, bounds: Bounds<Pixels>) -> (u32, u32) {
        let logical_size = cocoa::foundation::NSSize::new(
            f64::from(bounds.size.width.as_f32().max(1.0)),
            f64::from(bounds.size.height.as_f32().max(1.0)),
        );

        if let Some(nsview) = self.nsview {
            unsafe {
                let backing: cocoa::foundation::NSSize =
                    msg_send![nsview, convertSizeToBacking: logical_size];
                return (
                    backing.width.max(1.0).round() as u32,
                    backing.height.max(1.0).round() as u32,
                );
            }
        }

        (
            (logical_size.width * f64::from(self.scale_factor))
                .max(1.0)
                .round() as u32,
            (logical_size.height * f64::from(self.scale_factor))
                .max(1.0)
                .round() as u32,
        )
    }

    #[cfg(target_os = "macos")]
    fn commit_surface_resize(&mut self, bounds: Bounds<Pixels>) {
        let Some(ref terminal) = self.terminal else {
            return;
        };

        let (width_px, height_px) = self.surface_size_in_backing_pixels(bounds);
        let size = terminal.size();
        if size.width_px == width_px && size.height_px == height_px {
            return;
        }

        let started = perf_trace_enabled().then(Instant::now);
        terminal.set_size(width_px, height_px);
        terminal.draw();
        if let Some(started) = started {
            let elapsed = started.elapsed();
            log::info!(
                target: "con::perf",
                "surface resize request old_px={}x{} old_grid={}x{} new_px={}x{} logical_pt={:.1}x{:.1} call_ms={:.3}",
                size.width_px,
                size.height_px,
                size.columns,
                size.rows,
                width_px,
                height_px,
                bounds.size.width.as_f32(),
                bounds.size.height.as_f32(),
                elapsed.as_secs_f64() * 1000.0
            );
        }
    }

    /// Show or hide the native NSView. Used to manage z-order when
    /// GPUI overlays (settings, command palette) need to appear on top.
    #[cfg(target_os = "macos")]
    pub fn set_visible(&self, visible: bool) {
        self.native_view_visible.set(visible);
        self.apply_native_visibility();
    }

    #[cfg(target_os = "macos")]
    fn apply_native_visibility(&self) {
        if let Some(host_view) = self.host_view {
            unsafe {
                let effective_visible = self.native_view_visible.get()
                    && !self.awaiting_first_layout_visibility
                    && !self.pending_native_layout;
                let hidden = if effective_visible { NO } else { YES };
                let _: () = msg_send![host_view, setHidden:hidden];
                if effective_visible {
                    let _: () = msg_send![host_view, setNeedsDisplay:YES];
                }
            }
        }
        if self.native_view_visible.get()
            && !self.awaiting_first_layout_visibility
            && !self.pending_native_layout
        {
            self.draw_surface_now();
        }
    }

    #[cfg(target_os = "macos")]
    fn draw_surface_now(&self) {
        let Some(terminal) = self.terminal.as_ref() else {
            return;
        };
        terminal.draw();
    }

    #[cfg(target_os = "macos")]
    pub fn sync_window_background_blur(&mut self) {
        self.sync_native_backing_background();

        let Some(host_view) = self.host_view else {
            return;
        };

        unsafe {
            let nswindow: id = msg_send![host_view, window];
            if nswindow.is_null() {
                return;
            }
            ffi::ghostty_set_window_background_blur(self.app.raw(), nswindow as *mut c_void);
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn sync_window_background_blur(&self) {}

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

    #[cfg(target_os = "macos")]
    fn refresh_mouse_modifiers(&self, modifiers: &Modifiers) {
        let (Some(terminal), Some(position)) = (self.terminal.as_ref(), self.last_mouse_position)
        else {
            return;
        };

        let (x, y) = self.view_local_pos(position);
        terminal.send_mouse_pos(x, y, gpui_mods_to_ghostty(modifiers));
    }

    /// Handle key input by forwarding to ghostty's key processing pipeline.
    ///
    /// Uses `ghostty_surface_key` with macOS virtual keycodes so ghostty handles
    /// mode-dependent sequences (application cursor mode, kitty protocol, etc.)
    /// correctly. Falls back to `ghostty_surface_text` for composed/IME text
    /// when no keycode mapping exists.
    fn handle_key_down(
        &self,
        event: &KeyDownEvent,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let terminal = match self.terminal.as_ref() {
            Some(t) => t,
            None => return false,
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
                    return true;
                }
                "v" => {
                    if self.paste_from_clipboard(cx) {
                        cx.notify();
                    }
                    return true;
                }
                _ => {}
            }
        }

        if crate::terminal_shortcuts::key_down_starts_action_binding(
            event,
            window,
            &crate::TogglePaneZoom,
        ) {
            return false;
        }

        // App-level shortcuts — skip forwarding so GPUI action dispatch handles them.
        // All other Cmd/Ctrl combos pass through to ghostty (e.g. cmd-k for clear screen).
        if keystroke.modifiers.platform {
            match keystroke.key.as_str() {
                // Tab management
                "q" | "w" | "t" | "," | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => {
                    return false;
                }
                // Window cycling (cmd-`, cmd-shift-`)
                "`" | "~" | ">" | "<" => return false,
                // Splits (cmd-d, cmd-shift-d)
                "d" => return false,
                // Agent & input
                "l" | "i" => return false,
                // Edit menu (handled by OS)
                "c" | "v" | "x" | "z" | "a" => return false,
                // Command palette (cmd-shift-p)
                "p" if keystroke.modifiers.shift => return false,
                // Everything else (including cmd-k) passes to terminal
                _ => {}
            }
        }

        // Ctrl+` is reserved for toggle-input-bar (app shortcut).
        if keystroke.modifiers.control && keystroke.key == "`" {
            return false;
        }

        let mods = gpui_mods_to_ghostty(&keystroke.modifiers);
        let key_name = keystroke.key.as_str();

        // Try to map GPUI key name to macOS virtual keycode.
        if let Some((keycode, unshifted_codepoint)) = gpui_key_to_keycode(key_name) {
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
            return true;
        }

        // No keycode mapping — fall back to text input.
        // This handles unusual keys and compose sequences.
        if let Some(ref key_char) = keystroke.key_char {
            if !key_char.is_empty() {
                terminal.send_text(key_char);
                return true;
            }
        }
        if key_name.len() == 1 {
            terminal.send_text(key_name);
            return true;
        }
        false
    }
}

struct GhosttyInputHandler {
    view: WeakEntity<GhosttyView>,
}

impl InputHandler for GhosttyInputHandler {
    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&mut self, _window: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        self.view
            .read_with(cx, |view, _| {
                view.ime_marked_text
                    .as_ref()
                    .map(|text| 0..text.encode_utf16().count())
            })
            .ok()
            .flatten()
    }

    fn text_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<String> {
        *adjusted_range = Some(0..0);
        Some(String::new())
    }

    fn replace_text_in_range(
        &mut self,
        _replacement_range: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut App,
    ) {
        if text.is_empty() {
            return;
        }
        let _ = self.view.update(cx, |view, _| {
            let had_marked_text = view.ime_marked_text.take().is_some();
            if let Some(terminal) = &view.terminal {
                if !had_marked_text && should_send_ime_insert_as_key_event(text) {
                    send_ime_insert_as_key_events(terminal, text);
                } else {
                    terminal.send_text(text);
                }
                terminal.refresh();
            }
        });
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _range_utf16: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut App,
    ) {
        let _ = self.view.update(cx, |view, _| {
            view.ime_marked_text = if new_text.is_empty() {
                None
            } else {
                Some(new_text.to_string())
            };
            if let Some(terminal) = &view.terminal {
                terminal.refresh();
            }
        });
    }

    fn unmark_text(&mut self, _window: &mut Window, cx: &mut App) {
        let _ = self.view.update(cx, |view, _| {
            view.ime_marked_text = None;
            if let Some(terminal) = &view.terminal {
                terminal.refresh();
            }
        });
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        _window: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        self.view
            .read_with(cx, |view, _| {
                let terminal = view.terminal.as_ref()?;
                let bounds = view.last_bounds?;
                let ime_point = terminal.ime_point();
                Some(Bounds::new(
                    point(
                        bounds.origin.x + px(ime_point.x as f32),
                        bounds.origin.y + px(ime_point.y as f32),
                    ),
                    size(
                        px((ime_point.width as f32).max(1.0)),
                        px((ime_point.height as f32).max(1.0)),
                    ),
                ))
            })
            .ok()
            .flatten()
    }

    fn character_index_for_point(
        &mut self,
        _point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Option<usize> {
        Some(0)
    }

    fn prefers_ime_for_printable_keys(&mut self, _window: &mut Window, _cx: &mut App) -> bool {
        true
    }
}

fn should_send_ime_insert_as_key_event(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|ch| ch.is_ascii() && !ch.is_control())
}

fn send_ime_insert_as_key_events(terminal: &GhosttyTerminal, text: &str) {
    for ch in text.chars() {
        let mut buffer = [0; 4];
        terminal.send_text_as_key_event(ch.encode_utf8(&mut buffer));
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

fn gpui_scroll_mods_to_ghostty(delta: &ScrollDelta) -> i32 {
    match delta {
        ScrollDelta::Pixels(_) => ffi::GHOSTTY_SCROLL_MODS_PRECISION,
        ScrollDelta::Lines(_) => 0,
    }
}

// ── GPUI key name → macOS virtual keycode mapping ────────────
//
// These are the kVK_* constants from Carbon/HIToolbox/Events.h.
// GPUI gives us key names (strings); ghostty needs raw keycodes
// so it can process them through its key binding and terminal
// mode pipeline (DECCKM, kitty keyboard protocol, etc.)

fn gpui_key_to_keycode(key: &str) -> Option<(u32, u32)> {
    Some(match key {
        // Letters (macOS kVK_ANSI_* — NOT sequential, based on QWERTY position)
        "a" => (0x00, 'a' as u32),
        "s" => (0x01, 's' as u32),
        "d" => (0x02, 'd' as u32),
        "f" => (0x03, 'f' as u32),
        "h" => (0x04, 'h' as u32),
        "g" => (0x05, 'g' as u32),
        "z" => (0x06, 'z' as u32),
        "x" => (0x07, 'x' as u32),
        "c" => (0x08, 'c' as u32),
        "v" => (0x09, 'v' as u32),
        "b" => (0x0B, 'b' as u32),
        "q" => (0x0C, 'q' as u32),
        "w" => (0x0D, 'w' as u32),
        "e" => (0x0E, 'e' as u32),
        "r" => (0x0F, 'r' as u32),
        "y" => (0x10, 'y' as u32),
        "t" => (0x11, 't' as u32),
        "o" => (0x1F, 'o' as u32),
        "u" => (0x20, 'u' as u32),
        "i" => (0x22, 'i' as u32),
        "p" => (0x23, 'p' as u32),
        "l" => (0x25, 'l' as u32),
        "j" => (0x26, 'j' as u32),
        "k" => (0x28, 'k' as u32),
        "n" => (0x2D, 'n' as u32),
        "m" => (0x2E, 'm' as u32),
        // Numbers
        "1" | "!" => (0x12, '1' as u32),
        "2" | "@" => (0x13, '2' as u32),
        "3" | "#" => (0x14, '3' as u32),
        "4" | "$" => (0x15, '4' as u32),
        "5" | "%" => (0x17, '5' as u32),
        "6" | "^" => (0x16, '6' as u32),
        "7" | "&" => (0x1A, '7' as u32),
        "8" | "*" => (0x1C, '8' as u32),
        "9" | "(" => (0x19, '9' as u32),
        "0" | ")" => (0x1D, '0' as u32),
        // Punctuation
        "-" | "_" => (0x1B, '-' as u32),
        "=" | "+" => (0x18, '=' as u32),
        "[" | "{" => (0x21, '[' as u32),
        "]" | "}" => (0x1E, ']' as u32),
        "\\" | "|" => (0x2A, '\\' as u32),
        ";" | ":" => (0x29, ';' as u32),
        "'" | "\"" => (0x27, '\'' as u32),
        "`" | "~" => (0x32, '`' as u32),
        "," | "<" => (0x2B, ',' as u32),
        "." | ">" => (0x2F, '.' as u32),
        "/" | "?" => (0x2C, '/' as u32),
        // Special keys
        "enter" | "return" => (0x24, 0),
        "tab" => (0x30, '\t' as u32),
        "space" => (0x31, ' ' as u32),
        "backspace" => (0x33, 0),
        "escape" => (0x35, 0),
        "delete" => (0x75, 0),
        "home" => (0x73, 0),
        "end" => (0x77, 0),
        "pageup" => (0x74, 0),
        "pagedown" => (0x79, 0),
        "up" => (0x7E, 0),
        "down" => (0x7D, 0),
        "left" => (0x7B, 0),
        "right" => (0x7C, 0),
        // Function keys
        "f1" => (0x7A, 0),
        "f2" => (0x78, 0),
        "f3" => (0x63, 0),
        "f4" => (0x76, 0),
        "f5" => (0x60, 0),
        "f6" => (0x61, 0),
        "f7" => (0x62, 0),
        "f8" => (0x64, 0),
        "f9" => (0x65, 0),
        "f10" => (0x6D, 0),
        "f11" => (0x67, 0),
        "f12" => (0x6F, 0),
        _ => return None,
    })
}

impl Drop for GhosttyView {
    fn drop(&mut self) {
        self.host_view = None;
        #[cfg(target_os = "macos")]
        {
            self.document_view = None;
            self.nsview = None;
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
        let input_focus = focus.clone();
        let entity = cx.entity().downgrade();
        let show_layout_fallback = self.show_layout_fallback();
        let layout_fallback_bg = self
            .app
            .background_rgb()
            .map(|rgb| {
                Rgba {
                    r: f32::from(rgb[0]) / 255.0,
                    g: f32::from(rgb[1]) / 255.0,
                    b: f32::from(rgb[2]) / 255.0,
                    a: 1.0,
                }
                .into()
            })
            .unwrap_or_else(|| cx.theme().background);

        div()
            .size_full()
            .map(|div| {
                if show_layout_fallback {
                    div.bg(layout_fallback_bg)
                } else {
                    div
                }
            })
            .key_context("GhosttyTerminal")
            .track_focus(&focus)
            // Consume Tab/Shift-Tab so Root's focus cycling doesn't intercept them.
            // The actual Tab key is forwarded to ghostty via on_key_down below.
            .on_action(cx.listener(|this, _: &ConsumeTab, window, _cx| {
                if !this.focus_handle.is_focused(window) {
                    return;
                }
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
            .on_action(cx.listener(|this, _: &ConsumeTabPrev, window, _cx| {
                if !this.focus_handle.is_focused(window) {
                    return;
                }
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
                if this.paste_from_clipboard(cx) {
                    cx.notify();
                }
            }))
            .drag_over::<ExternalPaths>(|style, _, _, _| style)
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                let Some(payload) = payload_from_external_paths(paths) else {
                    return;
                };
                window.focus(&this.focus_handle, cx);
                if this.send_terminal_paste_payload(payload) {
                    cx.emit(GhosttyFocusChanged);
                    cx.notify();
                }
            }))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.focus(&focus, cx);
                    this.last_mouse_position = Some(event.position);
                    if let Some(ref terminal) = this.terminal {
                        let (x, y) = this.view_local_pos(event.position);
                        let mods = gpui_mods_to_ghostty(&event.modifiers);
                        terminal.send_mouse_pos(x, y, mods);
                        terminal.send_mouse_button(true, MouseButton::Left, mods);
                    }
                    cx.emit(GhosttyFocusChanged);
                    cx.notify();
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                    this.last_mouse_position = Some(event.position);
                    if let Some(ref terminal) = this.terminal {
                        let (x, y) = this.view_local_pos(event.position);
                        let mods = gpui_mods_to_ghostty(&event.modifiers);
                        terminal.send_mouse_pos(x, y, mods);
                        terminal.send_mouse_button(false, MouseButton::Left, mods);
                    }
                    let changed = this.drain_surface_state(true, cx);
                    if changed {
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, _cx| {
                this.last_mouse_position = Some(event.position);
                if let Some(ref terminal) = this.terminal {
                    let (x, y) = this.view_local_pos(event.position);
                    terminal.send_mouse_pos(x, y, gpui_mods_to_ghostty(&event.modifiers));
                }
            }))
            .on_modifiers_changed(cx.listener(
                |this, event: &ModifiersChangedEvent, _window, _cx| {
                    this.refresh_mouse_modifiers(&event.modifiers);
                },
            ))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, _cx| {
                this.last_mouse_position = Some(event.position);
                if let Some(ref terminal) = this.terminal {
                    let delta = match event.delta {
                        ScrollDelta::Lines(d) => (f64::from(d.x), f64::from(d.y)),
                        ScrollDelta::Pixels(d) => {
                            // Match Ghostty's AppKit host: precise trackpad
                            // deltas are sent as-is with a subjective 2x
                            // multiplier and the ScrollMods precision bit.
                            // Ghostty core then accumulates sub-row remainders.
                            (f64::from(d.x) * 2.0, f64::from(d.y) * 2.0)
                        }
                    };
                    terminal.send_mouse_scroll(
                        delta.0,
                        delta.1,
                        gpui_scroll_mods_to_ghostty(&event.delta),
                    );
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if !this.focus_handle.is_focused(window) {
                    return;
                }
                if this.handle_key_down(event, window, cx) {
                    window.prevent_default();
                    cx.stop_propagation();
                }
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
                                view.on_layout(bounds, window, _cx);
                            });
                        }
                    },
                    {
                        let focus = input_focus.clone();
                        let entity = entity.clone();
                        move |_bounds, _state, window, cx| {
                            window.handle_input(
                                &focus,
                                GhosttyInputHandler {
                                    view: entity.clone(),
                                },
                                cx,
                            );
                        }
                    },
                )
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::should_send_ime_insert_as_key_event;

    #[test]
    fn direct_ascii_ime_commits_use_key_event_path() {
        assert!(should_send_ime_insert_as_key_event("abc"));
        assert!(should_send_ime_insert_as_key_event("A quick test."));
        assert!(!should_send_ime_insert_as_key_event(""));
        assert!(!should_send_ime_insert_as_key_event("hello\n"));
        assert!(!should_send_ime_insert_as_key_event("你好"));
    }
}
