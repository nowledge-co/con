use gpui::*;
use gpui_component::ActiveTheme;

use crate::agent_panel::AgentPanel;
use crate::command_palette::{CommandPalette, PaletteSelect, ToggleCommandPalette};
use crate::input_bar::{EscapeInput, InputBar, InputMode, SubmitInput};
use crate::settings_panel::{self, SaveSettings, SettingsPanel};
use crate::sidebar::{SessionEntry, SessionSidebar, SidebarSelect};
use crate::terminal_view::TerminalView;
use crate::{CloseTab, NewTab, ToggleAgentPanel};
use con_core::config::Config;
use con_core::harness::{AgentHarness, HarnessEvent};
use con_core::session::Session;

struct Tab {
    terminal: Entity<TerminalView>,
    title: String,
}

/// The main workspace: sidebar + tabs + agent panel + input bar + settings overlay
pub struct ConWorkspace {
    sidebar: Entity<SessionSidebar>,
    tabs: Vec<Tab>,
    active_tab: usize,
    font_size: f32,
    scrollback_lines: usize,
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
        let font_size = config.terminal.font_size;
        let scrollback_lines = config.terminal.scrollback_lines;
        let session = Session::load().unwrap_or_default();

        let mut tabs: Vec<Tab> = session
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab_state)| {
                let terminal = cx.new(|cx| TerminalView::new(80, 24, font_size, scrollback_lines, cx));
                Tab {
                    terminal,
                    title: if tab_state.title.is_empty() {
                        format!("Terminal {}", i + 1)
                    } else {
                        tab_state.title.clone()
                    },
                }
            })
            .collect();
        if tabs.is_empty() {
            tabs.push(Tab {
                terminal: cx.new(|cx| TerminalView::new(80, 24, font_size, scrollback_lines, cx)),
                title: "Terminal".to_string(),
            });
        }
        let active_tab = session.active_tab.min(tabs.len() - 1);
        let agent_panel_open = session.agent_panel_open;
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
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_select)
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
            active_tab,
            font_size,
            scrollback_lines,
            agent_panel,
            input_bar,
            settings_panel,
            command_palette,
            harness,
            agent_panel_open,
        }
    }

    fn active_terminal(&self) -> &Entity<TerminalView> {
        &self.tabs[self.active_tab].terminal
    }

    fn save_session(&self, cx: &App) {
        let tabs: Vec<con_core::session::TabState> = self
            .tabs
            .iter()
            .map(|tab| {
                let cwd = tab.terminal.read(cx).grid().lock().current_dir.clone();
                let title = tab
                    .terminal
                    .read(cx)
                    .title()
                    .unwrap_or_else(|| tab.title.clone());
                con_core::session::TabState {
                    title,
                    cwd,
                    panes: vec![],
                }
            })
            .collect();

        let session = Session {
            tabs,
            active_tab: self.active_tab,
            agent_panel_open: self.agent_panel_open,
        };
        if let Err(e) = session.save() {
            log::warn!("Failed to save session: {}", e);
        }
    }

    fn on_sidebar_select(
        &mut self,
        _sidebar: &Entity<SessionSidebar>,
        event: &SidebarSelect,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_tab(event.index, cx);
    }

    fn sync_sidebar(&self, cx: &mut Context<Self>) {
        let sessions: Vec<SessionEntry> = self
            .tabs
            .iter()
            .map(|tab| {
                let title = tab
                    .terminal
                    .read(cx)
                    .title()
                    .unwrap_or_else(|| tab.title.clone());
                SessionEntry {
                    name: title,
                    is_ssh: false,
                }
            })
            .collect();
        self.sidebar.update(cx, |sidebar, cx| {
            sidebar.sync_sessions(sessions, self.active_tab, cx);
        });
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
                let terminal = self.active_terminal().clone();
                terminal.update(cx, |tv, _| {
                    tv.write_to_pty(format!("{}\n", content).as_bytes());
                });
                // Refocus terminal after sending shell command
                terminal.focus_handle(cx).focus(window, cx);
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
        self.save_session(cx);
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
        let font_size = self.font_size;
        let scrollback_lines = self.scrollback_lines;
        let terminal = cx.new(|cx| TerminalView::new(80, 24, font_size, scrollback_lines, cx));
        let tab_number = self.tabs.len() + 1;
        self.tabs.push(Tab {
            terminal,
            title: format!("Terminal {}", tab_number),
        });
        self.active_tab = self.tabs.len() - 1;
        self.sync_sidebar(cx);
        self.save_session(cx);
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
        self.sync_sidebar(cx);
        self.save_session(cx);
        cx.notify();
    }

    fn activate_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.active_tab = index;
            self.sync_sidebar(cx);
            self.save_session(cx);
            cx.notify();
        }
    }
}

impl Render for ConWorkspace {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_terminal = self.tabs[self.active_tab].terminal.clone();

        // Sync sidebar with current tabs
        self.sync_sidebar(cx);

        // Sync CWD from active terminal to input bar
        let cwd = active_terminal.read(cx).grid().lock().current_dir.clone();
        if let Some(cwd) = cwd {
            let display_cwd = match dirs::home_dir() {
                Some(home) => {
                    let home_str = home.to_string_lossy().to_string();
                    if cwd.starts_with(&home_str) {
                        format!("~{}", &cwd[home_str.len()..])
                    } else {
                        cwd
                    }
                }
                None => cwd,
            };
            self.input_bar.update(cx, |bar, _cx| {
                bar.set_cwd(display_cwd);
            });
        }

        let theme = cx.theme();

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

        // Tab bar — macOS-style with close buttons and quiet active treatment
        let tab_count = self.tabs.len();
        let mut tab_bar = div()
            .flex()
            .h(px(38.0))
            .bg(theme.title_bar)
            .items_end()
            .pl(px(80.0)) // leave room for traffic lights
            .pr(px(16.0))
            .gap(px(1.0))
            .border_b_1()
            .border_color(theme.border);

        for (index, tab) in self.tabs.iter().enumerate() {
            let is_active = index == self.active_tab;
            let title = tab
                .terminal
                .read(cx)
                .title()
                .unwrap_or_else(|| tab.title.clone());

            // Truncate long titles
            let display_title: String = if title.len() > 24 {
                format!("{}...", &title[..21])
            } else {
                title
            };

            let close_id = ElementId::Name(format!("tab-close-{}", index).into());

            // Close button — visible on hover for inactive tabs, always for active
            let show_close = tab_count > 1;
            let close_button = if show_close {
                Some(
                    div()
                        .id(close_id)
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(16.0))
                        .rounded(px(4.0))
                        .ml(px(6.0))
                        .text_size(px(10.0))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.muted.opacity(0.3)))
                        .text_color(theme.muted_foreground)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                // Close this specific tab
                                if this.tabs.len() > 1 {
                                    this.tabs.remove(index);
                                    if this.active_tab >= this.tabs.len() {
                                        this.active_tab = this.tabs.len() - 1;
                                    } else if this.active_tab > index {
                                        this.active_tab -= 1;
                                    }
                                    this.save_session(cx);
                                    cx.notify();
                                }
                                let _ = window;
                            }),
                        )
                        .child("×"),
                )
            } else {
                None
            };

            let mut tab_el = div()
                .id(ElementId::Name(format!("tab-{}", index).into()))
                .group("tab")
                .flex()
                .items_center()
                .px(px(12.0))
                .py(px(6.0))
                .rounded_t(px(8.0))
                .text_size(px(12.0))
                .max_w(px(200.0))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _window, cx| {
                    this.activate_tab(index, cx);
                }));

            if is_active {
                tab_el = tab_el
                    .bg(theme.background)
                    .text_color(theme.foreground)
                    .font_weight(FontWeight::MEDIUM);
            } else {
                tab_el = tab_el
                    .text_color(theme.muted_foreground)
                    .hover(|s| s.bg(theme.secondary.opacity(0.5)));
            }

            let mut tab_content = div()
                .flex()
                .items_center()
                .overflow_x_hidden()
                .child(display_title);

            if let Some(close) = close_button {
                tab_content = tab_content.child(close);
            }

            tab_bar = tab_bar.child(tab_el.child(tab_content));
        }

        // "+" button for new tab
        tab_bar = tab_bar.child(
            div()
                .id("tab-new")
                .flex()
                .items_center()
                .justify_center()
                .size(px(24.0))
                .mb(px(4.0))
                .rounded(px(6.0))
                .text_size(px(14.0))
                .text_color(theme.muted_foreground)
                .cursor_pointer()
                .hover(|s| s.bg(theme.secondary.opacity(0.5)))
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
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                let mods = &event.keystroke.modifiers;
                let key = event.keystroke.key.as_str();

                // Cmd+1..9 — jump to tab
                if mods.platform && !mods.shift {
                    if let Some(digit) = key.chars().next().and_then(|c| c.to_digit(10)) {
                        let tab_index = if digit == 0 { 9 } else { (digit - 1) as usize };
                        if tab_index < this.tabs.len() {
                            this.activate_tab(tab_index, cx);
                        }
                    }
                }

                // Cmd+Shift+[ — previous tab
                if mods.platform && mods.shift && key == "[" {
                    if this.active_tab > 0 {
                        this.activate_tab(this.active_tab - 1, cx);
                    } else if !this.tabs.is_empty() {
                        this.activate_tab(this.tabs.len() - 1, cx);
                    }
                }

                // Cmd+Shift+] — next tab
                if mods.platform && mods.shift && key == "]" {
                    let next = this.active_tab + 1;
                    if next < this.tabs.len() {
                        this.activate_tab(next, cx);
                    } else {
                        this.activate_tab(0, cx);
                    }
                }
            }))
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
