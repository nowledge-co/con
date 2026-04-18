//! Windows terminal view — drives the `con-ghostty` Windows backend
//! (`WindowsGhosttyApp` + `WindowsGhosttyTerminal` + the `HostView`
//! that owns a child `WS_CHILD` HWND parented to GPUI's main window).
//!
//! Same public type names as the macOS `ghostty_view` so the rest of
//! `con-app` (terminal_pane.rs, workspace.rs) compiles unchanged. The
//! `#[path]` selector in `main.rs` picks this file on Windows.
//!
//! Lifecycle:
//!
//! 1. `GhosttyView::new(app, cwd, font_size, cx)` — pre-allocates a
//!    `WindowsGhosttyTerminal` so `terminal_pane` can hold an Arc to
//!    it. The `HostView` (the actual HWND + renderer + ConPTY +
//!    parser) is constructed lazily in [`GhosttyView::ensure_host`]
//!    once we have the parent HWND from GPUI.
//! 2. The geometry of the WS_CHILD HWND is positioned by
//!    [`GhosttyView::reposition_host`], called from
//!    `on_children_prepainted` so the host follows GPUI's layout.
//! 3. Drop releases the `HostView`, destroying the HWND and ending
//!    the child shell.

use std::sync::Arc;

use con_ghostty::{GhosttyApp, GhosttySplitDirection, GhosttyTerminal};
use gpui::*;
use gpui_component::ActiveTheme;

actions!(ghostty, [ConsumeTab, ConsumeTabPrev]);

#[allow(dead_code)]
pub struct GhosttyTitleChanged(pub Option<String>);
pub struct GhosttyProcessExited;
pub struct GhosttyFocusChanged;
pub struct GhosttySplitRequested(pub GhosttySplitDirection);

impl EventEmitter<GhosttyTitleChanged> for GhosttyView {}
impl EventEmitter<GhosttyProcessExited> for GhosttyView {}
impl EventEmitter<GhosttyFocusChanged> for GhosttyView {}
impl EventEmitter<GhosttySplitRequested> for GhosttyView {}

pub struct GhosttyView {
    app: Arc<GhosttyApp>,
    terminal: Option<Arc<GhosttyTerminal>>,
    focus_handle: FocusHandle,
    initial_cwd: Option<String>,
    initial_font_size: f32,
    initialized: bool,
    /// Set true after a HostView::new failure to suppress retry on
    /// every layout pass — the same DXGI / D3D errors fire ~60×/s
    /// otherwise. The user has to explicitly recreate the pane to
    /// clear this; richer recovery (delayed retry, rebuild on resize)
    /// can come later.
    init_failed: bool,
}

pub fn init(_cx: &mut App) {
    // Tab/Shift-Tab interception is handled by the host_view's
    // WM_KEYDOWN handler; no GPUI key bindings needed for the terminal
    // itself (all keys flow into the WS_CHILD HWND directly while the
    // child is focused).
}

impl GhosttyView {
    pub fn new(
        app: Arc<GhosttyApp>,
        cwd: Option<String>,
        font_size: f32,
        cx: &mut Context<Self>,
    ) -> Self {
        // Pre-allocate the GhosttyTerminal so terminal_pane can hold an
        // Arc to it before the HostView is wired up. On macOS the
        // Terminal is created at the same point — see ghostty_view.rs.
        let terminal = Arc::new(GhosttyTerminal::new());

        Self {
            app,
            terminal: Some(terminal),
            focus_handle: cx.focus_handle(),
            initial_cwd: cwd,
            initial_font_size: font_size,
            initialized: false,
            init_failed: false,
        }
    }

    pub fn terminal(&self) -> Option<&Arc<GhosttyTerminal>> {
        self.terminal.as_ref()
    }

    pub fn write_or_queue(&mut self, data: &[u8]) {
        if let Some(terminal) = &self.terminal {
            terminal.write_to_pty(data);
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
        self.initialized
    }

    #[allow(dead_code)]
    pub fn selection_text(&self) -> Option<String> {
        self.terminal.as_ref().and_then(|t| t.selection_text())
    }

    pub fn shutdown_surface(&mut self) {
        if let Some(terminal) = &self.terminal {
            terminal.request_close();
        }
        self.initialized = false;
    }

    pub fn set_surface_focus_state(&mut self, focused: bool) {
        if let Some(terminal) = &self.terminal {
            terminal.set_focus(focused);
        }
    }

    pub fn ensure_initialized_for_control(&mut self, _window: &mut Window) {
        // Initialization happens lazily inside `Render::render` once we
        // have both a parent HWND and bounds. Marking initialized here
        // would lie about the HostView's existence.
    }

    pub fn set_visible(&self, _visible: bool) {
        // The WS_CHILD HWND inherits visibility from its parent; an
        // explicit ShowWindow(SW_HIDE) is a future optimization for
        // occluded panes.
    }

    pub fn sync_window_background_blur(&self) {
        // No-op on Windows; the HWND's swapchain composes against
        // whatever DWM puts behind us.
    }

    /// Bring up the HostView (HWND + renderer + ConPTY) at the given
    /// physical-pixel rectangle inside the parent HWND. Idempotent.
    fn ensure_host(&mut self, parent_hwnd: isize, rect_px: (i32, i32, i32, i32)) {
        use windows::Win32::Foundation::{HWND, RECT};

        if self.initialized || self.init_failed {
            return;
        }

        let parent = HWND(parent_hwnd as *mut _);
        let rect = RECT {
            left: rect_px.0,
            top: rect_px.1,
            right: rect_px.2,
            bottom: rect_px.3,
        };

        let renderer_config = self.app.renderer_config();

        match con_ghostty::windows::host_view::HostView::new(parent, rect, renderer_config) {
            Ok(host) => {
                if let Some(terminal) = &self.terminal {
                    terminal.attach(host);
                }
                self.initialized = true;
                let _ = (&self.initial_cwd, self.initial_font_size); // TODO(cwd, font_size)
            }
            Err(err) => {
                log::error!("HostView::new failed: {:#}", err);
                // Latch so we don't re-attempt on every layout. Trade-
                // off: the user has to recreate the pane to retry.
                self.init_failed = true;
            }
        }
    }

    fn reposition_host(&self, rect_px: (i32, i32, i32, i32)) {
        use windows::Win32::Foundation::RECT;

        let rect = RECT {
            left: rect_px.0,
            top: rect_px.1,
            right: rect_px.2,
            bottom: rect_px.3,
        };
        if let Some(terminal) = &self.terminal {
            let inner = terminal.inner();
            if let Some(host) = inner.lock().as_ref() {
                host.reposition(rect);
            }
        }
    }

    /// Trigger one render pass now. GPUI's paint pipeline drives this
    /// rather than waiting for Windows-side `WM_PAINT`, which never
    /// fires on demand for a child HWND whose class has no auto-paint
    /// behavior.
    fn paint_host(&self) {
        if let Some(terminal) = &self.terminal {
            let inner = terminal.inner();
            if let Some(host) = inner.lock().as_ref() {
                host.paint_frame();
            }
        }
    }

    /// Translate a GPUI `KeyDownEvent` into bytes and forward to the
    /// ConPTY via `terminal.send_text`. Returns `true` if the key was
    /// handled (so GPUI can stop propagation).
    ///
    /// GPUI owns keyboard focus on Windows — our `WS_CHILD` HWND never
    /// receives `WM_CHAR` because it isn't in the focus chain. So we
    /// translate at this layer into the byte sequences a terminal
    /// emulator expects. Mapping matches Windows Terminal / Ghostty:
    ///   - printable → key_char bytes
    ///   - Enter → `\r` (ConPTY's line-editor expects CR)
    ///   - Backspace → `\x7f` (DEL — the xterm / VT220 convention modern shells want)
    ///   - Tab → `\t`
    ///   - Esc → `\x1b`
    ///   - Arrows / Home / End / Page / Delete → xterm CSI sequences
    ///   - Ctrl-<letter> → control character (Ctrl-C → 0x03, etc.)
    fn handle_key_down(&self, event: &KeyDownEvent) -> bool {
        let Some(terminal) = self.terminal.as_ref() else {
            return false;
        };
        let keystroke = &event.keystroke;

        // Named keys first. Arrow / nav sequences are the "application
        // cursor keys off" variants (ESC [ letter) — most modern shells
        // don't enable DECCKM so this is the safe default.
        let named: Option<&str> = match keystroke.key.as_str() {
            "enter" | "return" => Some("\r"),
            "backspace" => Some("\x7f"),
            "tab" => Some("\t"),
            "escape" => Some("\x1b"),
            "up" => Some("\x1b[A"),
            "down" => Some("\x1b[B"),
            "right" => Some("\x1b[C"),
            "left" => Some("\x1b[D"),
            "home" => Some("\x1b[H"),
            "end" => Some("\x1b[F"),
            "pageup" => Some("\x1b[5~"),
            "pagedown" => Some("\x1b[6~"),
            "delete" => Some("\x1b[3~"),
            _ => None,
        };
        if let Some(s) = named {
            terminal.send_text(s);
            return true;
        }

        // Ctrl + ascii letter → control char (Ctrl-C = 0x03 etc.)
        if keystroke.modifiers.control
            && !keystroke.modifiers.alt
            && !keystroke.modifiers.platform
            && keystroke.key.len() == 1
        {
            if let Some(ch) = keystroke.key.chars().next() {
                if ch.is_ascii_alphabetic() {
                    let code = ch.to_ascii_uppercase() as u8 - b'@';
                    let byte = [code];
                    if let Ok(s) = std::str::from_utf8(&byte) {
                        terminal.send_text(s);
                        return true;
                    }
                }
            }
        }

        // Plain printable: GPUI fills `key_char` with the rendered
        // character (respects shift / layout). Falls back to `key` for
        // single-char key names when `key_char` is absent.
        if let Some(ch) = keystroke.key_char.as_deref() {
            if !ch.is_empty() {
                terminal.send_text(ch);
                return true;
            }
        }
        if keystroke.key.len() == 1 {
            terminal.send_text(&keystroke.key);
            return true;
        }

        false
    }
}

impl Focusable for GhosttyView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for GhosttyView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = cx.entity().downgrade();

        // The HostView geometry follows our placement in GPUI's layout.
        // We capture our paint bounds via on_children_prepainted on a
        // single full-size child and translate them to the parent
        // HWND's coordinate space.
        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if !this.focus_handle.is_focused(window) {
                    return;
                }
                if this.handle_key_down(event) {
                    window.prevent_default();
                    cx.stop_propagation();
                }
            }))
            // Transparent so the HWND swapchain (sibling-composed by DWM)
            // shows through wherever this element paints.
            .bg(theme.background.opacity(0.0))
            .child(
                div()
                    .size_full()
                    .on_children_prepainted(move |bounds_list: Vec<Bounds<Pixels>>, window, cx| {
                        let Some(bounds) = bounds_list.first().copied() else {
                            return;
                        };
                        let scale = window.scale_factor();
                        let left = (f32::from(bounds.origin.x) * scale) as i32;
                        let top = (f32::from(bounds.origin.y) * scale) as i32;
                        let right =
                            ((f32::from(bounds.origin.x) + f32::from(bounds.size.width)) * scale) as i32;
                        let bottom = ((f32::from(bounds.origin.y) + f32::from(bounds.size.height))
                            * scale) as i32;

                        log::trace!(
                            "pane bounds: logical ({:.1},{:.1}) {:.1}x{:.1} scale={} → physical ({},{})-({},{})",
                            f32::from(bounds.origin.x),
                            f32::from(bounds.origin.y),
                            f32::from(bounds.size.width),
                            f32::from(bounds.size.height),
                            scale,
                            left, top, right, bottom,
                        );

                        let parent_hwnd = parent_hwnd_from_window(window);

                        if let Some(view) = entity.upgrade() {
                            view.update(cx, |view, _cx| {
                                if let Some(parent) = parent_hwnd {
                                    view.ensure_host(parent, (left, top, right, bottom));
                                }
                                view.reposition_host((left, top, right, bottom));
                                view.paint_host();
                            });
                        }
                    })
                    // A 1x1 placeholder child so on_children_prepainted
                    // actually fires; layout flex makes it expand.
                    .child(div().size_full()),
            )
    }
}

impl Drop for GhosttyView {
    fn drop(&mut self) {
        if let Some(terminal) = &self.terminal {
            terminal.request_close();
        }
    }
}

/// Look up the parent HWND from GPUI's window via `raw_window_handle`.
/// Returns the HWND as `isize` (raw pointer cast) so the caller doesn't
/// need to import the `windows` crate.
fn parent_hwnd_from_window(window: &mut Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = HasWindowHandle::window_handle(window).ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}
