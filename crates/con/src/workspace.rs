use gpui::*;
use gpui_component::ActiveTheme;

use crate::agent_panel::AgentPanel;
use crate::command_palette::{CommandPalette, PaletteSelect, ToggleCommandPalette};
use crate::input_bar::{EscapeInput, InputBar, InputMode, SubmitInput};
use crate::settings_panel::{self, SaveSettings, SettingsPanel};
use crate::sidebar::SessionSidebar;
use crate::terminal_view::TerminalView;
use crate::{CloseTab, NewTab, ToggleAgentPanel};
use con_core::config::Config;
use con_core::harness::{AgentHarness, HarnessEvent};

struct Tab {
    terminal: Entity<TerminalView>,
    title: String,
}

/// The main workspace: sidebar + tabs + agent panel + input bar + settings overlay
pub struct ConWorkspace {
    sidebar: Entity<SessionSidebar>,
    tabs: Vec<Tab>,
    active_tab: usize,
    agent_panel: Entity<AgentPanel>,
    input_bar: Entity<InputBar>,
    settings_panel: Entity<SettingsPanel>,
    command_palette: Entity<CommandPalette>,
    harness: AgentHarness,
    agent_panel_open: bool,
}

impl ConWorkspace {
    pub fn new(config: Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sidebar = cx.new(|cx| SessionSidebar::new(cx));
        let first_terminal = cx.new(|cx| TerminalView::new(80, 24, cx));
        let tabs = vec![Tab {
            terminal: first_terminal,
            title: "Terminal".to_string(),
        }];
        let agent_panel = cx.new(|cx| AgentPanel::new(cx));
        let input_bar = cx.new(|cx| InputBar::new(window, cx));
        let settings_panel = cx.new(|cx| SettingsPanel::new(&config, window, cx));
        let command_palette = cx.new(|cx| CommandPalette::new(window, cx));
        let harness = AgentHarness::new(&config);

        cx.subscribe_in(&input_bar, window, Self::on_input_submit)
            .detach();
        cx.subscribe_in(&input_bar, window, Self::on_input_escape)
            .detach();
        cx.subscribe_in(&settings_panel, window, Self::on_settings_saved)
            .detach();
        cx.subscribe_in(&command_palette, window, Self::on_palette_select)
            .detach();

        // Poll harness events periodically
        let events_rx = harness.events().clone();
        cx.spawn(async move |this, cx| {
            loop {
                let mut got_event = false;
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
            sidebar,
            tabs,
            active_tab: 0,
            agent_panel,
            input_bar,
            settings_panel,
            command_palette,
            harness,
            agent_panel_open: false,
        }
    }

    fn active_terminal(&self) -> &Entity<TerminalView> {
        &self.tabs[self.active_tab].terminal
    }

    fn on_palette_select(
        &mut self,
        _palette: &Entity<CommandPalette>,
        event: &PaletteSelect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.action_id.as_str() {
            "toggle-agent" => {
                self.agent_panel_open = !self.agent_panel_open;
            }
            "settings" => {
                self.settings_panel.update(cx, |panel, cx| {
                    panel.toggle(window, cx);
                });
            }
            "new-tab" => {
                self.new_tab(&NewTab, window, cx);
            }
            "close-tab" => {
                self.close_tab(&CloseTab, window, cx);
            }
            "quit" => {
                cx.quit();
            }
            _ => {}
        }
        cx.notify();
    }

    fn on_settings_saved(
        &mut self,
        settings: &Entity<SettingsPanel>,
        _event: &SaveSettings,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new_config = settings.read(cx).agent_config().clone();
        self.harness.update_config(new_config);
    }

    fn on_input_escape(
        &mut self,
        _input_bar: &Entity<InputBar>,
        _event: &EscapeInput,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    fn on_input_submit(
        &mut self,
        input_bar: &Entity<InputBar>,
        _event: &SubmitInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (content, mode) = input_bar.update(cx, |bar, cx| {
            (bar.take_content(window, cx), bar.mode())
        });

        if content.trim().is_empty() {
            return;
        }

        match mode {
            InputMode::Shell => {
                self.active_terminal().update(cx, |tv, _| {
                    tv.write_to_pty(format!("{}\n", content).as_bytes());
                });
            }
            InputMode::Agent | InputMode::Smart => {
                if !self.agent_panel_open {
                    self.agent_panel_open = true;
                }

                self.agent_panel.update(cx, |panel, cx| {
                    panel.add_message("user", &content, cx);
                });

                let grid = self.active_terminal().read(cx).grid();
                let context = self.harness.build_context(&grid.lock(), None);
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
            HarnessEvent::ToolCallStart {
                call_id,
                tool_name,
                args,
            } => {
                self.agent_panel.update(cx, |panel, cx| {
                    panel.add_tool_call(&call_id, &tool_name, &args, cx);
                });
            }
            HarnessEvent::ToolApprovalNeeded {
                call_id,
                tool_name,
                args,
                approval_tx,
            } => {
                self.agent_panel.update(cx, |panel, cx| {
                    panel.add_pending_approval(
                        &call_id,
                        &tool_name,
                        &args,
                        approval_tx,
                        cx,
                    );
                });
            }
            HarnessEvent::ToolCallComplete {
                call_id,
                tool_name,
                result,
            } => {
                self.agent_panel.update(cx, |panel, cx| {
                    panel.complete_tool_call(&call_id, &tool_name, &result, cx);
                });
            }
            HarnessEvent::ResponseComplete(msg) => {
                self.agent_panel.update(cx, |panel, cx| {
                    panel.complete_response(&msg.content, cx);
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

    fn toggle_command_palette(
        &mut self,
        _: &ToggleCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.command_palette.update(cx, |palette, cx| {
            palette.toggle(window, cx);
        });
        cx.notify();
    }

    fn toggle_settings(
        &mut self,
        _: &settings_panel::ToggleSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.settings_panel.update(cx, |panel, cx| {
            panel.toggle(window, cx);
        });
        cx.notify();
    }

    fn new_tab(&mut self, _: &NewTab, _window: &mut Window, cx: &mut Context<Self>) {
        let terminal = cx.new(|cx| TerminalView::new(80, 24, cx));
        let tab_number = self.tabs.len() + 1;
        self.tabs.push(Tab {
            terminal,
            title: format!("Terminal {}", tab_number),
        });
        self.active_tab = self.tabs.len() - 1;
        cx.notify();
    }

    fn close_tab(&mut self, _: &CloseTab, _window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }
        self.tabs.remove(self.active_tab);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
        cx.notify();
    }

    fn activate_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.active_tab = index;
            cx.notify();
        }
    }
}

impl Render for ConWorkspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let active_terminal = self.tabs[self.active_tab].terminal.clone();

        let mut main_area = div()
            .flex()
            .flex_1()
            .min_h_0()
            .child(self.sidebar.clone())
            .child(div().flex_1().min_w_0().child(active_terminal));

        if self.agent_panel_open {
            main_area = main_area.child(
                div()
                    .w(px(400.0))
                    .border_l_1()
                    .border_color(theme.border)
                    .child(self.agent_panel.clone()),
            );
        }

        // Tab bar
        let mut tab_bar = div()
            .flex()
            .h(px(38.0))
            .bg(theme.title_bar)
            .items_end()
            .px(px(80.0))
            .gap(px(2.0));

        for (index, tab) in self.tabs.iter().enumerate() {
            let is_active = index == self.active_tab;
            let title = tab.terminal.read(cx).title().unwrap_or_else(|| tab.title.clone());
            let mut tab_el = div()
                .id(ElementId::Name(format!("tab-{}", index).into()))
                .px(px(16.0))
                .py(px(6.0))
                .rounded_t(px(8.0))
                .text_size(px(12.0))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.activate_tab(index, cx);
                }));

            if is_active {
                tab_el = tab_el
                    .bg(theme.background)
                    .text_color(theme.foreground);
            } else {
                tab_el = tab_el
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.bg(theme.secondary));
            }

            tab_bar = tab_bar.child(tab_el.child(title));
        }

        // "+" button for new tab
        tab_bar = tab_bar.child(
            div()
                .id("tab-new")
                .px(px(8.0))
                .py(px(6.0))
                .rounded_t(px(8.0))
                .text_size(px(12.0))
                .text_color(theme.muted_foreground)
                .cursor_pointer()
                .hover(|s| s.bg(theme.secondary))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.new_tab(&NewTab, window, cx);
                }))
                .child("+"),
        );

        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .key_context("ConWorkspace")
            .on_action(cx.listener(Self::toggle_agent_panel))
            .on_action(cx.listener(Self::toggle_settings))
            .on_action(cx.listener(Self::toggle_command_palette))
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_tab))
            .child(tab_bar)
            .child(main_area)
            .child(
                div()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(self.input_bar.clone()),
            );

        let settings_visible = self.settings_panel.read(cx).is_visible();
        if settings_visible {
            root = root.child(self.settings_panel.clone());
        }

        let palette_visible = self.command_palette.read(cx).is_visible();
        if palette_visible {
            root = root.child(self.command_palette.clone());
        }

        root
    }
}
