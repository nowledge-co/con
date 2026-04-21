//! Windows terminal view — drives the `con-ghostty` Windows backend
//! (`WindowsGhosttyApp` + `WindowsGhosttyTerminal` + a `RenderSession`
//! that owns the renderer, VT parser, and ConPTY for one pane).
//!
//! Same public type names as the macOS `ghostty_view` so the rest of
//! `con-app` (terminal_pane.rs, workspace.rs) compiles unchanged. The
//! `#[path]` selector in `main.rs` picks this file on Windows.
//!
//! Paint model:
//! - No child HWND. The renderer draws into an offscreen D3D11 texture
//!   and hands back BGRA bytes each dirty frame.
//! - We wrap those bytes into an `Arc<RenderImage>` and paint them via a
//!   GPUI `img(ImageSource::Render(...))` element. The terminal pane
//!   lives inside GPUI's DirectComposition tree so modals (settings,
//!   command palette) and newly-opened panes compose correctly — no
//!   z-order flashes, no "modal is 100% transparent over the pane".
//!
//! Lifecycle:
//!
//! 1. `GhosttyView::new(app, cwd, font_size, cx)` pre-allocates a
//!    `WindowsGhosttyTerminal` so `terminal_pane` can hold an Arc to
//!    it. No renderer/ConPTY yet — those are built lazily.
//! 2. `on_children_prepainted` captures the pane's bounds the first
//!    time they're known. At that point we spin up a `RenderSession`
//!    (Renderer + VT + ConPTY) sized to those physical pixels.
//! 3. Each subsequent prepaint: resize on geometry change, update DPI
//!    on scale-factor change, pump one `render_frame()`. When the
//!    frame is fresh we rebuild `cached_image` and `cx.notify()` so
//!    the next `render()` picks it up.
//! 4. Drop releases the `RenderSession` and ends the child shell.

use std::sync::Arc;

use con_ghostty::{GhosttyApp, GhosttySplitDirection, GhosttyTerminal};
use gpui::*;
use gpui_component::ActiveTheme;
use image::{Frame, RgbaImage};
use smallvec::SmallVec;

use con_ghostty::windows::host_view::RenderSession;
use con_ghostty::windows::render::RenderOutcome;

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
    /// Latched after a `RenderSession::new` failure so we don't re-try
    /// on every layout pass (the same DXGI / D3D errors would fire ~60×/s
    /// otherwise). User has to recreate the pane to clear it.
    init_failed: bool,
    /// Emit `GhosttyProcessExited` exactly once on shell death.
    process_exit_emitted: bool,
    /// Pane bounds in logical window pixels, captured during prepaint.
    pane_bounds: Option<Bounds<Pixels>>,
    scale_factor: f32,
    /// Last physical-pixel size we sent to `session.resize`. Avoids
    /// resize churn when the logical bounds round to the same physical
    /// size frame-to-frame.
    last_physical_size: Option<(u32, u32)>,
    /// Last scale factor handed to `session.set_dpi`.
    last_scale_factor: f32,
    /// The most recently rendered frame, wrapped as a GPUI image. Each
    /// `RenderImage` has a unique `ImageId`, so a new Arc replaces the
    /// prior GPU-side upload; the `RenderOutcome::Unchanged` gate keeps
    /// us from allocating one per idle frame.
    cached_image: Option<Arc<RenderImage>>,
    /// The prior `cached_image`, kept live until the next prepaint so
    /// the paint that referenced it has finished. Dropped the frame
    /// after via `Window::drop_image` to evict its sprite-atlas tile.
    /// Without this, every frame would leak ~width×height×4 bytes of
    /// GPU memory.
    image_to_drop: Option<Arc<RenderImage>>,
}

pub fn init(_cx: &mut App) {
    // Keyboard input on Windows flows through GPUI focus → `on_key_down`
    // below (no HWND-level WM_CHAR routing), so no bindings are needed.
}

impl GhosttyView {
    pub fn new(
        app: Arc<GhosttyApp>,
        cwd: Option<String>,
        font_size: f32,
        cx: &mut Context<Self>,
    ) -> Self {
        let terminal = Arc::new(GhosttyTerminal::new());
        Self {
            app,
            terminal: Some(terminal),
            focus_handle: cx.focus_handle(),
            initial_cwd: cwd,
            initial_font_size: font_size,
            initialized: false,
            init_failed: false,
            process_exit_emitted: false,
            pane_bounds: None,
            scale_factor: 1.0,
            last_physical_size: None,
            last_scale_factor: 0.0,
            cached_image: None,
            image_to_drop: None,
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
        // Release our own Arcs. The sprite-atlas tiles will stay until
        // the window closes; no way to reach `Window::drop_image` from
        // here. A per-pane ~2×framebytes residue is acceptable.
        self.cached_image = None;
        self.image_to_drop = None;
        self.last_physical_size = None;
    }

    pub fn set_surface_focus_state(&mut self, focused: bool) {
        if let Some(terminal) = &self.terminal {
            terminal.set_focus(focused);
        }
    }

    pub fn ensure_initialized_for_control(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        // Initialization is lazy inside `ensure_session` once a real
        // layout pass hands us bounds and DPI. Claiming initialized here
        // would lie about the RenderSession's existence.
    }

    /// Cross-platform hide hook used on macOS when switching tabs (each
    /// tab's child NSView is toggled so only the active tab's terminal
    /// paints). On Windows the renderer composites through GPUI's image
    /// path, and inactive tabs simply aren't in the element tree, so
    /// there's nothing to toggle — no-op.
    pub fn set_visible(&self, _visible: bool) {}

    pub fn sync_window_background_blur(&self) {
        // Windows uses DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)
        // at window-creation time; there's nothing per-pane to refresh.
    }

    pub fn drain_surface_state(&mut self, _cx: &mut Context<Self>) -> bool {
        false
    }

    pub fn pump_deferred_work(&mut self, cx: &mut Context<Self>) -> bool {
        // No action-callback channel on Windows (cf. macOS's
        // `wake_generation`). Poll `is_alive` so workspace's
        // `on_terminal_process_exited` runs when the child shell exits.
        if self.initialized
            && !self.process_exit_emitted
            && self
                .terminal
                .as_ref()
                .is_some_and(|t| !t.is_alive())
        {
            self.process_exit_emitted = true;
            cx.emit(GhosttyProcessExited);
            return true;
        }
        false
    }

    fn ensure_session(&mut self, width_px: u32, height_px: u32, dpi: u32) {
        if self.initialized || self.init_failed {
            return;
        }
        if width_px == 0 || height_px == 0 {
            return;
        }

        let mut config = self.app.renderer_config();
        if self.initial_font_size > 0.0 {
            config.font_size_px = self.initial_font_size;
        }
        config.initial_width = width_px;
        config.initial_height = height_px;

        match RenderSession::new(width_px, height_px, dpi, config) {
            Ok(session) => {
                if let Some(terminal) = &self.terminal {
                    terminal.attach(session);
                }
                self.initialized = true;
                self.last_physical_size = Some((width_px, height_px));
                self.last_scale_factor = dpi as f32 / 96.0;
                let _ = &self.initial_cwd; // TODO: honour cwd in ConPTY spawn.
            }
            Err(err) => {
                log::error!("RenderSession::new failed: {:#}", err);
                self.init_failed = true;
            }
        }
    }

    /// Called from the children-prepainted listener. Drives session
    /// lifecycle (init/resize/DPI) and pumps one render. Returns `true`
    /// when a fresh image was produced — the caller uses that to decide
    /// whether to `cx.notify()`.
    fn sync_render(
        &mut self,
        bounds: Bounds<Pixels>,
        scale_factor: f32,
        window: &mut Window,
    ) -> bool {
        // Drop the tile that the PRIOR frame painted. Paint has already
        // flushed for that frame (we're in prepaint for the next one),
        // so its sprite-atlas entry is no longer referenced and we can
        // evict it without corrupting what we're about to paint.
        if let Some(old) = self.image_to_drop.take() {
            let _ = window.drop_image(old);
        }

        self.pane_bounds = Some(bounds);
        self.scale_factor = scale_factor;

        // `.ceil()` matches `Window::paint_image`, which does
        // `map_size(|size| size.ceil())` on the scaled physical quad. If
        // we render at `.round()` our texture ends up 1px smaller than
        // the quad on half-pixel bounds and LINEAR sampling blurs every
        // pixel by a tiny fraction.
        let width_px = ((f32::from(bounds.size.width) * scale_factor).ceil() as u32).max(1);
        let height_px = ((f32::from(bounds.size.height) * scale_factor).ceil() as u32).max(1);
        let dpi = (scale_factor * 96.0).round().max(1.0) as u32;

        self.ensure_session(width_px, height_px, dpi);
        if !self.initialized {
            return false;
        }

        let Some(session_arc) = self.terminal.as_ref().map(|t| t.inner()) else {
            return false;
        };
        let guard = session_arc.lock();
        let Some(session) = guard.as_ref() else {
            return false;
        };

        if (scale_factor - self.last_scale_factor).abs() > f32::EPSILON {
            if let Err(err) = session.set_dpi(dpi) {
                log::warn!("RenderSession::set_dpi failed: {err:#}");
            }
            self.last_scale_factor = scale_factor;
        }

        if self.last_physical_size != Some((width_px, height_px)) {
            if let Err(err) = session.resize(width_px, height_px) {
                log::warn!("RenderSession::resize failed: {err:#}");
            }
            self.last_physical_size = Some((width_px, height_px));
        }

        match session.render_frame() {
            Ok(RenderOutcome::Unchanged) => false,
            Ok(RenderOutcome::Rendered(frame)) => {
                if let Some(image) = bgra_frame_to_image(frame.bytes, frame.width, frame.height) {
                    // Stash the prior image so next prepaint can drop
                    // its atlas tile. `Option::replace` returns the old
                    // inner value, matching our `Option<Arc<_>>` slot.
                    self.image_to_drop = self.cached_image.replace(image);
                    true
                } else {
                    false
                }
            }
            Err(err) => {
                log::warn!("RenderSession::render_frame failed: {err:#}");
                false
            }
        }
    }

    /// Convert a window-coordinate mouse position into a 0-based
    /// (col, row) cell address. Returns `None` when we don't yet have a
    /// session / bounds to project into.
    fn cell_from_event_position(&self, pos: Point<Pixels>) -> Option<(u16, u16)> {
        let bounds = self.pane_bounds?;
        let terminal = self.terminal.as_ref()?;
        let inner = terminal.inner();
        let guard = inner.lock();
        let session = guard.as_ref()?;
        let metrics = session.metrics();
        if metrics.cell_width_px == 0 || metrics.cell_height_px == 0 {
            return None;
        }
        let local_x = (f32::from(pos.x) - f32::from(bounds.origin.x)).max(0.0);
        let local_y = (f32::from(pos.y) - f32::from(bounds.origin.y)).max(0.0);
        let phys_x = (local_x * self.scale_factor) as u32;
        let phys_y = (local_y * self.scale_factor) as u32;
        let col = (phys_x / metrics.cell_width_px.max(1)) as u16;
        let row = (phys_y / metrics.cell_height_px.max(1)) as u16;
        Some((col, row))
    }

    fn forward_mouse_down(&self, pos: Point<Pixels>) {
        if let Some((col, row)) = self.cell_from_event_position(pos) {
            if let Some(terminal) = &self.terminal {
                let inner = terminal.inner();
                if let Some(session) = inner.lock().as_ref() {
                    session.mouse_down(col, row);
                }
            }
        }
    }

    fn forward_mouse_drag(&self, pos: Point<Pixels>) {
        if let Some((col, row)) = self.cell_from_event_position(pos) {
            if let Some(terminal) = &self.terminal {
                let inner = terminal.inner();
                if let Some(session) = inner.lock().as_ref() {
                    session.mouse_drag(col, row);
                }
            }
        }
    }

    fn forward_mouse_up(&self, pos: Point<Pixels>) {
        if let Some((col, row)) = self.cell_from_event_position(pos) {
            if let Some(terminal) = &self.terminal {
                let inner = terminal.inner();
                if let Some(session) = inner.lock().as_ref() {
                    session.mouse_up(col, row);
                }
            }
        }
    }

    fn forward_scroll(&self, pos: Point<Pixels>, delta: ScrollDelta) {
        let Some(terminal) = &self.terminal else {
            return;
        };
        let inner = terminal.inner();
        let guard = inner.lock();
        let Some(session) = guard.as_ref() else {
            return;
        };
        // Only report wheel to the shell when it has explicitly enabled
        // mouse tracking (SGR). Otherwise modern TUIs expect the scroll
        // to move their own scrollback buffer via shell keybinds, not
        // arrive as mouse events.
        if !session.mouse_tracking_active() {
            return;
        }
        let Some((col0, row0)) = self.cell_from_event_position(pos) else {
            return;
        };
        let delta_y_px = match delta {
            ScrollDelta::Pixels(p) => f32::from(p.y),
            ScrollDelta::Lines(p) => {
                let metrics = session.metrics();
                p.y * metrics.cell_height_px.max(1) as f32
            }
        };
        if delta_y_px.abs() < f32::EPSILON {
            return;
        }
        // `forward_wheel` expects 1-based SGR coordinates.
        session.forward_wheel(col0 + 1, row0 + 1, delta_y_px);
    }

    /// Translate a GPUI `KeyDownEvent` into bytes and forward to the
    /// ConPTY. Returns `true` if the key was handled (so GPUI can stop
    /// propagation).
    ///
    /// GPUI owns keyboard focus on Windows — there's no child HWND in
    /// the focus chain — so we translate at this layer into byte
    /// sequences a terminal emulator expects. DECCKM-aware arrows live
    /// alongside the printable / Ctrl-letter paths.
    fn handle_key_down(&self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(terminal) = self.terminal.as_ref() else {
            return false;
        };
        let keystroke = &event.keystroke;

        // Ctrl+Shift+C / Ctrl+Shift+V → clipboard. These must run ahead
        // of the generic Ctrl-letter path below, which would otherwise
        // emit ^C / ^V to the shell.
        if keystroke.modifiers.control
            && keystroke.modifiers.shift
            && !keystroke.modifiers.alt
            && !keystroke.modifiers.platform
        {
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
                    if let Some(text) = cx
                        .read_from_clipboard()
                        .and_then(|item| item.text().map(|s| s.to_string()))
                    {
                        if !text.is_empty() {
                            send_paste(terminal, &text);
                            cx.notify();
                        }
                    }
                    return true;
                }
                _ => {}
            }
        }

        let decckm = {
            let inner = terminal.inner();
            inner.lock().as_ref().is_some_and(|s| s.is_decckm())
        };

        // Named / special keys — arrows, home/end, page nav, insert,
        // delete, F1-F12, tab/shift-tab, enter, backspace, escape. Each
        // honours DECCKM (arrows) and xterm modifier-parameter encoding
        // for Ctrl/Shift/Alt (CSI 1;m<X> for arrows + F1-F4, CSI n;m~
        // for tilde-terminated keys).
        if let Some(bytes) = encode_special_key(&keystroke.key, &keystroke.modifiers, decckm) {
            terminal.send_text(&bytes);
            return true;
        }

        // Ctrl + ascii letter → control char (Ctrl-C = 0x03 etc.)
        if keystroke.modifiers.control
            && !keystroke.modifiers.alt
            && !keystroke.modifiers.platform
            && !keystroke.modifiers.shift
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

        // Alt + printable ASCII → ESC + char (meta-prefix convention
        // readline / vim / tmux / fzf all recognise: Alt+B word-back,
        // Alt+F word-forward, Alt+D word-delete, Alt+. last-arg, …).
        // Only when Alt is the only app-consumable modifier — AltGr-
        // produced characters already arrive decoded via `key_char`
        // without the `alt` flag on most layouts, and Ctrl+Alt is left
        // for the terminal's own modifyOtherKeys semantics if added
        // later.
        if keystroke.modifiers.alt
            && !keystroke.modifiers.control
            && !keystroke.modifiers.platform
        {
            if let Some(ch) = keystroke.key_char.as_deref().filter(|s| !s.is_empty()) {
                let mut out = String::with_capacity(1 + ch.len());
                out.push('\x1b');
                out.push_str(ch);
                terminal.send_text(&out);
                return true;
            }
        }

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

/// Wrap a BGRA readback buffer as a `RenderImage`. `RenderImage`
/// internally stores BGRA already (see `3pp/zed/crates/gpui/src/elements/img.rs`,
/// where the loader swaps RGBA→BGRA on decode), so we feed the D3D11
/// `DXGI_FORMAT_B8G8R8A8_UNORM` readback directly without a swap.
fn bgra_frame_to_image(bytes: Vec<u8>, width: u32, height: u32) -> Option<Arc<RenderImage>> {
    let expected = (width as usize) * (height as usize) * 4;
    if bytes.len() != expected {
        log::warn!(
            "bgra_frame_to_image: byte len {} != expected {} ({}x{})",
            bytes.len(),
            expected,
            width,
            height
        );
        return None;
    }
    let buffer = RgbaImage::from_raw(width, height, bytes)?;
    let frame = Frame::new(buffer);
    let data: SmallVec<[Frame; 1]> = SmallVec::from_buf([frame]);
    Some(Arc::new(RenderImage::new(data)))
}

/// xterm modifier parameter (`m`) — `1 + (shift | alt<<1 | ctrl<<2)`,
/// where `1` itself means "no modifier" (so the encoder omits it).
///
/// Used in CSI `1;m{A|B|C|D|H|F|P|Q|R|S}` and CSI `{n};m~` sequences.
/// Source: xterm's "PC-Style Function Keys" encoding.
fn xterm_modifier_param(modifiers: &Modifiers) -> Option<u8> {
    let mask = u8::from(modifiers.shift)
        | (u8::from(modifiers.alt) << 1)
        | (u8::from(modifiers.control) << 2);
    if mask == 0 { None } else { Some(1 + mask) }
}

/// Translate a GPUI key name + modifiers into the byte sequence the
/// shell / TUI expects. Returns `None` for keys that should flow
/// through the printable-character path (letters, digits, symbols).
///
/// Covers arrows (DECCKM-aware and modifier-aware), home/end, pageup/
/// pagedown, insert/delete, F1-F12, enter, backspace, tab/shift-tab,
/// escape. xterm modifier encoding is applied uniformly: any combination
/// of Shift/Alt/Ctrl shifts the sequence into its CSI `1;m<final>`
/// (arrows, home/end, F1-F4) or CSI `n;m~` (tilde-terminated) form.
fn encode_special_key(key: &str, modifiers: &Modifiers, decckm: bool) -> Option<String> {
    let m = xterm_modifier_param(modifiers);

    let tilde = |code: u8| match m {
        Some(m) => format!("\x1b[{};{}~", code, m),
        None => format!("\x1b[{}~", code),
    };

    // CSI-1-final covers arrows + home/end + F1-F4. For the no-modifier
    // case: arrows honour DECCKM, F1-F4 use SS3 (ESC O x), home/end use
    // plain CSI.
    let csi1 = |final_byte: char, ss3_when_plain: bool, decckm_arrow: bool| match m {
        Some(m) => format!("\x1b[1;{}{}", m, final_byte),
        None if decckm_arrow && decckm => format!("\x1bO{}", final_byte),
        None if ss3_when_plain => format!("\x1bO{}", final_byte),
        None => format!("\x1b[{}", final_byte),
    };

    Some(match key {
        "up" => csi1('A', false, true),
        "down" => csi1('B', false, true),
        "right" => csi1('C', false, true),
        "left" => csi1('D', false, true),
        "home" => csi1('H', false, false),
        "end" => csi1('F', false, false),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "f1" => csi1('P', true, false),
        "f2" => csi1('Q', true, false),
        "f3" => csi1('R', true, false),
        "f4" => csi1('S', true, false),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        "enter" | "return" => "\r".into(),
        "escape" => "\x1b".into(),
        // Alt+Backspace is readline's word-delete (ESC + DEL).
        "backspace" if modifiers.alt && !modifiers.control && !modifiers.platform => {
            "\x1b\x7f".into()
        }
        "backspace" => "\x7f".into(),
        // Shift+Tab is the xterm "back-tab" CSI Z — bash/zsh completion
        // menus and fzf use it to cycle backwards.
        "tab" if modifiers.shift && !modifiers.control && !modifiers.platform => {
            "\x1b[Z".into()
        }
        "tab" => "\t".into(),
        _ => return None,
    })
}

fn send_paste(terminal: &GhosttyTerminal, text: &str) {
    // Bracketed paste lets the shell tell pasted text apart from real
    // typing, so vim / readline can disable auto-indent. Hold the lock
    // across both wrapper writes so the session can't be swapped out
    // mid-paste (which would strip the trailing `ESC[201~`).
    let inner = terminal.inner();
    let guard = inner.lock();
    if let Some(session) = guard.as_ref() {
        if session.is_bracketed_paste() {
            session.write_pty_raw(b"\x1b[200~");
            session.write_input(text);
            session.write_pty_raw(b"\x1b[201~");
        } else {
            session.write_input(text);
        }
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
        // `ObjectFit::Fill` keeps the image quad exactly equal to the
        // element's bounds. The default `Contain` applies aspect-ratio
        // letterboxing using float math, which produces a quad a fraction
        // of a pixel smaller than our source texture — the LINEAR sprite
        // sampler then blends neighbouring texels and every terminal cell
        // shows faint speckles below the glyph baseline.
        let image_child = self
            .cached_image
            .clone()
            .map(|img_arc| {
                img(ImageSource::Render(img_arc))
                    .size_full()
                    .object_fit(ObjectFit::Fill)
            });

        let focus = self.focus_handle.clone();

        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .bg(theme.background.opacity(0.0))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if !this.focus_handle.is_focused(window) {
                    return;
                }
                if this.handle_key_down(event, cx) {
                    window.prevent_default();
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.focus(&focus, cx);
                    this.forward_mouse_down(event.position);
                    cx.emit(GhosttyFocusChanged);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    this.forward_mouse_drag(event.position);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                    this.forward_mouse_up(event.position);
                    cx.notify();
                }),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, cx| {
                this.forward_scroll(event.position, event.delta);
                cx.notify();
            }))
            .child(
                div()
                    .size_full()
                    .on_children_prepainted(
                        move |bounds_list: Vec<Bounds<Pixels>>, window, cx| {
                            let Some(bounds) = bounds_list.first().copied() else {
                                return;
                            };
                            let scale = window.scale_factor();
                            if let Some(view) = entity.upgrade() {
                                view.update(cx, |view, cx| {
                                    let changed = view.sync_render(bounds, scale, window);
                                    if changed {
                                        cx.notify();
                                    }
                                });
                            }
                        },
                    )
                    .children(image_child)
                    // A 1×1 placeholder so `on_children_prepainted` always
                    // fires with at least one bounds entry; flex growth
                    // makes it expand to the full pane.
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

