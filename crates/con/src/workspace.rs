use gpui::*;
use gpui_component::ActiveTheme;

const AGENT_PANEL_DEFAULT_WIDTH: f32 = 400.0;
const AGENT_PANEL_MIN_WIDTH: f32 = 200.0;
const AGENT_PANEL_MAX_WIDTH: f32 = 800.0;

use crate::agent_panel::{AgentPanel, CancelRequest, EnableAutoApprove, LoadConversation, NewConversation, PanelState};
use crate::command_palette::{CommandPalette, PaletteSelect, ToggleCommandPalette};
use crate::input_bar::{EscapeInput, InputBar, InputMode, PaneInfo, SubmitInput};
use crate::pane_tree::{PaneTree, SplitDirection};
use crate::settings_panel::{self, SaveSettings, SettingsPanel, ThemePreview};
use crate::sidebar::{NewSession, SessionEntry, SessionSidebar, SidebarSelect};
use crate::terminal_pane::{TerminalPane, subscribe_terminal_pane};
use crate::terminal_view::{ClosePaneRequest, ExplainCommand, FocusChanged, InputChanged, TerminalView};
use con_terminal::TerminalTheme;

#[cfg(target_os = "macos")]
use crate::ghostty_view::{GhosttyFocusChanged, GhosttyProcessExited, GhosttyTitleChanged, GhosttyView};
use crate::{CloseTab, NewTab, SplitDown, SplitRight, ToggleAgentPanel};
use con_core::config::Config;
use con_agent::{Conversation, TerminalExecRequest, TerminalExecResponse};
use con_core::harness::{AgentHarness, AgentSession, HarnessEvent, InputKind};
use con_core::session::Session;
use con_core::suggestions::SuggestionEngine;

struct Tab {
    pane_tree: PaneTree,
    title: String,
    needs_attention: bool,
    session: AgentSession,
    panel_state: PanelState,
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
    /// Agent panel drag state: start X position and start width when drag began.
    agent_panel_drag: Option<(f32, f32)>,
    /// Shell command suggestion engine (debounced AI completions)
    suggestion_engine: SuggestionEngine,
    /// Channel for sending suggestion results from the async engine back to GPUI
    suggestion_tx: crossbeam_channel::Sender<(gpui::EntityId, String)>,
    /// Current terminal color theme
    terminal_theme: TerminalTheme,
    /// Whether to use ghostty backend for new terminals
    use_ghostty: bool,
    /// Shared ghostty app instance (macOS only, lazy-initialized)
    #[cfg(target_os = "macos")]
    ghostty_app: Option<std::sync::Arc<con_ghostty::GhosttyApp>>,
}

impl ConWorkspace {
    pub fn new(config: Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sidebar = cx.new(|cx| SessionSidebar::new(cx));
        let font_size = config.terminal.font_size;
        let scrollback_lines = config.terminal.scrollback_lines;
        let terminal_theme = TerminalTheme::by_name(&config.terminal.theme)
            .unwrap_or_default();
        let session = Session::load().unwrap_or_default();

        let mut tabs: Vec<Tab> = session
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab_state)| {
                let theme = &terminal_theme;
                let cwd = tab_state.cwd.as_deref();
                let terminal_view = cx.new(|cx| TerminalView::with_options(80, 24, font_size, scrollback_lines, theme, cwd, cx));
                let terminal = TerminalPane::Legacy(terminal_view);
                // Restore per-tab conversation, with migration from global conversation_id
                let agent_session = if let Some(conv_id) = &tab_state.conversation_id {
                    match Conversation::load(conv_id) {
                        Ok(conv) => AgentSession::with_conversation(conv),
                        Err(_) => AgentSession::new(),
                    }
                } else if i == 0 {
                    // Migration: first tab gets the old session-level conversation
                    if let Some(conv_id) = &session.conversation_id {
                        match Conversation::load(conv_id) {
                            Ok(conv) => AgentSession::with_conversation(conv),
                            Err(_) => AgentSession::new(),
                        }
                    } else {
                        AgentSession::new()
                    }
                } else {
                    AgentSession::new()
                };
                let panel_state = {
                    let conv = agent_session.conversation().clone();
                    let conv = conv.lock();
                    PanelState::from_conversation(&conv)
                };
                Tab {
                    pane_tree: PaneTree::new(terminal),
                    title: if tab_state.title.is_empty() {
                        format!("Terminal {}", i + 1)
                    } else {
                        tab_state.title.clone()
                    },
                    needs_attention: false,
                    session: agent_session,
                    panel_state,
                }
            })
            .collect();
        if tabs.is_empty() {
            let terminal_view = cx.new(|cx| TerminalView::with_options(80, 24, font_size, scrollback_lines, &terminal_theme, None, cx));
            let terminal = TerminalPane::Legacy(terminal_view);
            tabs.push(Tab {
                pane_tree: PaneTree::new(terminal),
                title: "Terminal".to_string(),
                needs_attention: false,
                session: AgentSession::new(),
                panel_state: PanelState::new(),
            });
        }
        // Subscribe to events from all terminals
        for tab in &tabs {
            for terminal in tab.pane_tree.all_terminals() {
                subscribe_terminal_pane(terminal, window, cx);
            }
        }
        let active_tab = session.active_tab.min(tabs.len() - 1);
        let agent_panel_open = session.agent_panel_open;
        let agent_panel_width = session.agent_panel_width.unwrap_or(AGENT_PANEL_DEFAULT_WIDTH);
        // Take the active tab's restored panel state for the AgentPanel
        let initial_panel_state = std::mem::replace(
            &mut tabs[active_tab].panel_state,
            PanelState::new(),
        );
        let agent_panel = cx.new(|cx| AgentPanel::with_state(initial_panel_state, cx));
        let input_bar = cx.new(|cx| InputBar::new(window, cx));
        let settings_panel = cx.new(|cx| SettingsPanel::new(&config, window, cx));
        let command_palette = cx.new(|cx| CommandPalette::new(window, cx));
        let harness = AgentHarness::new(&config).unwrap_or_else(|e| {
            log::error!("Failed to create agent harness: {}. Agent features disabled.", e);
            panic!("Fatal: agent harness initialization failed: {}", e);
        });
        let suggestion_engine = harness.suggestion_engine(300);
        let (suggestion_tx, suggestion_rx) = crossbeam_channel::unbounded();

        cx.subscribe_in(&input_bar, window, Self::on_input_submit)
            .detach();
        cx.subscribe_in(&input_bar, window, Self::on_input_escape)
            .detach();
        cx.subscribe_in(&settings_panel, window, Self::on_settings_saved)
            .detach();
        cx.subscribe_in(&settings_panel, window, Self::on_theme_preview)
            .detach();
        cx.subscribe_in(&command_palette, window, Self::on_palette_select)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_new_conversation)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_load_conversation)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_cancel_request)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_enable_auto_approve)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_select)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_new_session)
            .detach();

        // Poll all tabs' agent sessions + suggestions
        let suggestion_rx_for_poll = suggestion_rx.clone();
        cx.spawn(async move |this, cx| {
            loop {
                let mut got_event = false;

                this.update(cx, |workspace, cx| {
                    // Drain events from every tab's session
                    for tab_idx in 0..workspace.tabs.len() {
                        let is_active = tab_idx == workspace.active_tab;

                        // Agent events
                        while let Ok(event) = workspace.tabs[tab_idx].session.events().try_recv() {
                            got_event = true;
                            if is_active {
                                workspace.handle_harness_event(event, cx);
                            } else {
                                workspace.tabs[tab_idx].panel_state.apply_event(event);
                                workspace.tabs[tab_idx].needs_attention = true;
                            }
                        }

                        // Terminal exec requests — route to the tab that owns the session
                        while let Ok(req) = workspace.tabs[tab_idx].session.terminal_exec_requests().try_recv() {
                            got_event = true;
                            workspace.handle_terminal_exec_request_for_tab(tab_idx, req, cx);
                        }

                        // Pane queries — route to the tab that owns the session
                        while let Ok(req) = workspace.tabs[tab_idx].session.pane_requests().try_recv() {
                            got_event = true;
                            workspace.handle_pane_request_for_tab(tab_idx, req, cx);
                        }
                    }

                    // Apply suggestion results
                    while let Ok((entity_id, suggestion)) = suggestion_rx_for_poll.try_recv() {
                        got_event = true;
                        workspace.apply_suggestion(entity_id, suggestion, cx);
                    }
                }).ok();

                if !got_event {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(16))
                        .await;
                }
            }
        })
        .detach();

        // Focus the initial terminal so the user can start typing immediately
        let initial_terminal = tabs[active_tab].pane_tree.focused_terminal();
        initial_terminal.focus(window, cx);

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
            agent_panel_width,
            modal_was_open: false,
            pending_drag_init: std::sync::Arc::new(std::sync::Mutex::new(None)),
            agent_panel_drag: None,
            suggestion_engine,
            suggestion_tx,
            terminal_theme,
            use_ghostty: config.terminal.use_ghostty(),
            #[cfg(target_os = "macos")]
            ghostty_app: None,
        }
    }

    /// Create a new TerminalPane using the configured backend.
    fn create_terminal(
        &mut self,
        cwd: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TerminalPane {
        #[cfg(target_os = "macos")]
        if self.use_ghostty {
            // Lazy-init the shared ghostty app
            if self.ghostty_app.is_none() {
                match con_ghostty::GhosttyApp::new() {
                    Ok(app) => {
                        self.ghostty_app = Some(std::sync::Arc::new(app));
                    }
                    Err(e) => {
                        log::error!("Failed to create ghostty app: {}. Falling back to grid.", e);
                        self.use_ghostty = false;
                    }
                }
            }
            if let Some(ref app) = self.ghostty_app {
                let app = app.clone();
                let view = cx.new(|cx| {
                    crate::ghostty_view::GhosttyView::new(app, cx)
                });
                let pane = TerminalPane::Ghostty(view);
                subscribe_terminal_pane(&pane, window, cx);
                return pane;
            }
        }
        let font_size = self.font_size;
        let scrollback_lines = self.scrollback_lines;
        let theme = &self.terminal_theme;
        let terminal_view = cx.new(|cx| {
            if let Some(cwd) = cwd {
                TerminalView::with_options(80, 24, font_size, scrollback_lines, theme, Some(cwd), cx)
            } else {
                TerminalView::with_theme(80, 24, font_size, scrollback_lines, theme, cx)
            }
        });
        let pane = TerminalPane::Legacy(terminal_view);
        subscribe_terminal_pane(&pane, window, cx);
        pane
    }

    fn active_terminal(&self) -> &TerminalPane {
        self.tabs[self.active_tab].pane_tree.focused_terminal()
    }

    /// Build agent context from the focused pane, including summaries of other panes.
    fn build_agent_context(&self, cx: &App) -> con_agent::TerminalContext {
        let pane_tree = &self.tabs[self.active_tab].pane_tree;
        let focused = self.active_terminal();

        // Determine focused pane's 1-based index and hostname
        let all_terminals = pane_tree.all_terminals();
        let focused_pid = pane_tree.focused_pane_id();
        let focused_pane_index = all_terminals
            .iter()
            .enumerate()
            .find(|(_, t)| pane_tree.pane_id_for_terminal(t) == Some(focused_pid))
            .map(|(i, _)| i + 1)
            .unwrap_or(1);
        let focused_hostname = focused.detected_remote_host(cx);

        let mut other_pane_summaries = Vec::new();
        if pane_tree.pane_count() > 1 {
            for (idx, terminal) in all_terminals.iter().enumerate() {
                if let Some(pid) = pane_tree.pane_id_for_terminal(terminal) {
                    if pid == focused_pid {
                        continue;
                    }
                    other_pane_summaries.push(con_agent::context::PaneSummary {
                        pane_index: idx + 1,
                        hostname: terminal.detected_remote_host(cx),
                        cwd: terminal.current_dir(cx),
                        last_command: terminal.last_command(cx),
                        last_exit_code: terminal.last_exit_code(cx),
                        is_busy: terminal.is_busy(cx),
                        recent_output: terminal.content_lines(10, cx),
                    });
                }
            }
        }

        // Build context via Grid if available (legacy), otherwise construct manually
        if let Some(focused_grid) = focused.as_grid(cx) {
            self.harness
                .build_context(&focused_grid.lock(), None, focused_pane_index, focused_hostname, other_pane_summaries)
        } else {
            // Ghostty: build context without Grid access
            self.harness
                .build_context_from_snapshot(
                    &focused.content_lines(50, cx),
                    focused.current_dir(cx),
                    focused.last_command(cx),
                    focused.last_exit_code(cx),
                    focused.is_busy(cx),
                    focused_pane_index,
                    focused_hostname,
                    other_pane_summaries,
                )
        }
    }

    fn save_session(&self, cx: &App) {
        let tabs: Vec<con_core::session::TabState> = self
            .tabs
            .iter()
            .map(|tab| {
                let terminal = tab.pane_tree.focused_terminal();
                let cwd = terminal.current_dir(cx);
                let title = terminal
                    .title(cx)
                    .unwrap_or_else(|| tab.title.clone());
                con_core::session::TabState {
                    title,
                    cwd,
                    panes: vec![],
                    conversation_id: Some(tab.session.conversation_id()),
                }
            })
            .collect();

        let session = Session {
            tabs,
            active_tab: self.active_tab,
            agent_panel_open: self.agent_panel_open,
            agent_panel_width: Some(self.agent_panel_width),
            conversation_id: None, // deprecated — per-tab now
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
        self.tabs[self.active_tab].session.new_conversation();
        self.agent_panel.update(cx, |panel, cx| {
            panel.clear_messages(cx);
        });
        self.save_session(cx);
    }

    fn on_load_conversation(
        &mut self,
        _panel: &Entity<AgentPanel>,
        event: &LoadConversation,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs[self.active_tab].session.load_conversation(&event.id) {
            // Rebuild panel state from the loaded conversation
            let conv = self.tabs[self.active_tab].session.conversation();
            let conv = conv.lock();
            let new_state = PanelState::from_conversation(&conv);
            drop(conv);
            self.agent_panel.update(cx, |panel, cx| {
                panel.swap_state(new_state, cx);
            });
            self.save_session(cx);
        }
    }

    fn on_cancel_request(
        &mut self,
        _panel: &Entity<AgentPanel>,
        _event: &CancelRequest,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.tabs[self.active_tab].session.cancel_current();
    }

    fn on_enable_auto_approve(
        &mut self,
        _panel: &Entity<AgentPanel>,
        _event: &EnableAutoApprove,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.harness.set_auto_approve(true);
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
            .enumerate()
            .map(|(i, tab)| {
                let terminal = tab.pane_tree.focused_terminal();
                let hostname = terminal.detected_remote_host(cx);
                let is_ssh = hostname.is_some();
                let title = terminal.title(cx);
                let current_dir = terminal.current_dir(cx);
                let name = pane_display_name(&hostname, &title, &current_dir, i);
                SessionEntry { name, is_ssh }
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
                self.active_terminal().clear_scrollback(cx);
            }
            "focus-terminal" => {
                self.active_terminal().focus(window, cx);
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

        // Apply terminal theme if changed
        if let Some(new_theme) = TerminalTheme::by_name(&term_config.theme) {
            if new_theme.name != self.terminal_theme.name {
                self.terminal_theme = new_theme.clone();
                // Update all existing terminal panes
                for tab in &self.tabs {
                    for terminal in tab.pane_tree.all_terminals() {
                        terminal.set_theme(&new_theme, cx);
                    }
                }
                // Sync GPUI UI theme (dark/light) with terminal theme
                crate::theme::sync_gpui_mode(&new_theme.name, window, cx);
            }
        }

        // Settings panel closes on save — restore terminal focus
        self.focus_terminal(window, cx);
    }

    fn on_theme_preview(
        &mut self,
        _settings: &Entity<SettingsPanel>,
        event: &ThemePreview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(new_theme) = TerminalTheme::by_name(&event.0) {
            if new_theme.name != self.terminal_theme.name {
                self.terminal_theme = new_theme.clone();
                for tab in &self.tabs {
                    for terminal in tab.pane_tree.all_terminals() {
                        terminal.set_theme(&new_theme, cx);
                    }
                }
                // Sync GPUI UI theme (dark/light) with terminal theme
                crate::theme::sync_gpui_mode(&new_theme.name, window, cx);
                cx.notify();
            }
        }
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
                let is_remote = self.active_terminal().detected_remote_host(cx).is_some();
                match self.harness.classify_input(&content, is_remote) {
                    InputKind::ShellCommand(cmd) => {
                        self.execute_shell(&cmd, window, cx);
                    }
                    InputKind::NaturalLanguage(text) => {
                        self.send_to_agent(&text, cx);
                    }
                    InputKind::SkillInvoke(name, args) => {
                        let context = self.build_agent_context(cx);
                        let session = &self.tabs[self.active_tab].session;
                        if let Some(desc) = self.harness.invoke_skill(session, &name, args.as_deref(), context) {
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
            HarnessEvent::ThinkingDelta(text) => {
                self.agent_panel.update(cx, |panel, cx| {
                    panel.update_thinking(&text, cx);
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

    /// Handle a visible terminal execution request from the agent.
    ///
    /// Writes the command to the focused PTY so the user sees it execute.
    /// Registers an OSC 133 completion callback to capture the output
    /// and send it back to the tool. Falls back to a timeout-based capture
    /// for shells without shell integration.
    fn handle_terminal_exec_request_for_tab(
        &mut self,
        tab_idx: usize,
        req: TerminalExecRequest,
        cx: &mut Context<Self>,
    ) {
        // Use the specified pane, or fall back to the focused pane.
        let pane = if let Some(pane_index) = req.pane_index {
            let pane_tree = &self.tabs[tab_idx].pane_tree;
            let all_terminals = pane_tree.all_terminals();
            if pane_index == 0 || pane_index > all_terminals.len() {
                let _ = req.response_tx.send(TerminalExecResponse {
                    output: format!(
                        "Invalid pane index {}. Use list_panes to see available panes (1-{}).",
                        pane_index,
                        all_terminals.len()
                    ),
                    exit_code: Some(1),
                });
                return;
            }
            all_terminals[pane_index - 1].clone()
        } else {
            self.tabs[tab_idx].pane_tree.focused_terminal().clone()
        };

        // Safety: refuse to execute on a dead PTY.
        if !pane.is_alive(cx) {
            let _ = req.response_tx.send(TerminalExecResponse {
                output: "Pane PTY process has exited — cannot execute command.".to_string(),
                exit_code: Some(1),
            });
            return;
        }

        // Safety: warn if pane is busy (command in progress).
        if pane.is_busy(cx) {
            log::warn!(
                "[workspace] Executing on busy pane — a command is already in progress. \
                 OSC 133 tracking may produce unexpected results."
            );
        }

        // For legacy terminals, register the OSC 133 completion callback.
        // Ghostty doesn't support OSC 133 callbacks, so we rely solely on the fallback.
        if let Some(grid) = pane.as_grid(cx) {
            let response_tx = req.response_tx.clone();
            grid.lock().set_command_complete_callback(Box::new(
                move |output, exit_code| {
                    let _ = response_tx.send(TerminalExecResponse { output, exit_code });
                },
            ));
        }

        // Write the command to the PTY — user sees it execute in real time
        let cmd_with_newline = format!("{}\n", req.command);
        pane.write(cmd_with_newline.as_bytes(), cx);

        // Fallback: if OSC 133 never fires (no shell integration, e.g. SSH or ghostty),
        // detect completion via cursor stability or timeout.
        let fallback_response_tx = req.response_tx;
        let grid = pane.as_grid(cx);
        cx.spawn(async move |_this, cx| {
            // Wait for the command to start producing output
            cx.background_executor()
                .timer(std::time::Duration::from_millis(500))
                .await;

            let mut stable_count: u32 = 0;
            let mut last_cursor = (usize::MAX, usize::MAX);
            let required_stable = 2; // 2 × 500ms = 1s of no cursor movement

            for _ in 0..29 {
                // Check if the OSC 133 callback already responded
                if fallback_response_tx.is_full() {
                    return;
                }

                if let Some(ref grid) = grid {
                    let (done_osc, cursor_pos) = {
                        let g = grid.lock();
                        let osc_done = !g.is_busy() && g.last_prompt_row.is_some();
                        let pos = (g.cursor.row, g.cursor.col);
                        (osc_done, pos)
                    };

                    // Fast path: shell integration detected completion
                    if done_osc {
                        break;
                    }

                    // Slow path: cursor stability (for sessions without shell integration)
                    if cursor_pos == last_cursor {
                        stable_count += 1;
                        if stable_count >= required_stable {
                            break;
                        }
                    } else {
                        stable_count = 0;
                        last_cursor = cursor_pos;
                    }
                } else {
                    // Ghostty: no Grid access, just wait via timeout
                    stable_count += 1;
                    if stable_count >= 6 { // 6 × 500ms = 3s timeout
                        break;
                    }
                }

                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
            }

            // If the callback already fired, try_send will fail (channel closed).
            let output = if let Some(ref grid) = grid {
                let g = grid.lock();
                g.recent_lines(50).join("\n")
            } else {
                // Ghostty: no text extraction available
                "(output capture not available for ghostty backend)".to_string()
            };
            let _ = fallback_response_tx.try_send(TerminalExecResponse {
                output,
                exit_code: None,
            });
        })
        .detach();
    }

    fn handle_pane_request_for_tab(
        &self,
        tab_idx: usize,
        req: con_agent::PaneRequest,
        cx: &mut Context<Self>,
    ) {
        use con_agent::{PaneInfo, PaneQuery, PaneResponse};

        log::info!("[workspace] handle_pane_request entered");
        let pane_tree = &self.tabs[tab_idx].pane_tree;
        let focused_pid = pane_tree.focused_pane_id();
        let all_terminals = pane_tree.all_terminals();

        let response = match req.query {
            PaneQuery::List => {
                let panes: Vec<PaneInfo> = all_terminals
                    .iter()
                    .enumerate()
                    .map(|(idx, terminal)| {
                        let pid = pane_tree
                            .pane_id_for_terminal(terminal)
                            .unwrap_or(idx);
                        let title = terminal.title(cx)
                            .unwrap_or_else(|| format!("Pane {}", idx + 1));
                        let (cols, rows) = terminal.grid_size(cx);
                        let has_shell_integration = terminal.as_grid(cx)
                            .map(|g| g.lock().last_prompt_row.is_some())
                            .unwrap_or(false);
                        PaneInfo {
                            index: idx + 1,
                            title,
                            cwd: terminal.current_dir(cx),
                            is_focused: pid == focused_pid,
                            rows,
                            cols,
                            is_alive: terminal.is_alive(cx),
                            hostname: terminal.detected_remote_host(cx),
                            has_shell_integration,
                            last_command: terminal.last_command(cx),
                            last_exit_code: terminal.last_exit_code(cx),
                            is_busy: terminal.is_busy(cx),
                        }
                    })
                    .collect();
                PaneResponse::PaneList(panes)
            }
            PaneQuery::ReadContent { pane_index, lines } => {
                if pane_index == 0 || pane_index > all_terminals.len() {
                    PaneResponse::Error(format!(
                        "Invalid pane index {}. Use list_panes to see available panes (1-{}).",
                        pane_index,
                        all_terminals.len()
                    ))
                } else {
                    let terminal = &all_terminals[pane_index - 1];
                    let content = terminal.recent_lines(lines, cx).join("\n");
                    PaneResponse::Content(content)
                }
            }
            PaneQuery::SendKeys { pane_index, keys } => {
                if pane_index == 0 || pane_index > all_terminals.len() {
                    PaneResponse::Error(format!(
                        "Invalid pane index {}. Use list_panes to see available panes (1-{}).",
                        pane_index,
                        all_terminals.len()
                    ))
                } else {
                    let terminal = &all_terminals[pane_index - 1];
                    terminal.write(keys.as_bytes(), cx);
                    PaneResponse::KeysSent
                }
            }
            PaneQuery::SearchText {
                pane_index,
                pattern,
                max_matches,
            } => {
                let targets: Vec<(usize, &TerminalPane)> = match pane_index {
                    Some(idx) if idx >= 1 && idx <= all_terminals.len() => {
                        vec![(idx, all_terminals[idx - 1])]
                    }
                    Some(idx) => {
                        return {
                            let _ = req.response_tx.send(PaneResponse::Error(format!(
                                "Invalid pane index {}. Use list_panes to see available panes (1-{}).",
                                idx,
                                all_terminals.len()
                            )));
                        };
                    }
                    None => all_terminals
                        .iter()
                        .enumerate()
                        .map(|(i, t)| (i + 1, *t))
                        .collect(),
                };

                let mut results = Vec::new();
                let remaining = max_matches;
                for (idx, terminal) in &targets {
                    let per_pane = remaining.saturating_sub(results.len());
                    if per_pane == 0 {
                        break;
                    }
                    for (line_num, text) in terminal.search_text(&pattern, per_pane, cx) {
                        results.push((*idx, line_num, text));
                    }
                }
                PaneResponse::SearchResults(results)
            }
        };

        let _ = req.response_tx.send(response);
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
        let terminal = self.create_terminal(None, window, cx);
        let tab_number = self.tabs.len() + 1;
        let old_active = self.active_tab;

        self.tabs.push(Tab {
            pane_tree: PaneTree::new(terminal.clone()),
            title: format!("Terminal {}", tab_number),
            needs_attention: false,
            session: AgentSession::new(),
            panel_state: PanelState::new(),
        });
        self.active_tab = self.tabs.len() - 1;

        // Swap panel state: stash old tab's state, load new tab's (empty) state
        let incoming = std::mem::replace(
            &mut self.tabs[self.active_tab].panel_state,
            PanelState::new(),
        );
        let outgoing = self.agent_panel.update(cx, |panel, cx| {
            panel.swap_state(incoming, cx)
        });
        self.tabs[old_active].panel_state = outgoing;

        terminal.focus(window, cx);
        self.save_session(cx);
        cx.notify();
    }

    fn close_tab(&mut self, _: &CloseTab, _window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }
        // Save the closing tab's conversation
        {
            let conv = self.tabs[self.active_tab].session.conversation();
            let _ = conv.lock().save();
        }
        self.tabs.remove(self.active_tab);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
        // Swap new active tab's panel state into the panel
        let incoming = std::mem::replace(
            &mut self.tabs[self.active_tab].panel_state,
            PanelState::new(),
        );
        self.agent_panel.update(cx, |panel, cx| {
            panel.swap_state(incoming, cx);
        });
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
                terminal.write(format!("{}\n", cmd).as_bytes(), cx);
            }
        }

        if is_broadcast {
            self.input_bar.focus_handle(cx).focus(window, cx);
        } else {
            self.active_terminal().focus(window, cx);
        }
    }

    fn send_to_agent(&mut self, content: &str, cx: &mut Context<Self>) {
        if !self.agent_panel_open {
            self.agent_panel_open = true;
        }
        self.agent_panel.update(cx, |panel, cx| {
            panel.add_message("user", content, cx);
        });
        let context = self.build_agent_context(cx);
        let session = &self.tabs[self.active_tab].session;
        self.harness.send_message(session, content.to_string(), context);
    }

    fn split_pane(
        &mut self,
        direction: SplitDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let terminal = self.create_terminal(None, window, cx);
        self.tabs[self.active_tab]
            .pane_tree
            .split(direction, terminal.clone());
        terminal.focus(window, cx);
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

    pub(crate) fn on_close_pane_request(
        &mut self,
        _terminal: &Entity<TerminalView>,
        _event: &ClosePaneRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pane_tree = &mut self.tabs[self.active_tab].pane_tree;
        if pane_tree.pane_count() > 1 {
            pane_tree.close_focused();
            let terminal = pane_tree.focused_terminal();
            terminal.focus(window, cx);
        } else if self.tabs.len() > 1 {
            self.close_tab(&CloseTab, window, cx);
        }
        cx.notify();
    }

    pub(crate) fn on_focus_changed(
        &mut self,
        terminal: &Entity<TerminalView>,
        _event: &FocusChanged,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entity_id = terminal.entity_id();
        let pane_tree = &mut self.tabs[self.active_tab].pane_tree;
        if let Some(pane_id) = pane_tree.pane_id_for_entity(entity_id) {
            pane_tree.focus(pane_id);
            terminal.focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn apply_suggestion(
        &mut self,
        entity_id: gpui::EntityId,
        suggestion: String,
        cx: &mut Context<Self>,
    ) {
        // Find the terminal with this entity_id across all tabs
        for tab in &self.tabs {
            for terminal in tab.pane_tree.all_terminals() {
                if terminal.entity_id() == entity_id {
                    terminal.set_suggestion(Some(suggestion), cx);
                    return;
                }
            }
        }
    }

    pub(crate) fn on_input_changed(
        &mut self,
        terminal: &Entity<TerminalView>,
        event: &InputChanged,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entity_id = terminal.entity_id();
        match &event.input {
            Some(input) if input.len() >= 2 => {
                let tx = self.suggestion_tx.clone();
                self.suggestion_engine.request(input, move |suggestion| {
                    let _ = tx.send((entity_id, suggestion));
                });
            }
            _ => {
                self.suggestion_engine.cancel();
                // Clear any existing suggestion — find the pane and use TerminalPane API
                for tab in &self.tabs {
                    for pane in tab.pane_tree.all_terminals() {
                        if pane.entity_id() == entity_id {
                            pane.set_suggestion(None, cx);
                            return;
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn on_explain_command(
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
        if index >= self.tabs.len() || index == self.active_tab {
            return;
        }
        let old_active = self.active_tab;

        // Take the incoming tab's panel state
        let incoming = std::mem::replace(
            &mut self.tabs[index].panel_state,
            PanelState::new(),
        );
        // Swap into the panel, get the outgoing state back
        let outgoing = self.agent_panel.update(cx, |panel, cx| {
            panel.swap_state(incoming, cx)
        });
        // Stash outgoing state into the old tab
        self.tabs[old_active].panel_state = outgoing;

        self.active_tab = index;
        self.tabs[index].needs_attention = false;
        self.tabs[index].pane_tree.focused_terminal().focus(window, cx);
        self.save_session(cx);
        cx.notify();
    }

    /// Focus the active terminal (used after modal close, etc.)
    fn focus_terminal(&self, window: &mut Window, cx: &mut App) {
        self.active_terminal().focus(window, cx);
    }

    // ── Ghostty event handlers ──────────────────────────────

    #[cfg(target_os = "macos")]
    pub(crate) fn on_ghostty_focus_changed(
        &mut self,
        entity: &Entity<GhosttyView>,
        _event: &GhosttyFocusChanged,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entity_id = entity.entity_id();
        let pane_tree = &mut self.tabs[self.active_tab].pane_tree;
        if let Some(pane_id) = pane_tree.pane_id_for_entity(entity_id) {
            pane_tree.focus(pane_id);
            entity.focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn on_ghostty_process_exited(
        &mut self,
        _entity: &Entity<GhosttyView>,
        _event: &GhosttyProcessExited,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Treat like close-pane request
        let pane_tree = &mut self.tabs[self.active_tab].pane_tree;
        if pane_tree.pane_count() > 1 {
            pane_tree.close_focused();
            pane_tree.focused_terminal().focus(window, cx);
        } else if self.tabs.len() > 1 {
            self.close_tab(&CloseTab, window, cx);
        }
        cx.notify();
    }

    #[cfg(target_os = "macos")]
    pub(crate) fn on_ghostty_title_changed(
        &mut self,
        _entity: &Entity<GhosttyView>,
        _event: &GhosttyTitleChanged,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Title changed — sync sidebar and tab bar
        self.sync_sidebar(cx);
        cx.notify();
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
        let focused_pane_id = pane_tree.focused_pane_id();
        let pane_infos: Vec<PaneInfo> = pane_tree
            .pane_terminals()
            .into_iter()
            .map(|(id, terminal)| {
                let hostname = terminal.detected_remote_host(cx);
                let title = terminal.title(cx);
                let current_dir = terminal.current_dir(cx);
                let name = pane_display_name(&hostname, &title, &current_dir, id);
                let is_busy = terminal.is_busy(cx);
                let is_alive = terminal.is_alive(cx);
                PaneInfo { id, name, hostname, is_busy, is_alive }
            })
            .collect();

        let cwd = active_terminal.current_dir(cx);
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
            // Draggable divider between terminal area and agent panel
            main_area = main_area
                .child(
                    div()
                        .id("agent-panel-divider")
                        .w(px(4.0))
                        .h_full()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_col_resize()
                        .hover(|s| s.bg(theme.primary.opacity(0.06)))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, event: &MouseDownEvent, _window, _cx| {
                                this.agent_panel_drag =
                                    Some((f32::from(event.position.x), this.agent_panel_width));
                            }),
                        ),
                )
                .child(
                    div()
                        .w(px(self.agent_panel_width))
                        .h_full()
                        .overflow_hidden()
                        .child(self.agent_panel.clone()),
                );
        }

        // Tab bar — macOS-style with close buttons and quiet active treatment
        let tab_count = self.tabs.len();
        let mut tab_bar = div()
            .flex()
            .h(px(36.0))
            .bg(theme.title_bar)
            .items_end()
            .pl(px(78.0)) // leave room for traffic lights
            .pr(px(8.0))
            .gap(px(1.0));

        for (index, tab) in self.tabs.iter().enumerate() {
            let is_active = index == self.active_tab;
            let needs_attention = tab.needs_attention && !is_active;
            let terminal = tab.pane_tree.focused_terminal();
            let title = terminal
                .title(cx)
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
                        .hover(|s| s.bg(theme.muted.opacity(0.20)))
                        .text_color(theme.muted_foreground.opacity(0.5))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                if this.tabs.len() <= 1 {
                                    return;
                                }
                                // Save the closing tab's conversation
                                {
                                    let conv = this.tabs[index].session.conversation();
                                    let _ = conv.lock().save();
                                }
                                let was_active = index == this.active_tab;
                                this.tabs.remove(index);
                                if this.active_tab >= this.tabs.len() {
                                    this.active_tab = this.tabs.len() - 1;
                                } else if this.active_tab > index {
                                    this.active_tab -= 1;
                                }
                                // If the active tab was closed, swap new active's state into the panel
                                if was_active {
                                    let incoming = std::mem::replace(
                                        &mut this.tabs[this.active_tab].panel_state,
                                        PanelState::new(),
                                    );
                                    this.agent_panel.update(cx, |panel, cx| {
                                        panel.swap_state(incoming, cx);
                                    });
                                }
                                this.sync_sidebar(cx);
                                this.save_session(cx);
                                cx.notify();
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
                .h(px(30.0))
                .rounded(px(6.0))
                .mb(px(2.0))
                .text_size(px(12.0))
                .max_w(px(180.0))
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
                    .text_color(theme.muted_foreground.opacity(0.6))
                    .hover(|s| s.bg(theme.secondary));
            }

            let mut tab_content = div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .overflow_x_hidden();

            // Attention dot for tabs with pending agent activity
            if needs_attention {
                tab_content = tab_content.child(
                    div()
                        .size(px(6.0))
                        .rounded_full()
                        .bg(theme.primary),
                );
            }

            // Terminal icon
            tab_content = tab_content.child(
                svg()
                    .path("phosphor/terminal.svg")
                    .size(px(13.0))
                    .flex_shrink_0()
                    .text_color(if is_active {
                        theme.foreground
                    } else {
                        theme.muted_foreground.opacity(0.5)
                    }),
            );

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
                .ml(px(2.0))
                .rounded(px(5.0))
                .cursor_pointer()
                .text_color(theme.muted_foreground.opacity(0.4))
                .hover(|s| s.bg(theme.secondary).text_color(theme.muted_foreground))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.new_tab(&NewTab, window, cx);
                }))
                .child(
                    svg()
                        .path("phosphor/plus.svg")
                        .size(px(12.0)),
                ),
        );

        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .font_family("Ioskeley Mono")
            .key_context("ConWorkspace")
            // Pane drag-to-resize: capture mouse move/up on root so it works
            // even when cursor is over terminal views (which capture mouse events).
            .on_mouse_move({
                let pending = self.pending_drag_init.clone();
                cx.listener(move |this, event: &MouseMoveEvent, win, cx| {
                    // Agent panel resize drag
                    if let Some((start_x, start_width)) = this.agent_panel_drag {
                        let delta = start_x - f32::from(event.position.x);
                        let new_width = (start_width + delta).clamp(AGENT_PANEL_MIN_WIDTH, AGENT_PANEL_MAX_WIDTH);
                        if (this.agent_panel_width - new_width).abs() > 1.0 {
                            this.agent_panel_width = new_width;
                            // Notify all terminals so they detect new available space
                            for terminal in this.tabs[this.active_tab].pane_tree.all_terminals() {
                                terminal.notify(cx);
                            }
                            cx.notify();
                        }
                        return;
                    }

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
                                    let panel_w = if this.agent_panel_open { this.agent_panel_width + 7.0 } else { 0.0 };
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
                        // Notify all terminals in the active tab so they
                        // re-render and detect new bounds during canvas prepaint
                        for terminal in pane_tree.all_terminals() {
                            terminal.notify(cx);
                        }
                        cx.notify();
                    }
                })
            })
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                    if this.agent_panel_drag.is_some() {
                        this.agent_panel_drag = None;
                        this.save_session(cx);
                        cx.notify();
                        return;
                    }
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
            .child(self.input_bar.clone());

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

/// Derive a display name for a pane from available signals.
///
/// Priority:
/// 1. Remote hostname (SSH session detected via OSC 7, title, or persistent tracking)
/// 2. Terminal title — cleaned: strip `user@host:` prefix for local sessions,
///    use the path part if it's a typical `user@host: /path` pattern
/// 3. CWD directory name (skip bare home directories like `/Users/name`)
/// 4. Fallback "Pane N"
fn pane_display_name(
    hostname: &Option<String>,
    title: &Option<String>,
    current_dir: &Option<String>,
    pane_id: usize,
) -> String {
    // SSH session → show hostname
    if let Some(host) = hostname {
        return host.clone();
    }

    // Terminal title set by shell
    if let Some(title) = title {
        // Many shells set title to "user@host: /path" or "dirname — user@host"
        // For local sessions, extract the meaningful part
        let cleaned = if let Some(colon_pos) = title.find(':') {
            let before = title[..colon_pos].trim();
            let after = title[colon_pos + 1..].trim();
            // If before-colon contains @, it's user@host — use the path after colon
            if before.contains('@') {
                if after.is_empty() { before.to_string() } else { shorten_path(after) }
            } else {
                before.to_string()
            }
        } else if title.contains('@') {
            // Just "user@host" without path — use as-is
            title.clone()
        } else {
            title.clone()
        };
        if !cleaned.is_empty() {
            return cleaned;
        }
    }

    // CWD basename
    if let Some(dir) = current_dir {
        let path = std::path::Path::new(dir);
        // Skip bare home directories (e.g., /Users/weyl → "weyl" is confusing)
        let is_bare_home = matches!(
            path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy()),
            Some(ref name) if name == "home" || name == "Users"
        ) && path.parent().and_then(|p| p.parent()).map_or(false, |pp| pp.parent().is_none());

        if !is_bare_home {
            if let Some(base) = path.file_name() {
                return base.to_string_lossy().to_string();
            }
        }
    }

    format!("Pane {}", pane_id + 1)
}

/// Shorten a path for display: ~/foo/bar → bar, /long/deep/path → path
fn shorten_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed == "~" || trimmed == "/" {
        return trimmed.to_string();
    }
    // Use the last path component
    std::path::Path::new(trimmed)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| trimmed.to_string())
}
