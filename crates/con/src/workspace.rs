use gpui::*;

use crate::agent_panel::AgentPanel;
use crate::input_bar::InputBar;
use crate::terminal_view::TerminalView;
use crate::theme::Theme;
use crate::{CloseTab, NewTab, ToggleAgentPanel};
use con_core::Config;

/// The main workspace: terminal + agent panel + input bar
pub struct ConWorkspace {
    terminal: Entity<TerminalView>,
    agent_panel: Entity<AgentPanel>,
    input_bar: Entity<InputBar>,
    agent_panel_open: bool,
    config: Config,
}

impl ConWorkspace {
    pub fn new(config: Config, cx: &mut Context<Self>) -> Self {
        let terminal = cx.new(|cx| TerminalView::new(80, 24, cx));
        let agent_panel = cx.new(|cx| AgentPanel::new(cx));
        let input_bar = cx.new(|cx| InputBar::new(cx));

        Self {
            terminal,
            agent_panel,
            input_bar,
            agent_panel_open: false,
            config,
        }
    }

    fn toggle_agent_panel(
        &mut self,
        _: &ToggleAgentPanel,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.agent_panel_open = !self.agent_panel_open;
        cx.notify();
    }

    fn new_tab(&mut self, _: &NewTab, _window: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
    }

    fn close_tab(&mut self, _: &CloseTab, _window: &mut Window, cx: &mut Context<Self>) {
        cx.notify();
    }
}

impl Render for ConWorkspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut main_area = div().flex().flex_1().min_h_0().child(
            div().flex_1().min_w_0().child(self.terminal.clone()),
        );

        if self.agent_panel_open {
            main_area = main_area.child(
                div()
                    .w(px(400.0))
                    .border_l_1()
                    .border_color(rgb(Theme::surface0()))
                    .child(self.agent_panel.clone()),
            );
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(Theme::base()))
            .key_context("ConWorkspace")
            .on_action(cx.listener(Self::toggle_agent_panel))
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_tab))
            // Titlebar
            .child(
                div()
                    .flex()
                    .h(px(38.0))
                    .bg(rgb(Theme::mantle()))
                    .items_center()
                    .px(px(80.0))
                    .child(
                        div()
                            .px(px(16.0))
                            .py(px(6.0))
                            .rounded_t(px(6.0))
                            .text_sm()
                            .bg(rgb(Theme::base()))
                            .text_color(rgb(Theme::text()))
                            .child("Terminal"),
                    ),
            )
            // Main area
            .child(main_area)
            // Input bar
            .child(
                div()
                    .border_t_1()
                    .border_color(rgb(Theme::surface0()))
                    .child(self.input_bar.clone()),
            )
    }
}
