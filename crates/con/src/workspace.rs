use gpui::*;
use gpui_component::ActiveTheme;

use crate::agent_panel::{AgentPanel, LoadConversation, NewConversation};
use crate::command_palette::{CommandPalette, PaletteSelect, ToggleCommandPalette};
use crate::input_bar::{EscapeInput, InputBar, InputMode, PaneInfo, SubmitInput};
use crate::pane_tree::{PaneTree, SplitDirection};
use crate::settings_panel::{self, SaveSettings, SettingsPanel};
use crate::sidebar::{NewSession, SessionEntry, SessionSidebar, SidebarSelect};
use crate::terminal_view::{ClosePaneRequest, ExplainCommand, FocusChanged, TerminalView};
use crate::{CloseTab, NewTab, SplitDown, SplitRight, ToggleAgentPanel};
use con_core::config::Config;
use con_core::harness::{AgentHarness, HarnessEvent, InputKind};
use con_core::session::Session;

struct Tab {
    pane_tree: PaneTree,
    title: String,
    needs_attention: bool,
}

/// The main workspace: tabs + agent panel + input bar + settings overlay
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
    agent_panel_width: f32,
    /// Tracks whether a modal was open on the last render, so we can
    /// restore terminal focus when a modal dismisses itself internally.
    modal_was_open: bool,
    /// Shared bridge between divider on_mouse_down (plain Fn closure) and
    /// workspace's entity-level drag handler. Persists across render cycles.
    pending_drag_init: std::sync::Arc<std::sync::Mutex<Option<(usize, f32)>>>,
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
                    pane_tree: PaneTree::new(terminal),
                    title: if tab_state.title.is_empty() {
                        format!("Terminal {}", i + 1)
                    } else {
                        tab_state.title.clone()
                    },
                    needs_attention: false,
                }
            })
            .collect();
        if tabs.is_empty() {
            let terminal = cx.new(|cx| TerminalView::new(80, 24, font_size, scrollback_lines, cx));
            tabs.push(Tab {
                pane_tree: PaneTree::new(terminal),
                title: "Terminal".to_string(),
                needs_attention: false,
            });
        }
        // Subscribe to events from all terminals
        for tab in &tabs {
            for terminal in tab.pane_tree.all_terminals() {
                cx.subscribe_in(terminal, window, Self::on_explain_command)
                    .detach();
                cx.subscribe_in(terminal, window, Self::on_close_pane_request)
                    .detach();
                cx.subscribe_in(terminal, window, Self::on_focus_changed)
                    .detach();
            }
        }
        let active_tab = session.active_tab.min(tabs.len() - 1);
        let agent_panel_open = session.agent_panel_open;
        let agent_panel = cx.new(|cx| AgentPanel::new(cx));
        let input_bar = cx.new(|cx| InputBar::new(window, cx));
        let settings_panel = cx.new(|cx| SettingsPanel::new(&config, window, cx));
        let command_palette = cx.new(|cx| CommandPalette::new(window, cx));
        let mut harness = AgentHarness::new(&config);

        // Restore previous conversation if saved in session
        if let Some(conv_id) = &session.conversation_id {
            harness.load_conversation(conv_id);
        }

        cx.subscribe_in(&input_bar, window, Self::on_input_submit)
            .detach();
        cx.subscribe_in(&input_bar, window, Self::on_input_escape)
            .detach();
        cx.subscribe_in(&settings_panel, window, Self::on_settings_saved)
            .detach();
        cx.subscribe_in(&command_palette, window, Self::on_palette_select)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_new_conversation)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_load_conversation)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_select)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_new_session)
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

        // Focus the initial terminal so the user can start typing immediately
        let initial_terminal = tabs[active_tab].pane_tree.focused_terminal().clone();
        initial_terminal.focus_handle(cx).focus(window, cx);

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
            agent_panel_width: 400.0,
            modal_was_open: false,
            pending_drag_init: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn active_terminal(&self) -> &Entity<TerminalView> {
        self.tabs[self.active_tab].pane_tree.focused_terminal()
    }

    fn save_session(&self, cx: &App) {
        let tabs: Vec<con_core::session::TabState> = self
            .tabs
            .iter()
            .map(|tab| {
                let terminal = tab.pane_tree.focused_terminal();
                let cwd = terminal.read(cx).grid().lock().current_dir.clone();
                let title = terminal
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
            conversation_id: Some(self.harness.conversation_id()),
        };
        if let Err(e) = session.save() {
            log::warn!("Failed to save session: {}", e);
        }
    }

    fn on_new_conversation(
        &mut self,
        _panel: &Entity<AgentPanel>,
        _event: &NewConversation,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.harness.new_conversation();
        self.save_session(cx);
    }

    fn on_load_conversation(
        &mut self,
        _panel: &Entity<AgentPanel>,
        event: &LoadConversation,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.harness.load_conversation(&event.id) {
            // Replay messages into the panel
            self.agent_panel.update(cx, |panel, cx| {
                panel.clear_messages(cx);
                let conv = self.harness.conversation();
                let conv = conv.lock().unwrap_or_else(|e| e.into_inner());
                for msg in &conv.messages {
                    let role = match msg.role {
                        con_agent::MessageRole::User => "user",
                        con_agent::MessageRole::Assistant => "assistant",
                        con_agent::MessageRole::System => "system",
                        con_agent::MessageRole::Tool => "system",
                    };
                    panel.add_message(role, &msg.content, cx);
                }
            });
            self.save_session(cx);
        }
    }

    fn on_sidebar_select(
        &mut self,
        _sidebar: &Entity<SessionSidebar>,
        event: &SidebarSelect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_tab(event.index, window, cx);
    }

    fn on_sidebar_new_session(
        &mut self,
        _sidebar: &Entity<SessionSidebar>,
        _event: &NewSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_tab(&NewTab, window, cx);
    }

    fn sync_sidebar(&self, cx: &mut Context<Self>) {
        let sessions: Vec<SessionEntry> = self
            .tabs
            .iter()
            .map(|tab| {
                let terminal = tab.pane_tree.focused_terminal();
                let title = terminal
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
            "split-right" => {
                self.split_pane(SplitDirection::Horizontal, window, cx);
            }
            "split-down" => {
                self.split_pane(SplitDirection::Vertical, window, cx);
            }
            "clear-terminal" => {
                let terminal = self.active_terminal().clone();
                terminal.update(cx, |tv, _| {
                    tv.grid().lock().clear_scrollback();
                });
            }
            "focus-terminal" => {
                let terminal = self.active_terminal().clone();
                terminal.focus_handle(cx).focus(window, cx);
            }
            "toggle-sidebar" => {
                self.sidebar.update(cx, |sidebar, cx| {
                    sidebar.toggle_collapsed(cx);
                });
            }
            "cycle-input-mode" => {
                self.input_bar.update(cx, |bar, cx| {
                    bar.cycle_mode(window, cx);
                });
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new_config = settings.read(cx).agent_config().clone();
        self.harness.update_config(new_config);

        let term_config = settings.read(cx).terminal_config().clone();
        self.font_size = term_config.font_size;
        self.scrollback_lines = term_config.scrollback_lines;

        // Settings panel closes on save — restore terminal focus
        self.focus_terminal(window, cx);
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
                self.execute_shell(&content, window, cx);
            }
            InputMode::Agent => {
                self.send_to_agent(&content, cx);
            }
            InputMode::Smart => {
                match self.harness.classify_input(&content) {
                    InputKind::ShellCommand(cmd) => {
                        self.execute_shell(&cmd, window, cx);
                    }
                    InputKind::NaturalLanguage(text) => {
                        self.send_to_agent(&text, cx);
                    }
                    InputKind::SkillInvoke(name, args) => {
                        let grid = self.active_terminal().read(cx).grid();
                        let context = self.harness.build_context(&grid.lock(), None);
                        if let Some(desc) = self.harness.invoke_skill(&name, args.as_deref(), context) {
                            if !self.agent_panel_open {
                                self.agent_panel_open = true;
                            }
                            self.agent_panel.update(cx, |panel, cx| {
                                let label = format!("/{name}");
                                panel.add_message("user", &label, cx);
                                panel.add_step(&desc, cx);
                            });
                        }
                    }
                }
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
                // Mark non-active tabs for attention
                for (i, tab) in self.tabs.iter_mut().enumerate() {
                    if i != self.active_tab {
                        tab.needs_attention = true;
                    }
                }
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
        // Close settings if open (mutually exclusive)
        if self.settings_panel.read(cx).is_visible() {
            self.settings_panel.update(cx, |panel, cx| {
                panel.toggle(window, cx);
            });
        }
        self.command_palette.update(cx, |palette, cx| {
            palette.toggle(window, cx);
        });
        // Restore terminal focus if palette just closed
        if !self.is_modal_open(cx) {
            self.focus_terminal(window, cx);
        }
        cx.notify();
    }

    fn toggle_settings(
        &mut self,
        _: &settings_panel::ToggleSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Close command palette if open (mutually exclusive)
        if self.command_palette.read(cx).is_visible() {
            self.command_palette.update(cx, |palette, cx| {
                palette.toggle(window, cx);
            });
        }
        self.settings_panel.update(cx, |panel, cx| {
            panel.toggle(window, cx);
        });
        // Restore terminal focus if settings just closed
        if !self.is_modal_open(cx) {
            self.focus_terminal(window, cx);
        }
        cx.notify();
    }

    fn is_modal_open(&self, cx: &App) -> bool {
        self.settings_panel.read(cx).is_visible()
            || self.command_palette.read(cx).is_visible()
    }

    fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        let font_size = self.font_size;
        let scrollback_lines = self.scrollback_lines;
        let terminal = cx.new(|cx| TerminalView::new(80, 24, font_size, scrollback_lines, cx));
        cx.subscribe_in(&terminal, window, Self::on_explain_command)
            .detach();
        cx.subscribe_in(&terminal, window, Self::on_close_pane_request)
            .detach();
        cx.subscribe_in(&terminal, window, Self::on_focus_changed)
            .detach();
        let tab_number = self.tabs.len() + 1;
        self.tabs.push(Tab {
            pane_tree: PaneTree::new(terminal.clone()),
            title: format!("Terminal {}", tab_number),
            needs_attention: false,
        });
        self.active_tab = self.tabs.len() - 1;
        // Focus the new terminal
        terminal.focus_handle(cx).focus(window, cx);
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

    fn execute_shell(&self, cmd: &str, window: &mut Window, cx: &mut Context<Self>) {
        let target_ids = self.input_bar.read(cx).target_pane_ids();
        let is_broadcast = target_ids.len() > 1;
        let pane_tree = &self.tabs[self.active_tab].pane_tree;
        let all_terminals = pane_tree.all_terminals();

        for terminal in &all_terminals {
            if all_terminals.len() == 1 || target_ids.iter().any(|&tid| {
                pane_tree.terminal_has_pane_id(terminal, tid)
            }) {
                terminal.update(cx, |tv, _| {
                    tv.write_to_pty(format!("{}\n", cmd).as_bytes());
                });
            }
        }

        if is_broadcast {
            // After broadcast, keep focus on input bar so user can type another command
            self.input_bar.focus_handle(cx).focus(window, cx);
        } else {
            // Single-pane send: focus the target terminal
            let focused = self.active_terminal().clone();
            focused.focus_handle(cx).focus(window, cx);
        }
    }

    fn send_to_agent(&mut self, content: &str, cx: &mut Context<Self>) {
        if !self.agent_panel_open {
            self.agent_panel_open = true;
        }
        self.agent_panel.update(cx, |panel, cx| {
            panel.add_message("user", content, cx);
        });
        let grid = self.active_terminal().read(cx).grid();
        let context = self.harness.build_context(&grid.lock(), None);
        self.harness.send_message(content.to_string(), context);
    }

    fn split_pane(
        &mut self,
        direction: SplitDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let font_size = self.font_size;
        let scrollback_lines = self.scrollback_lines;
        let terminal = cx.new(|cx| TerminalView::new(80, 24, font_size, scrollback_lines, cx));
        cx.subscribe_in(&terminal, window, Self::on_explain_command)
            .detach();
        cx.subscribe_in(&terminal, window, Self::on_close_pane_request)
            .detach();
        cx.subscribe_in(&terminal, window, Self::on_focus_changed)
            .detach();
        self.tabs[self.active_tab]
            .pane_tree
            .split(direction, terminal.clone());
        terminal.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn split_right(
        &mut self,
        _: &SplitRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.split_pane(SplitDirection::Horizontal, window, cx);
    }

    fn split_down(
        &mut self,
        _: &SplitDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.split_pane(SplitDirection::Vertical, window, cx);
    }

    fn on_close_pane_request(
        &mut self,
        _terminal: &Entity<TerminalView>,
        _event: &ClosePaneRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane_tree = &mut self.tabs[self.active_tab].pane_tree;
        if pane_tree.pane_count() > 1 {
            pane_tree.close_focused();
            // Focus the new focused terminal
            let terminal = pane_tree.focused_terminal().clone();
            terminal.focus_handle(cx).focus(window, cx);
        } else if self.tabs.len() > 1 {
            self.close_tab(&CloseTab, window, cx);
        }
        cx.notify();
    }

    fn on_focus_changed(
        &mut self,
        terminal: &Entity<TerminalView>,
        _event: &FocusChanged,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Terminal gained focus — directly update pane tracking and ensure focus sticks
        let pane_tree = &mut self.tabs[self.active_tab].pane_tree;
        if let Some(pane_id) = pane_tree.pane_id_for_terminal(terminal) {
            pane_tree.focus(pane_id);
            // Re-assert focus on the terminal to ensure nothing steals it
            terminal.focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn on_explain_command(
        &mut self,
        _terminal: &Entity<TerminalView>,
        event: &ExplainCommand,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let prompt = format!(
            "Explain this command and its output:\n\nCommand: {}\n\nOutput:\n```\n{}\n```",
            event.command, event.output
        );
        self.send_to_agent(&prompt, cx);
    }

    fn activate_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.active_tab = index;
            self.tabs[index].needs_attention = false;
            // Focus the terminal in the newly activated tab
            let terminal = self.tabs[index].pane_tree.focused_terminal().clone();
            terminal.focus_handle(cx).focus(window, cx);
            self.save_session(cx);
            cx.notify();
        }
    }

    /// Focus the active terminal (used after modal close, etc.)
    fn focus_terminal(&self, window: &mut Window, cx: &mut App) {
        let terminal = self.active_terminal().clone();
        terminal.focus_handle(cx).focus(window, cx);
    }
}

impl Render for ConWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let active_terminal = self.active_terminal().clone();

        // If a modal was dismissed internally (escape/backdrop), restore terminal focus
        let is_modal_open = self.is_modal_open(cx);
        if self.modal_was_open && !is_modal_open {
            self.focus_terminal(window, cx);
        }
        self.modal_was_open = is_modal_open;

        // Keep pane focus in sync with which terminal has window focus
        self.tabs[self.active_tab].pane_tree.sync_focus(window, cx);

        // Sync pane info and CWD to input bar
        let pane_tree = &self.tabs[self.active_tab].pane_tree;
        let pane_names = pane_tree.pane_names(cx);
        let focused_pane_id = pane_tree.focused_pane_id();
        let pane_infos: Vec<PaneInfo> = pane_names
            .into_iter()
            .map(|(id, name)| PaneInfo { id, name })
            .collect();

        let cwd = active_terminal.read(cx).grid().lock().current_dir.clone();
        let display_cwd = cwd
            .map(|cwd| match dirs::home_dir() {
                Some(home) => {
                    let home_str = home.to_string_lossy().to_string();
                    if cwd.starts_with(&home_str) {
                        format!("~{}", &cwd[home_str.len()..])
                    } else {
                        cwd
                    }
                }
                None => cwd,
            })
            .unwrap_or_else(|| "~".to_string());

        self.input_bar.update(cx, |bar, _cx| {
            bar.set_panes(pane_infos, focused_pane_id);
            bar.set_cwd(display_cwd);
        });

        let theme = cx.theme();

        let pane_tree_rendered = {
            let pending = self.pending_drag_init.clone();
            let begin_drag_cb = move |split_id: usize, start_pos: f32| {
                if let Ok(mut guard) = pending.lock() {
                    *guard = Some((split_id, start_pos));
                }
            };
            self.tabs[self.active_tab]
                .pane_tree
                .render(begin_drag_cb, cx)
        };

        let terminal_area = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(pane_tree_rendered);

        let mut main_area = div()
            .flex()
            .flex_1()
            .min_h_0()
            .child(terminal_area);

        if self.agent_panel_open {
            main_area = main_area.child(
                div()
                    .w(px(self.agent_panel_width))
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
            let needs_attention = tab.needs_attention && !is_active;
            let terminal = tab.pane_tree.focused_terminal();
            let title = terminal
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
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.activate_tab(index, window, cx);
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
                .overflow_x_hidden();

            // Attention dot for tabs with pending agent activity
            if needs_attention {
                tab_content = tab_content.child(
                    div()
                        .size(px(6.0))
                        .rounded_full()
                        .bg(theme.primary)
                        .mr(px(6.0)),
                );
            }

            tab_content = tab_content.child(display_title);

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
            // Pane drag-to-resize: capture mouse move/up on root so it works
            // even when cursor is over terminal views (which capture mouse events).
            .on_mouse_move({
                let pending = self.pending_drag_init.clone();
                let agent_panel_open = self.agent_panel_open;
                let agent_panel_width = self.agent_panel_width;
                cx.listener(move |this, event: &MouseMoveEvent, win, cx| {
                    let pane_tree = &mut this.tabs[this.active_tab].pane_tree;

                    // Consume a pending drag initiation written by divider on_mouse_down
                    if let Ok(mut guard) = pending.lock() {
                        if let Some((split_id, start_pos)) = guard.take() {
                            pane_tree.begin_drag(split_id, start_pos);
                        }
                    }

                    if !pane_tree.is_dragging() {
                        return;
                    }

                    // Estimate terminal area from window bounds minus fixed chrome
                    // (tab bar ~38px, input bar ~40px, agent panel if open)
                    let win_w = f32::from(win.bounds().size.width);
                    let win_h = f32::from(win.bounds().size.height);
                    let (current_pos, total_size) =
                        if let Some(dir) = pane_tree.dragging_direction() {
                            match dir {
                                SplitDirection::Horizontal => {
                                    let panel_w = if agent_panel_open { agent_panel_width } else { 0.0 };
                                    (f32::from(event.position.x), win_w - panel_w)
                                }
                                SplitDirection::Vertical => {
                                    (f32::from(event.position.y), win_h - 78.0) // tab bar + input bar
                                }
                            }
                        } else {
                            return;
                        };

                    if pane_tree.update_drag(current_pos, total_size) {
                        cx.notify();
                    }
                })
            })
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    let pane_tree = &mut this.tabs[this.active_tab].pane_tree;
                    if pane_tree.is_dragging() {
                        pane_tree.end_drag();
                        cx.notify();
                    }
                }),
            )
            .on_action(cx.listener(Self::toggle_agent_panel))
            .on_action(cx.listener(Self::toggle_settings))
            .on_action(cx.listener(Self::toggle_command_palette))
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::split_right))
            .on_action(cx.listener(Self::split_down))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // Don't handle workspace shortcuts when a modal overlay is open
                if this.settings_panel.read(cx).is_visible()
                    || this.command_palette.read(cx).is_visible()
                {
                    return;
                }

                let mods = &event.keystroke.modifiers;
                let key = event.keystroke.key.as_str();

                // Cmd+1..9 — jump to tab
                if mods.platform && !mods.shift {
                    if let Some(digit) = key.chars().next().and_then(|c| c.to_digit(10)) {
                        let tab_index = if digit == 0 { 9 } else { (digit - 1) as usize };
                        if tab_index < this.tabs.len() {
                            this.activate_tab(tab_index, window, cx);
                        }
                    }
                }

                // Cmd+Shift+[ — previous tab
                if mods.platform && mods.shift && key == "[" {
                    if this.active_tab > 0 {
                        let prev = this.active_tab - 1;
                        this.activate_tab(prev, window, cx);
                    } else if !this.tabs.is_empty() {
                        let last = this.tabs.len() - 1;
                        this.activate_tab(last, window, cx);
                    }
                }

                // Cmd+Shift+] — next tab
                if mods.platform && mods.shift && key == "]" {
                    let next = this.active_tab + 1;
                    if next < this.tabs.len() {
                        this.activate_tab(next, window, cx);
                    } else {
                        this.activate_tab(0, window, cx);
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
