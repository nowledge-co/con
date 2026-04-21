//! Linux placeholder for `ghostty_view`.
//!
//! Mirrors the public surface of `crate::ghostty_view` exactly so the
//! rest of `con-app` compiles unchanged while the Linux renderer is
//! still pending. Unlike the generic stub, this file is Linux-specific
//! and now targets the chosen long-term lane: a local Unix PTY backend
//! plus GPUI-owned rendering.

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
    #[allow(dead_code)]
    app: Arc<GhosttyApp>,
    terminal: Option<Arc<GhosttyTerminal>>,
    focus_handle: FocusHandle,
    #[allow(dead_code)]
    initial_cwd: Option<String>,
    #[allow(dead_code)]
    initial_font_size: f32,
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
        }
    }

    pub fn terminal(&self) -> Option<&Arc<GhosttyTerminal>> {
        self.terminal.as_ref()
    }

    pub fn write_or_queue(&mut self, _data: &[u8]) {}

    pub fn title(&self) -> Option<String> {
        None
    }

    pub fn current_dir(&self) -> Option<String> {
        None
    }

    pub fn is_alive(&self) -> bool {
        false
    }

    pub fn surface_ready(&self) -> bool {
        false
    }

    #[allow(dead_code)]
    pub fn selection_text(&self) -> Option<String> {
        None
    }

    pub fn shutdown_surface(&mut self) {}

    pub fn set_surface_focus_state(&mut self, _focused: bool) {}

    pub fn ensure_initialized_for_control(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    pub fn set_visible(&self, _visible: bool) {}

    pub fn sync_window_background_blur(&self) {}

    pub fn drain_surface_state(&mut self, _cx: &mut Context<Self>) -> bool {
        false
    }

    pub fn pump_deferred_work(&mut self, _cx: &mut Context<Self>) -> bool {
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
        div()
            .flex()
            .flex_col()
            .size_full()
            .items_center()
            .justify_center()
            .gap(px(10.0))
            .bg(theme.background.opacity(0.9))
            .text_color(theme.foreground.opacity(0.8))
            .child(div().text_lg().child("Linux terminal renderer under construction"))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.foreground.opacity(0.5))
                    .child("Plan: docs/impl/linux-port.md"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.foreground.opacity(0.4))
                    .child(
                        "Lane selected: local Linux backend. Unix PTY \
                         lifecycle now lives in con; GPUI-owned rendering \
                         is the next milestone before this pane becomes live.",
                    ),
            )
    }
}
