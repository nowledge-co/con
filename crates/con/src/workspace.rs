use gpui::*;

use crate::agent_panel::AgentPanel;
use crate::input_bar::InputBar;
use crate::settings_panel::{self, SettingsPanel};
use crate::terminal_view::TerminalView;
use crate::theme::Theme;
use crate::{CloseTab, NewTab, ToggleAgentPanel};
use con_core::Config;

/// The main workspace: terminal + agent panel + input bar + settings overlay
pub struct ConWorkspace {
    terminal: Entity<TerminalView>,
    agent_panel: Entity<AgentPanel>,
    input_bar: Entity<InputBar>,
    settings_panel: Entity<SettingsPanel>,
    agent_panel_open: bool,
}

impl ConWorkspace {
    pub fn new(config: Config, cx: &mut Context<Self>) -> Self {
        let terminal = cx.new(|cx| TerminalView::new(80, 24, cx));
        let agent_panel = cx.new(|cx| AgentPanel::new(cx));
        let input_bar = cx.new(|cx| InputBar::new(cx));
        let settings_panel = cx.new(|cx| SettingsPanel::new(&config, cx));

        Self {
            terminal,
            agent_panel,
            input_bar,
            settings_panel,
            agent_panel_open: false,
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

    fn toggle_settings(
        &mut self,
        _: &settings_panel::ToggleSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_panel.update(cx, |panel, cx| {
            panel.toggle(cx);
            if panel.is_visible() {
                panel.focus_handle(cx).focus(window);
            }
        });
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

        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(Theme::base()))
            .key_context("ConWorkspace")
            .on_action(cx.listener(Self::toggle_agent_panel))
            .on_action(cx.listener(Self::toggle_settings))
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
            );

        // Settings overlay (rendered on top)
        let settings_visible = self
            .settings_panel
            .read(cx)
            .is_visible();

        if settings_visible {
            root = root.child(self.settings_panel.clone());
        }

        root
    }
}
