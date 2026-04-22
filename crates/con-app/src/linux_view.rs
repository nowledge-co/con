//! Linux terminal view backed by con's local Unix PTY + VT scaffold.
//!
//! This still is not the final Linux grid renderer, but it now renders
//! from parsed VT screen state instead of a lossy transcript. That keeps
//! shell bring-up and prompt rendering aligned with the real terminal
//! semantics while the full styled cell renderer is still pending.

use std::sync::Arc;

use con_ghostty::{GhosttyApp, GhosttySplitDirection, GhosttyTerminal, SurfaceSize};
use gpui::*;
use gpui_component::ActiveTheme;

const DEFAULT_FONT_SIZE: f32 = 14.0;
const DEFAULT_CELL_WIDTH_RATIO: f32 = 0.62;
const DEFAULT_CELL_HEIGHT_RATIO: f32 = 1.45;
const MAX_RENDER_LINES: usize = 256;

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
    process_exit_emitted: bool,
    last_title: Option<String>,
    pending_write: Option<Vec<u8>>,
    screen_lines: Vec<String>,
    pane_bounds: Option<Bounds<Pixels>>,
    scale_factor: f32,
    last_surface_size: Option<(u32, u32, u16, u16)>,
}

pub fn init(_cx: &mut App) {}

impl GhosttyView {
    pub fn new(
        app: Arc<GhosttyApp>,
        cwd: Option<String>,
        font_size: f32,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            app,
            terminal: Some(Arc::new(GhosttyTerminal::new())),
            focus_handle: cx.focus_handle(),
            initial_cwd: cwd,
            initial_font_size: font_size,
            initialized: false,
            process_exit_emitted: false,
            last_title: None,
            pending_write: None,
            screen_lines: Vec::new(),
            pane_bounds: None,
            scale_factor: 1.0,
            last_surface_size: None,
        }
    }

    pub fn terminal(&self) -> Option<&Arc<GhosttyTerminal>> {
        self.terminal.as_ref()
    }

    pub fn write_or_queue(&mut self, data: &[u8]) {
        if let Some(terminal) = &self.terminal {
            if self.initialized && terminal.is_attached() {
                terminal.write_to_pty(data);
                return;
            }
        }

        self.pending_write
            .get_or_insert_with(Vec::new)
            .extend_from_slice(data);
    }

    pub fn title(&self) -> Option<String> {
        self.terminal.as_ref().and_then(|terminal| terminal.title())
    }

    pub fn current_dir(&self) -> Option<String> {
        self.terminal
            .as_ref()
            .and_then(|terminal| terminal.current_dir())
            .or_else(|| self.initial_cwd.clone())
    }

    pub fn is_alive(&self) -> bool {
        self.terminal
            .as_ref()
            .is_some_and(|terminal| terminal.is_alive())
    }

    pub fn surface_ready(&self) -> bool {
        self.initialized
    }

    #[allow(dead_code)]
    pub fn selection_text(&self) -> Option<String> {
        None
    }

    pub fn shutdown_surface(&mut self) {
        if let Some(terminal) = &self.terminal {
            terminal.request_close();
        }
        self.initialized = false;
        self.process_exit_emitted = false;
        self.last_title = None;
        self.pending_write = None;
        self.screen_lines.clear();
        self.last_surface_size = None;
    }

    pub fn set_surface_focus_state(&mut self, focused: bool) {
        if let Some(terminal) = &self.terminal {
            terminal.set_focus(focused);
        }
    }

    pub fn ensure_initialized_for_control(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = self.ensure_session(cx);
        if let Some(bounds) = self.pane_bounds {
            let _ = self.sync_surface_size(bounds, window.scale_factor());
        }
    }

    pub fn set_visible(&self, _visible: bool) {}

    pub fn sync_window_background_blur(&self) {}

    pub fn drain_surface_state(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = self.ensure_session(cx);

        let Some(terminal) = self.terminal.as_ref().cloned() else {
            return changed;
        };

        if terminal.take_needs_render() {
            changed |= self.refresh_screen_cache();
        }

        let title = terminal.title();
        if title != self.last_title {
            self.last_title = title.clone();
            changed = true;
            cx.emit(GhosttyTitleChanged(title));
        }

        if !terminal.is_alive() && !self.process_exit_emitted {
            self.process_exit_emitted = true;
            changed = true;
            cx.emit(GhosttyProcessExited);
        }

        if changed {
            cx.notify();
        }

        changed
    }

    pub fn pump_deferred_work(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = self.ensure_session(cx);

        if let Some(terminal) = self.terminal.as_ref().cloned() {
            if terminal.take_needs_render() {
                changed |= self.refresh_screen_cache();
            } else if self.initialized && terminal.is_alive() {
                changed |= self.refresh_screen_cache();
            }

            let title = terminal.title();
            if title != self.last_title {
                self.last_title = title.clone();
                changed = true;
                cx.emit(GhosttyTitleChanged(title));
            }

            if !terminal.is_alive() && !self.process_exit_emitted {
                self.process_exit_emitted = true;
                changed = true;
                cx.emit(GhosttyProcessExited);
            }
        }

        if changed {
            cx.notify();
        }

        changed
    }

    fn ensure_session(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(terminal) = self.terminal.as_ref() else {
            return false;
        };

        if terminal.is_attached() {
            self.initialized = true;
            return false;
        }

        let options = self.app.default_pty_options(self.initial_cwd.as_deref());
        match terminal.spawn_with_options(options) {
            Ok(()) => {
                self.initialized = true;
                self.process_exit_emitted = false;
                if let Some(pending) = self.pending_write.take() {
                    terminal.write_to_pty(&pending);
                }
                self.last_title = terminal.title();
                let _ = self.refresh_screen_cache();
                cx.notify();
                true
            }
            Err(err) => {
                log::error!("failed to start linux shell: {err}");
                false
            }
        }
    }

    fn refresh_screen_cache(&mut self) -> bool {
        let Some(terminal) = self.terminal.as_ref() else {
            return false;
        };
        let lines = terminal.read_screen_text(MAX_RENDER_LINES);
        if !lines.is_empty() {
            log::info!(
                "linux view screen cache lines={} last={:?}",
                lines.len(),
                lines.last()
            );
        }
        if lines == self.screen_lines {
            return false;
        }
        self.screen_lines = lines;
        true
    }

    fn sync_surface_size(&mut self, bounds: Bounds<Pixels>, scale_factor: f32) -> bool {
        self.pane_bounds = Some(bounds);
        self.scale_factor = scale_factor;

        let Some(terminal) = self.terminal.as_ref() else {
            return false;
        };

        let size = self.estimate_surface_size(bounds, scale_factor);
        let signature = (size.width_px, size.height_px, size.columns, size.rows);
        if self.last_surface_size == Some(signature) {
            return false;
        }

        terminal.resize_surface(size);
        self.last_surface_size = Some(signature);
        false
    }

    fn estimate_surface_size(&self, bounds: Bounds<Pixels>, scale_factor: f32) -> SurfaceSize {
        let width_px = ((f32::from(bounds.size.width) * scale_factor).ceil() as u32).max(1);
        let height_px = ((f32::from(bounds.size.height) * scale_factor).ceil() as u32).max(1);

        // Until the real grid renderer lands we estimate the PTY grid
        // from the configured mono font size so shells and TUIs do not
        // stay stuck at the initial 80x24 forever.
        let logical_font_size = if self.initial_font_size > 0.0 {
            self.initial_font_size
        } else {
            DEFAULT_FONT_SIZE
        };
        let font_size_px = logical_font_size * scale_factor;
        let cell_width_px = (font_size_px * DEFAULT_CELL_WIDTH_RATIO).round().max(7.0) as u32;
        let cell_height_px = (font_size_px * DEFAULT_CELL_HEIGHT_RATIO).round().max(14.0) as u32;
        let columns = (width_px / cell_width_px.max(1))
            .max(1)
            .min(u32::from(u16::MAX)) as u16;
        let rows = (height_px / cell_height_px.max(1))
            .max(1)
            .min(u32::from(u16::MAX)) as u16;

        SurfaceSize {
            columns,
            rows,
            width_px,
            height_px,
            cell_width_px,
            cell_height_px,
        }
    }

    fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(terminal) = self.terminal.as_ref() else {
            return false;
        };

        let keystroke = &event.keystroke;
        if keystroke.modifiers.platform {
            return false;
        }

        if keystroke.modifiers.control
            && keystroke.modifiers.shift
            && !keystroke.modifiers.alt
            && keystroke.key == "v"
        {
            if let Some(text) = cx
                .read_from_clipboard()
                .and_then(|item| item.text().map(|s| s.to_string()))
                .filter(|text| !text.is_empty())
            {
                if terminal.is_bracketed_paste() {
                    let mut wrapped = String::with_capacity(text.len() + 12);
                    wrapped.push_str("\x1b[200~");
                    wrapped.push_str(&text);
                    wrapped.push_str("\x1b[201~");
                    terminal.send_text(&wrapped);
                } else {
                    terminal.send_text(&text);
                }
                return true;
            }
        }

        let decckm = terminal.is_decckm();
        if let Some(bytes) = encode_special_key(&keystroke.key, &keystroke.modifiers, decckm) {
            terminal.send_text(&bytes);
            return true;
        }

        if keystroke.modifiers.control
            && !keystroke.modifiers.alt
            && !keystroke.modifiers.shift
            && keystroke.key.len() == 1
        {
            if let Some(ch) = keystroke.key.chars().next() {
                if ch.is_ascii_alphabetic() {
                    let control = [ch.to_ascii_uppercase() as u8 - b'@'];
                    if let Ok(text) = std::str::from_utf8(&control) {
                        terminal.send_text(text);
                        return true;
                    }
                }
            }
        }

        if keystroke.modifiers.alt && !keystroke.modifiers.control && !keystroke.modifiers.shift {
            if let Some(ch) = keystroke.key_char.as_deref().filter(|ch| !ch.is_empty()) {
                let mut out = String::with_capacity(1 + ch.len());
                out.push('\x1b');
                out.push_str(ch);
                terminal.send_text(&out);
                return true;
            }
        }

        if let Some(text) = keystroke
            .key_char
            .as_deref()
            .filter(|text| !text.is_empty())
        {
            terminal.send_text(text);
            return true;
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
        let focus = self.focus_handle.clone();
        let entity = cx.entity().downgrade();
        let line_height = px((self.initial_font_size.max(12.0) * 1.45).round());
        let screen_lines = self.screen_lines.clone();
        let screen_text = if screen_lines.is_empty() {
            None
        } else {
            Some(
                screen_lines
                    .iter()
                    .map(|line| render_terminal_line(line))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        };

        let status_line = if !self.initialized {
            Some(("Launching Linux shell…", theme.foreground.opacity(0.55)))
        } else if !self.is_alive() {
            Some(("Linux shell exited", theme.foreground.opacity(0.55)))
        } else if screen_lines.is_empty() {
            Some(("Waiting for shell prompt…", theme.foreground.opacity(0.45)))
        } else {
            None
        };

        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .bg(theme.background)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if !this.focus_handle.is_focused(window) {
                    return;
                }
                let _ = this.ensure_session(cx);
                if this.handle_key_down(event, cx) {
                    window.prevent_default();
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                    window.focus(&focus, cx);
                    let _ = this.ensure_session(cx);
                    cx.emit(GhosttyFocusChanged);
                    cx.notify();
                }),
            )
            .child(
                div()
                    .size_full()
                    .overflow_hidden()
                    .on_children_prepainted(move |bounds_list: Vec<Bounds<Pixels>>, window, cx| {
                        let Some(bounds) = bounds_list.first().copied() else {
                            return;
                        };
                        let scale = window.scale_factor();
                        if let Some(view) = entity.upgrade() {
                            view.update(cx, |view, cx| {
                                let mut changed = view.ensure_session(cx);
                                changed |= view.sync_surface_size(bounds, scale);
                                if changed {
                                    cx.notify();
                                }
                            });
                        }
                    })
                    .child(
                        div()
                            .size_full()
                            .overflow_hidden()
                            .bg(theme.background)
                            .px(px(12.0))
                            .py(px(10.0))
                            .children(status_line.map(|(text, color)| {
                                div()
                                    .font_family(theme.mono_font_family.clone())
                                    .text_size(px(self.initial_font_size.max(12.0)))
                                    .line_height(line_height)
                                    .text_color(color)
                                    .whitespace_pre()
                                    .child(render_terminal_line(text))
                            }))
                            .children(screen_text.map(|text| {
                                div()
                                    .font_family(theme.mono_font_family.clone())
                                    .text_size(px(self.initial_font_size.max(12.0)))
                                    .line_height(line_height)
                                    .text_color(theme.foreground)
                                    .whitespace_pre()
                                    .child(text)
                            })),
                    ),
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

fn render_terminal_line(line: &str) -> String {
    if line.is_empty() {
        return "\u{00A0}".to_string();
    }

    let mut rendered = String::with_capacity(line.len());
    for ch in line.chars() {
        match ch {
            ' ' => rendered.push('\u{00A0}'),
            '\t' => rendered.push_str("\u{00A0}\u{00A0}\u{00A0}\u{00A0}"),
            _ => rendered.push(ch),
        }
    }
    rendered
}

fn xterm_modifier_param(modifiers: &Modifiers) -> Option<u8> {
    let mask = u8::from(modifiers.shift)
        | (u8::from(modifiers.alt) << 1)
        | (u8::from(modifiers.control) << 2);
    if mask == 0 { None } else { Some(1 + mask) }
}

fn encode_special_key(key: &str, modifiers: &Modifiers, decckm: bool) -> Option<String> {
    let m = xterm_modifier_param(modifiers);

    let tilde = |code: u8| match m {
        Some(m) => format!("\x1b[{};{}~", code, m),
        None => format!("\x1b[{}~", code),
    };

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
        "backspace" if modifiers.alt && !modifiers.control && !modifiers.platform => {
            "\x1b\x7f".into()
        }
        "backspace" => "\x7f".into(),
        "tab" if modifiers.shift && !modifiers.control && !modifiers.platform => "\x1b[Z".into(),
        "tab" => "\t".into(),
        _ => return None,
    })
}
