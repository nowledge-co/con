use gpui::*;

use crate::agent_panel::AgentPanel;
use crate::input_bar::{EscapeInput, InputBar, InputMode, SubmitInput};
use crate::settings_panel::{self, SettingsPanel};
use crate::terminal_view::TerminalView;
use crate::theme::Theme;
use crate::{CloseTab, NewTab, ToggleAgentPanel};
use con_core::config::Config;
use con_core::harness::{AgentHarness, HarnessEvent};

/// The main workspace: terminal + agent panel + input bar + settings overlay
pub struct ConWorkspace {
    terminal: Entity<TerminalView>,
    agent_panel: Entity<AgentPanel>,
    input_bar: Entity<InputBar>,
    settings_panel: Entity<SettingsPanel>,
    harness: AgentHarness,
    agent_panel_open: bool,
}

impl ConWorkspace {
    pub fn new(config: Config, cx: &mut Context<Self>) -> Self {
        let terminal = cx.new(|cx| TerminalView::new(80, 24, cx));
        let agent_panel = cx.new(|cx| AgentPanel::new(cx));
        let input_bar = cx.new(|cx| InputBar::new(cx));
        let settings_panel = cx.new(|cx| SettingsPanel::new(&config, cx));
        let harness = AgentHarness::new(&config);

        // Listen for input bar events
        cx.subscribe(&input_bar, Self::on_input_submit).detach();
        cx.subscribe(&input_bar, Self::on_input_escape).detach();

        // Poll harness events periodically
        let events_rx = harness.events().clone();
        cx.spawn(async move |this, cx| {
            loop {
                let mut got_event = false;
                // Drain all pending events
                while let Ok(event) = events_rx.try_recv() {
                    got_event = true;
                    let ev = event.clone();
                    this.update(cx, |workspace, cx| {
                        workspace.handle_harness_event(ev, cx);
                    })
                    .ok();
                }
                if !got_event {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(50))
                        .await;
                }
            }
        })
        .detach();

        Self {
            terminal,
            agent_panel,
            input_bar,
            settings_panel,
            harness,
            agent_panel_open: false,
        }
    }

    fn on_input_escape(
        &mut self,
        _input_bar: Entity<InputBar>,
        _event: &EscapeInput,
        _cx: &mut Context<Self>,
    ) {
        // Terminal will regain focus since input bar is no longer capturing keys
    }

    fn on_input_submit(
        &mut self,
        input_bar: Entity<InputBar>,
        _event: &SubmitInput,
        cx: &mut Context<Self>,
    ) {
        let (content, mode) = input_bar.update(cx, |bar, _| {
            (bar.take_content(), bar.mode())
        });

        if content.trim().is_empty() {
            return;
        }

        match mode {
            InputMode::Shell => {
                // Write directly to terminal PTY
                self.terminal.update(cx, |tv, _| {
                    tv.write_to_pty(format!("{}\n", content).as_bytes());
                });
            }
            InputMode::Agent | InputMode::Smart => {
                // Open agent panel if not already open
                if !self.agent_panel_open {
                    self.agent_panel_open = true;
                }

                // Show user message in panel
                self.agent_panel.update(cx, |panel, cx| {
                    panel.add_message("user", &content, cx);
                });

                // Build context from terminal grid
                let grid = self.terminal.read(cx).grid();
                let context = self.harness.build_context(&grid.lock(), None);

                // Send to agent
                self.harness.send_message(content, context);
            }
        }

        cx.notify();
    }

    fn handle_harness_event(&mut self, event: HarnessEvent, cx: &mut Context<Self>) {
        match event {
            HarnessEvent::Thinking => {
                self.agent_panel.update(cx, |panel, cx| {
                    panel.add_step("Thinking...", cx);
                });
            }
            HarnessEvent::Step(step) => {
                let step_text = format!("{:?}", step);
                self.agent_panel.update(cx, |panel, cx| {
                    panel.add_step(&step_text, cx);
                });
            }
            HarnessEvent::Token(token) => {
                self.agent_panel.update(cx, |panel, cx| {
                    panel.update_streaming(&token, cx);
                });
            }
            HarnessEvent::ResponseComplete(msg) => {
                self.agent_panel.update(cx, |panel, cx| {
                    panel.complete_streaming(&msg.content, cx);
                });
            }
            HarnessEvent::Error(err) => {
                self.agent_panel.update(cx, |panel, cx| {
                    panel.add_message("system", &format!("Error: {}", err), cx);
                });
            }
            HarnessEvent::SkillsUpdated(_) => {}
        }
        cx.notify();
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
        let settings_visible = self.settings_panel.read(cx).is_visible();

        if settings_visible {
            root = root.child(self.settings_panel.clone());
        }

        root
    }
}
