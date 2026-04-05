use gpui::*;
use gpui_component::ActiveTheme;

const AGENT_PANEL_DEFAULT_WIDTH: f32 = 400.0;
const AGENT_PANEL_MIN_WIDTH: f32 = 200.0;
const AGENT_PANEL_MAX_WIDTH: f32 = 800.0;

use crate::agent_panel::{
    AgentPanel, CancelRequest, DeleteConversation, EnableAutoApprove, InlineInputSubmit,
    InlineSkillAutocompleteChanged, LoadConversation, NewConversation, PanelState,
    RerunFromMessage,
};
use crate::command_palette::{CommandPalette, PaletteSelect, ToggleCommandPalette};
use crate::input_bar::{
    EscapeInput, InputBar, InputMode, PaneInfo, SkillAutocompleteChanged, SubmitInput,
};
use crate::model_registry::ModelRegistry;
use crate::pane_tree::{PaneTree, SplitDirection};
use crate::settings_panel::{self, SaveSettings, SettingsPanel, ThemePreview};
use crate::sidebar::{NewSession, SessionEntry, SessionSidebar, SidebarSelect};
use crate::terminal_pane::{TerminalPane, subscribe_terminal_pane};
use con_terminal::TerminalTheme;

use crate::ghostty_view::{
    GhosttyFocusChanged, GhosttyProcessExited, GhosttyTitleChanged, GhosttyView,
};
use crate::{CloseTab, FocusInput, NewTab, Quit, SplitDown, SplitRight, ToggleAgentPanel};
use con_agent::{Conversation, TerminalExecRequest, TerminalExecResponse};
use con_core::config::Config;
use con_core::harness::{AgentHarness, AgentSession, HarnessEvent, InputKind};
use con_core::session::Session;

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
    agent_panel: Entity<AgentPanel>,
    input_bar: Entity<InputBar>,
    settings_panel: Entity<SettingsPanel>,
    command_palette: Entity<CommandPalette>,
    harness: AgentHarness,
    agent_panel_open: bool,
    agent_panel_width: f32,
    input_bar_visible: bool,
    /// Tracks whether a modal was open on the last render, so we can
    /// restore terminal focus when a modal dismisses itself internally.
    modal_was_open: bool,
    ghostty_hidden: bool,
    /// Shared bridge between divider on_mouse_down (plain Fn closure) and
    /// workspace's entity-level drag handler. Persists across render cycles.
    pending_drag_init: std::sync::Arc<std::sync::Mutex<Option<(usize, f32)>>>,
    /// Agent panel drag state: start X position and start width when drag began.
    agent_panel_drag: Option<(f32, f32)>,
    /// Current terminal color theme
    terminal_theme: TerminalTheme,
    /// Shared Ghostty app instance for all panes in this window.
    ghostty_app: std::sync::Arc<con_ghostty::GhosttyApp>,
}

// ── Theme conversion ──────────────────────────────────────────

/// Convert con's TerminalTheme to ghostty's TerminalColors.
fn theme_to_ghostty_colors(theme: &TerminalTheme) -> con_ghostty::TerminalColors {
    let mut palette = [[0u8; 3]; 16];
    for (i, c) in theme.ansi.iter().enumerate() {
        palette[i] = [c.r, c.g, c.b];
    }
    con_ghostty::TerminalColors {
        foreground: [theme.foreground.r, theme.foreground.g, theme.foreground.b],
        background: [theme.background.r, theme.background.g, theme.background.b],
        palette,
    }
}

// ── Terminal factory functions ────────────────────────────────
//
// Standalone so they can be called both during ConWorkspace::new()
// (before `self` exists) and from create_terminal() (after).

fn make_ghostty_terminal(
    app: &std::sync::Arc<con_ghostty::GhosttyApp>,
    cwd: Option<&str>,
    font_size: f32,
    window: &mut Window,
    cx: &mut Context<ConWorkspace>,
) -> TerminalPane {
    let app = app.clone();
    let cwd = cwd.map(str::to_string);
    let view = cx.new(|cx| crate::ghostty_view::GhosttyView::new(app, cwd, font_size, cx));
    let pane = TerminalPane::new(view);
    subscribe_terminal_pane(&pane, window, cx);
    pane
}

impl ConWorkspace {
    pub fn new(config: Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sidebar = cx.new(|cx| SessionSidebar::new(cx));
        let font_size = config.terminal.font_size;
        let terminal_theme = TerminalTheme::by_name(&config.terminal.theme).unwrap_or_default();
        let session = Session::load().unwrap_or_default();
        let colors = theme_to_ghostty_colors(&terminal_theme);
        let ghostty_app = con_ghostty::GhosttyApp::new(Some(&colors), Some(font_size))
            .map(std::sync::Arc::new)
            .unwrap_or_else(|e| panic!("Fatal: failed to initialize Ghostty: {}", e));
        let harness = AgentHarness::new(&config).unwrap_or_else(|e| {
            log::error!(
                "Failed to create agent harness: {}. Agent features disabled.",
                e
            );
            panic!("Fatal: agent harness initialization failed: {}", e);
        });
        let model_registry = ModelRegistry::new();
        if model_registry.needs_refresh() {
            let registry_for_fetch = model_registry.clone();
            harness.spawn_detached(async move {
                if let Err(e) = registry_for_fetch.fetch().await {
                    log::warn!("Failed to refresh model registry: {}", e);
                }
            });
        }

        let make_terminal =
            |cwd: Option<&str>, window: &mut Window, cx: &mut Context<Self>| -> TerminalPane {
                make_ghostty_terminal(&ghostty_app, cwd, font_size, window, cx)
            };

        let mut tabs: Vec<Tab> = session
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab_state)| {
                let cwd = tab_state.cwd.as_deref();
                let terminal = make_terminal(cwd, window, cx);
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
            let terminal = make_terminal(None, window, cx);
            tabs.push(Tab {
                pane_tree: PaneTree::new(terminal),
                title: "Terminal".to_string(),
                needs_attention: false,
                session: AgentSession::new(),
                panel_state: PanelState::new(),
            });
        }
        let active_tab = session.active_tab.min(tabs.len() - 1);
        let agent_panel_open = session.agent_panel_open;
        let agent_panel_width = session
            .agent_panel_width
            .unwrap_or(AGENT_PANEL_DEFAULT_WIDTH);
        // Take the active tab's restored panel state for the AgentPanel
        let initial_panel_state =
            std::mem::replace(&mut tabs[active_tab].panel_state, PanelState::new());
        let agent_panel = cx.new(|cx| {
            let mut panel = AgentPanel::with_state(initial_panel_state, cx);
            panel.set_auto_approve(config.agent.auto_approve_tools);
            panel
        });
        let input_bar = cx.new(|cx| InputBar::new(window, cx));
        let registry = model_registry.clone();
        let settings_panel = cx.new(|cx| SettingsPanel::new(&config, registry, window, cx));
        let command_palette = cx.new(|cx| CommandPalette::new(window, cx));
        cx.subscribe_in(&input_bar, window, Self::on_input_submit)
            .detach();
        cx.subscribe_in(&input_bar, window, Self::on_input_escape)
            .detach();
        cx.subscribe_in(&input_bar, window, Self::on_skill_autocomplete_changed)
            .detach();
        cx.subscribe_in(&settings_panel, window, Self::on_settings_saved)
            .detach();
        cx.subscribe_in(&settings_panel, window, Self::on_theme_preview)
            .detach();
        // Re-render workspace when settings panel visibility changes (e.g. X close button)
        cx.observe(&settings_panel, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(&command_palette, window, Self::on_palette_select)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_new_conversation)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_load_conversation)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_delete_conversation)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_inline_input_submit)
            .detach();
        cx.subscribe_in(
            &agent_panel,
            window,
            Self::on_inline_skill_autocomplete_changed,
        )
        .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_cancel_request)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_enable_auto_approve)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_rerun_from_message)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_select)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_new_session)
            .detach();

        // Poll all tabs' agent sessions.
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
                        while let Ok(req) = workspace.tabs[tab_idx]
                            .session
                            .terminal_exec_requests()
                            .try_recv()
                        {
                            got_event = true;
                            workspace.handle_terminal_exec_request_for_tab(tab_idx, req, cx);
                        }

                        // Pane queries — route to the tab that owns the session
                        while let Ok(req) =
                            workspace.tabs[tab_idx].session.pane_requests().try_recv()
                        {
                            got_event = true;
                            workspace.handle_pane_request_for_tab(tab_idx, req, cx);
                        }
                    }
                })
                .ok();

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

        // Hide non-active tabs' ghostty NSViews so only the active tab is visible
        for (i, tab) in tabs.iter().enumerate() {
            if i != active_tab {
                for terminal in tab.pane_tree.all_terminals() {
                    terminal.set_native_view_visible(false, cx);
                }
            }
        }

        Self {
            sidebar,
            tabs,
            active_tab,
            font_size,
            agent_panel,
            input_bar,
            settings_panel,
            command_palette,
            harness,
            agent_panel_open,
            agent_panel_width,
            input_bar_visible: session.input_bar_visible,
            modal_was_open: false,
            ghostty_hidden: false,
            pending_drag_init: std::sync::Arc::new(std::sync::Mutex::new(None)),
            agent_panel_drag: None,
            terminal_theme,
            ghostty_app,
        }
    }

    /// Create a new Ghostty terminal pane.
    fn create_terminal(
        &mut self,
        cwd: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TerminalPane {
        make_ghostty_terminal(&self.ghostty_app, cwd, self.font_size, window, cx)
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
        let focused_observation = focused.observation_frame(50, cx);
        let focused_runtime =
            con_agent::context::PaneRuntimeState::from_observation(&focused_observation);

        let mut other_pane_summaries = Vec::new();
        if pane_tree.pane_count() > 1 {
            for (idx, terminal) in all_terminals.iter().enumerate() {
                if let Some(pid) = pane_tree.pane_id_for_terminal(terminal) {
                    if pid == focused_pid {
                        continue;
                    }
                    let observation = terminal.observation_frame(10, cx);
                    let runtime =
                        con_agent::context::PaneRuntimeState::from_observation(&observation);
                    other_pane_summaries.push(con_agent::context::PaneSummary {
                        pane_index: idx + 1,
                        hostname: runtime.remote_host.clone(),
                        hostname_confidence: runtime.remote_host_confidence,
                        hostname_source: runtime.remote_host_source,
                        title: observation.title.clone(),
                        mode: runtime.mode,
                        has_shell_integration: observation.has_shell_integration,
                        shell_metadata_fresh: runtime.shell_metadata_fresh,
                        runtime_stack: runtime.scope_stack,
                        runtime_warnings: runtime.warnings,
                        tmux_session: runtime.tmux_session,
                        cwd: observation.cwd,
                        last_command: observation.last_command,
                        last_exit_code: observation.last_exit_code,
                        is_busy: observation.is_busy,
                        recent_output: observation.recent_output,
                    });
                }
            }
        }

        self.harness.build_context_from_snapshot(
            focused_pane_index,
            &focused_observation,
            &focused_runtime,
            other_pane_summaries,
        )
    }

    fn panel_state_from_conversation(&self, conv: &Conversation) -> PanelState {
        let mut state = PanelState::new();
        for msg in &conv.messages {
            match msg.role {
                con_agent::MessageRole::User => {
                    let visible = self
                        .harness
                        .display_label_for_user_message(&msg.content)
                        .unwrap_or_else(|| msg.content.clone());
                    state.add_message("user", &visible);
                }
                con_agent::MessageRole::Assistant => {
                    state.add_message("assistant", &msg.content);
                }
                con_agent::MessageRole::System | con_agent::MessageRole::Tool => {
                    state.add_message("system", &msg.content);
                }
            }
        }
        state
    }

    fn save_session(&self, cx: &App) {
        let tabs: Vec<con_core::session::TabState> = self
            .tabs
            .iter()
            .map(|tab| {
                let terminal = tab.pane_tree.focused_terminal();
                let cwd = terminal.current_dir(cx);
                let title = terminal.title(cx).unwrap_or_else(|| tab.title.clone());
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
            input_bar_visible: self.input_bar_visible,
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
        if self.tabs[self.active_tab]
            .session
            .load_conversation(&event.id)
        {
            // Rebuild panel state from the loaded conversation
            let conv = self.tabs[self.active_tab].session.conversation();
            let conv = conv.lock();
            let new_state = self.panel_state_from_conversation(&conv);
            drop(conv);
            self.agent_panel.update(cx, |panel, cx| {
                panel.swap_state(new_state, cx);
            });
            self.save_session(cx);
        }
    }

    fn on_delete_conversation(
        &mut self,
        _panel: &Entity<AgentPanel>,
        event: &DeleteConversation,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(e) = con_agent::Conversation::delete(&event.id) {
            log::warn!("Failed to delete conversation: {}", e);
        }
        // Refresh the conversation list in the agent panel
        self.agent_panel.update(cx, |panel, cx| {
            panel.refresh_conversation_list(cx);
        });
    }

    fn on_inline_input_submit(
        &mut self,
        _panel: &Entity<AgentPanel>,
        event: &InlineInputSubmit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Forward inline input as an agent message
        self.send_to_agent(&event.text, cx);
        cx.notify();
    }

    fn on_inline_skill_autocomplete_changed(
        &mut self,
        _panel: &Entity<AgentPanel>,
        _event: &InlineSkillAutocompleteChanged,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
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

    fn on_rerun_from_message(
        &mut self,
        _panel: &Entity<AgentPanel>,
        event: &RerunFromMessage,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The panel already truncated its messages and re-added the user message.
        // Now sync the underlying conversation and re-send to agent.
        let panel_msg_count = self.agent_panel.read(cx).state().message_count();
        // Truncate conversation to match panel state (minus the re-added user message)
        let conv = self.tabs[self.active_tab].session.conversation();
        conv.lock().truncate_to(panel_msg_count.saturating_sub(1));

        let context = self.build_agent_context(cx);
        let session = &self.tabs[self.active_tab].session;
        if event.content.trim().starts_with('/') {
            match self.harness.classify_input(
                &event.content,
                self.active_terminal().detected_remote_host(cx).is_some(),
            ) {
                InputKind::SkillInvoke(name, args) => {
                    if let Some(desc) =
                        self.harness
                            .invoke_skill(session, &name, args.as_deref(), context)
                    {
                        self.agent_panel.update(cx, |panel, cx| {
                            panel.add_step(&desc, cx);
                        });
                    }
                }
                _ => self
                    .harness
                    .send_message(session, event.content.clone(), context),
            }
        } else {
            self.harness
                .send_message(session, event.content.clone(), context);
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
            "toggle-input-bar" => {
                self.toggle_input_bar(&crate::ToggleInputBar, window, cx);
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
                self.cancel_all_sessions();
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
        let auto_approve = new_config.auto_approve_tools;
        self.harness.update_config(new_config);

        // Sync auto-approve to agent panel UI
        self.agent_panel.update(cx, |panel, _cx| {
            panel.set_auto_approve(auto_approve);
        });

        // Apply updated skills paths (forces rescan on next cwd check)
        let skills_config = settings.read(cx).skills_config().clone();
        self.harness.update_skills_config(skills_config);
        if let Some(cwd) = self.active_terminal().current_dir(cx) {
            self.harness.scan_skills(&cwd);
        }

        let term_config = settings.read(cx).terminal_config().clone();
        self.font_size = term_config.font_size;

        if let Some(new_theme) = TerminalTheme::by_name(&term_config.theme) {
            self.apply_terminal_theme(new_theme, window, cx);
        } else {
            log::warn!(
                "Skipping terminal theme sync; theme {:?} was not found",
                term_config.theme
            );
        }

        // Re-apply keybindings at runtime so changes take effect immediately
        let kb = settings.read(cx).keybinding_config().clone();
        cx.bind_keys([
            KeyBinding::new(&kb.quit, crate::Quit, None),
            KeyBinding::new(&kb.new_tab, crate::NewTab, None),
            KeyBinding::new(&kb.toggle_agent, crate::ToggleAgentPanel, None),
            KeyBinding::new(&kb.close_tab, crate::CloseTab, None),
            KeyBinding::new(&kb.settings, settings_panel::ToggleSettings, None),
            KeyBinding::new(
                &kb.command_palette,
                crate::command_palette::ToggleCommandPalette,
                None,
            ),
            KeyBinding::new(&kb.split_right, crate::SplitRight, None),
            KeyBinding::new(&kb.split_down, crate::SplitDown, None),
            KeyBinding::new(&kb.focus_input, crate::FocusInput, None),
            KeyBinding::new(&kb.toggle_input_bar, crate::ToggleInputBar, None),
        ]);

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
                self.apply_terminal_theme(new_theme, window, cx);
                cx.notify();
            }
        }
    }

    /// Apply a new terminal theme to all panes and sync UI mode.
    fn apply_terminal_theme(
        &mut self,
        theme: TerminalTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.terminal_theme = theme.clone();
        let colors = theme_to_ghostty_colors(&theme);
        // Update all terminal panes (legacy gets full theme, ghostty gets color scheme)
        for tab in &self.tabs {
            for terminal in tab.pane_tree.all_terminals() {
                terminal.set_theme(&theme, &colors, self.font_size, cx);
            }
        }
        if let Err(e) = self.ghostty_app.update_appearance(&colors, self.font_size) {
            log::error!("Failed to update Ghostty appearance: {}", e);
        }
        // Sync GPUI UI theme colors with terminal theme
        crate::theme::sync_gpui_theme(&theme, window, cx);
    }

    fn on_input_escape(
        &mut self,
        _input_bar: &Entity<InputBar>,
        _event: &EscapeInput,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
    }

    fn on_skill_autocomplete_changed(
        &mut self,
        _input_bar: &Entity<InputBar>,
        _event: &SkillAutocompleteChanged,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.notify();
    }

    fn on_input_submit(
        &mut self,
        input_bar: &Entity<InputBar>,
        _event: &SubmitInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (content, mode) =
            input_bar.update(cx, |bar, cx| (bar.take_content(window, cx), bar.mode()));

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
                        if let Some(desc) =
                            self.harness
                                .invoke_skill(session, &name, args.as_deref(), context)
                        {
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
                if self.harness.config().auto_approve_tools {
                    // Auto-approved: send approval decision, show as regular tool call
                    let _ = approval_tx.send(con_agent::ToolApprovalDecision {
                        call_id: call_id.clone(),
                        allowed: true,
                        reason: Some("auto-approved".into()),
                    });
                    self.agent_panel.update(cx, |panel, cx| {
                        panel.add_tool_call(&call_id, &tool_name, &args, cx);
                    });
                } else {
                    self.agent_panel.update(cx, |panel, cx| {
                        panel.add_pending_approval(&call_id, &tool_name, &args, approval_tx, cx);
                    });
                }
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
                    panel.complete_response(&msg, cx);
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
    /// Uses Ghostty's COMMAND_FINISHED signal when available, with a bounded
    /// recent-output fallback when shell integration is unavailable.
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
                 Command completion tracking may produce unexpected results."
            );
        }

        // Write the command to the PTY — user sees it execute in real time
        let cmd_with_newline = format!("{}\n", req.command);
        pane.write(cmd_with_newline.as_bytes(), cx);

        let fallback_response_tx = req.response_tx;
        let pane_for_fallback = pane.clone();
        cx.spawn(async move |_this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(500))
                .await;

            for _ in 0..29 {
                let finished = _this
                    .update(cx, |_ws, cx| pane_for_fallback.take_command_finished(cx))
                    .ok()
                    .flatten();

                if let Some((exit_code, _duration)) = finished {
                    let output = _this
                        .update(cx, |_ws, cx| {
                            pane_for_fallback.recent_lines(50, cx).join("\n")
                        })
                        .unwrap_or_default();
                    let _ =
                        fallback_response_tx.try_send(TerminalExecResponse { output, exit_code });
                    return;
                }

                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
            }

            let output = _this
                .update(cx, |_ws, cx| {
                    pane_for_fallback.recent_lines(50, cx).join("\n")
                })
                .unwrap_or_default();
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
                        let pid = pane_tree.pane_id_for_terminal(terminal).unwrap_or(idx);
                        let observation = terminal.observation_frame(12, cx);
                        let runtime =
                            con_agent::context::PaneRuntimeState::from_observation(&observation);
                        let title = observation
                            .title
                            .clone()
                            .unwrap_or_else(|| format!("Pane {}", idx + 1));
                        let (cols, rows) = terminal.grid_size(cx);
                        PaneInfo {
                            index: idx + 1,
                            title,
                            cwd: observation.cwd.clone(),
                            is_focused: pid == focused_pid,
                            rows,
                            cols,
                            is_alive: terminal.is_alive(cx),
                            hostname: runtime.remote_host.clone(),
                            hostname_confidence: runtime.remote_host_confidence,
                            hostname_source: runtime.remote_host_source,
                            mode: runtime.mode,
                            shell_metadata_fresh: runtime.shell_metadata_fresh,
                            runtime_stack: runtime.scope_stack,
                            runtime_warnings: runtime.warnings,
                            tmux_session: runtime.tmux_session,
                            has_shell_integration: observation.has_shell_integration,
                            last_command: observation.last_command,
                            last_exit_code: observation.last_exit_code,
                            is_busy: observation.is_busy,
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

    fn toggle_input_bar(
        &mut self,
        _: &crate::ToggleInputBar,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.input_bar_visible = !self.input_bar_visible;
        if self.input_bar_visible {
            self.input_bar.focus_handle(cx).focus(window, cx);
        } else if self.agent_panel_open {
            let focused_inline = self
                .agent_panel
                .update(cx, |panel, cx| panel.focus_inline_input(window, cx));
            if !focused_inline {
                self.active_terminal().focus(window, cx);
            }
        } else {
            self.active_terminal().focus(window, cx);
        }
        self.save_session(cx);
        cx.notify();
    }

    fn quit(&mut self, _: &Quit, _window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_all_sessions();
        self.save_session(cx);
        // Tear down ghostty surfaces before app exit to avoid Metal/NSView crashes.
        // Hide views and unfocus first, then clear the tabs vector so GhosttyTerminal
        // Drop runs (calling ghostty_surface_free) before cx.quit() exits the process.
        for tab in &self.tabs {
            for t in tab.pane_tree.all_terminals() {
                t.set_focus_state(false, cx);
                t.set_native_view_visible(false, cx);
            }
        }
        self.tabs.clear();
        cx.quit();
    }

    fn focus_input(&mut self, _: &FocusInput, window: &mut Window, cx: &mut Context<Self>) {
        if !self.input_bar_visible {
            self.input_bar_visible = true;
            self.save_session(cx);
            cx.notify();
        }
        self.input_bar.focus_handle(cx).focus(window, cx);
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
        self.settings_panel.read(cx).is_visible() || self.command_palette.read(cx).is_visible()
    }

    fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        let terminal = self.create_terminal(None, window, cx);
        let tab_number = self.tabs.len() + 1;
        let old_active = self.active_tab;

        // Hide old tab's ghostty NSViews and unfocus surfaces
        for t in self.tabs[old_active].pane_tree.all_terminals() {
            t.set_native_view_visible(false, cx);
            t.set_focus_state(false, cx);
        }

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
        let outgoing = self
            .agent_panel
            .update(cx, |panel, cx| panel.swap_state(incoming, cx));
        self.tabs[old_active].panel_state = outgoing;

        terminal.set_focus_state(true, cx);
        terminal.focus(window, cx);
        self.save_session(cx);
        cx.notify();
    }

    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        // If the active tab has multiple panes, close the focused pane first.
        // Only close the entire tab when it's down to a single pane.
        let tab = &mut self.tabs[self.active_tab];
        if tab.pane_tree.pane_count() > 1 {
            // Hide the ghostty NSView of the pane being closed
            let closing = tab.pane_tree.focused_terminal();
            closing.set_native_view_visible(false, cx);
            closing.set_focus_state(false, cx);

            tab.pane_tree.close_focused();

            // Focus the new focused pane
            let new_focus = tab.pane_tree.focused_terminal();
            new_focus.set_focus_state(true, cx);
            new_focus.focus(window, cx);
            cx.notify();
            return;
        }
        self.close_tab_by_index(self.active_tab, window, cx);
    }

    fn close_tab_by_index(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 || index >= self.tabs.len() {
            return;
        }
        // Save the closing tab's conversation
        {
            let conv = self.tabs[index].session.conversation();
            let _ = conv.lock().save();
        }
        // Hide closing tab's ghostty NSViews
        for t in self.tabs[index].pane_tree.all_terminals() {
            t.set_native_view_visible(false, cx);
            t.set_focus_state(false, cx);
        }
        let was_active = index == self.active_tab;
        self.tabs.remove(index);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if self.active_tab > index {
            self.active_tab -= 1;
        }
        // Swap new active tab's panel state into the panel if needed
        if was_active {
            let incoming = std::mem::replace(
                &mut self.tabs[self.active_tab].panel_state,
                PanelState::new(),
            );
            self.agent_panel.update(cx, |panel, cx| {
                panel.swap_state(incoming, cx);
            });
        }
        // Show and focus new active tab's ghostty views
        for t in self.tabs[self.active_tab].pane_tree.all_terminals() {
            t.set_native_view_visible(true, cx);
        }
        let focused = self.tabs[self.active_tab].pane_tree.focused_terminal();
        focused.set_focus_state(true, cx);
        focused.focus(window, cx);
        self.sync_sidebar(cx);
        self.save_session(cx);
        cx.notify();
    }

    fn execute_shell(&self, cmd: &str, window: &mut Window, cx: &mut Context<Self>) {
        let target_ids = self.input_bar.read(cx).target_pane_ids();
        let pane_tree = &self.tabs[self.active_tab].pane_tree;
        let all_terminals = pane_tree.all_terminals();

        for terminal in &all_terminals {
            if all_terminals.len() == 1
                || target_ids
                    .iter()
                    .any(|&tid| pane_tree.terminal_has_pane_id(terminal, tid))
            {
                terminal.write(format!("{}\n", cmd).as_bytes(), cx);
            }
        }

        // Always keep focus on input bar after sending a command —
        // the terminal output is visible, and the user can click to focus it.
        self.input_bar.focus_handle(cx).focus(window, cx);
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
        self.harness
            .send_message(session, content.to_string(), context);
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

    fn split_right(&mut self, _: &SplitRight, window: &mut Window, cx: &mut Context<Self>) {
        self.split_pane(SplitDirection::Horizontal, window, cx);
    }

    fn split_down(&mut self, _: &SplitDown, window: &mut Window, cx: &mut Context<Self>) {
        self.split_pane(SplitDirection::Vertical, window, cx);
    }

    fn activate_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() || index == self.active_tab {
            return;
        }
        let old_active = self.active_tab;

        // Take the incoming tab's panel state
        let incoming = std::mem::replace(&mut self.tabs[index].panel_state, PanelState::new());
        // Swap into the panel, get the outgoing state back
        let outgoing = self
            .agent_panel
            .update(cx, |panel, cx| panel.swap_state(incoming, cx));
        // Stash outgoing state into the old tab
        self.tabs[old_active].panel_state = outgoing;

        // Hide old tab's ghostty NSViews and unfocus surfaces
        for terminal in self.tabs[old_active].pane_tree.all_terminals() {
            terminal.set_native_view_visible(false, cx);
            terminal.set_focus_state(false, cx);
        }

        self.active_tab = index;
        self.tabs[index].needs_attention = false;

        // Show new tab's ghostty NSViews and focus active surface
        for terminal in self.tabs[index].pane_tree.all_terminals() {
            terminal.set_native_view_visible(true, cx);
        }
        let focused = self.tabs[index].pane_tree.focused_terminal();
        focused.set_focus_state(true, cx);
        focused.focus(window, cx);

        self.save_session(cx);
        cx.notify();
    }

    /// Focus the active terminal (used after modal close, etc.)
    fn focus_terminal(&self, window: &mut Window, cx: &mut App) {
        self.active_terminal().focus(window, cx);
    }

    /// Cancel all pending agent operations across all tabs.
    /// Must be called before cx.quit() to prevent shutdown hang.
    fn cancel_all_sessions(&self) {
        for tab in &self.tabs {
            tab.session.cancel_current();
        }
    }

    /// Show or hide ghostty NSViews for z-order management.
    /// When showing, only the active tab's views are made visible.
    /// When hiding, all views are hidden (for modal overlays).
    fn set_ghostty_views_visible(&self, visible: bool, cx: &App) {
        if visible {
            // Only show the active tab's views
            for terminal in self.tabs[self.active_tab].pane_tree.all_terminals() {
                terminal.set_native_view_visible(true, cx);
            }
        } else {
            // Hide all tabs' views for modal z-order
            for tab in &self.tabs {
                for terminal in tab.pane_tree.all_terminals() {
                    terminal.set_native_view_visible(false, cx);
                }
            }
        }
    }

    // ── Ghostty event handlers ──────────────────────────────

    pub(crate) fn on_terminal_focus_changed(
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

    pub(crate) fn on_terminal_process_exited(
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
        } else {
            // Last pane in last tab — quit the app
            self.cancel_all_sessions();
            cx.quit();
        }
        cx.notify();
    }

    pub(crate) fn on_terminal_title_changed(
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
        let has_skill_popup = !self.input_bar.read(cx).filtered_skills(cx).is_empty();
        let has_inline_skill_popup = self.agent_panel_open
            && !self.input_bar_visible
            && !self
                .agent_panel
                .read(cx)
                .filtered_inline_skills(cx)
                .is_empty();
        let needs_ghostty_hidden = is_modal_open || has_skill_popup || has_inline_skill_popup;

        if self.modal_was_open && !is_modal_open {
            self.focus_terminal(window, cx);
        }
        // Manage ghostty NSView visibility separately — hide for modals AND skill popup
        if needs_ghostty_hidden && !self.ghostty_hidden {
            self.set_ghostty_views_visible(false, cx);
            self.ghostty_hidden = true;
        } else if !needs_ghostty_hidden && self.ghostty_hidden {
            self.set_ghostty_views_visible(true, cx);
            self.ghostty_hidden = false;
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
                PaneInfo {
                    id,
                    name,
                    hostname,
                    is_busy,
                    is_alive,
                }
            })
            .collect();

        let cwd = active_terminal.current_dir(cx);
        // Scan skills when cwd changes (project-local + global ~/.config/con/skills/)
        if let Some(ref raw_cwd) = cwd {
            self.harness.scan_skills(raw_cwd);
        }
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

        let skill_entries: Vec<crate::input_bar::SkillEntry> = self
            .harness
            .skill_summaries()
            .into_iter()
            .map(|(name, desc)| crate::input_bar::SkillEntry {
                name,
                description: desc,
            })
            .collect();
        self.input_bar.update(cx, |bar, _cx| {
            bar.set_panes(pane_infos, focused_pane_id);
            bar.set_cwd(display_cwd);
            bar.set_skills(skill_entries);
        });

        // Sync model name, inline input, and skills to agent panel
        let model_name = self.harness.active_model_name();
        let show_inline = !self.input_bar_visible && self.agent_panel_open;
        let panel_skills: Vec<crate::input_bar::SkillEntry> = self
            .harness
            .skill_summaries()
            .into_iter()
            .map(|(name, desc)| crate::input_bar::SkillEntry {
                name,
                description: desc,
            })
            .collect();
        self.agent_panel.update(cx, |panel, _cx| {
            panel.set_model_name(model_name);
            panel.set_show_inline_input(show_inline);
            panel.set_skills(panel_skills);
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

        let mut main_area = div().flex().flex_1().min_h_0().child(terminal_area);

        if self.agent_panel_open {
            // Draggable divider — matches pane divider style
            main_area = main_area
                .child(
                    div()
                        .id("agent-panel-divider")
                        .w(px(6.0))
                        .h_full()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_col_resize()
                        .bg(theme.title_bar)
                        .hover(|s| s.bg(theme.primary.opacity(0.08)))
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

        // Tab bar — flat, borderless, macOS-native feel
        let tab_count = self.tabs.len();
        let mut tab_bar = div()
            .id("tab-bar")
            .flex()
            .h(px(38.0))
            .bg(theme.title_bar)
            .items_end()
            .pl(px(78.0)) // leave room for traffic lights
            .pr(px(8.0))
            .gap(px(0.0))
            .on_click(|event, window, _cx| {
                if event.click_count() == 2 {
                    window.titlebar_double_click();
                }
            });

        for (index, tab) in self.tabs.iter().enumerate() {
            let is_active = index == self.active_tab;
            let needs_attention = tab.needs_attention && !is_active;
            let terminal = tab.pane_tree.focused_terminal();
            let title = terminal.title(cx).unwrap_or_else(|| tab.title.clone());

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
                let mut close_el = div()
                    .id(close_id)
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(18.0))
                    .flex_shrink_0()
                    .rounded(px(4.0))
                    .ml(px(2.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.muted.opacity(0.25)));
                // Only show on hover for inactive tabs
                if !is_active {
                    close_el = close_el.invisible().group_hover("tab", |s| s.visible());
                }
                Some(
                    close_el
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                this.close_tab_by_index(index, window, cx);
                            }),
                        )
                        .child(
                            svg()
                                .path("phosphor/x.svg")
                                .size(px(12.0))
                                .text_color(theme.muted_foreground),
                        ),
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
                .h(px(32.0))
                .text_size(px(12.5))
                .max_w(px(200.0))
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.activate_tab(index, window, cx);
                }))
                // Middle-click to close tab (browser convention)
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |this, _, window, cx| {
                        this.close_tab_by_index(index, window, cx);
                    }),
                );

            if is_active {
                // Active tab: rounded top corners, connects flush to content area below
                tab_el = tab_el
                    .rounded_t(px(8.0))
                    .bg(theme.background)
                    .text_color(theme.foreground)
                    .font_weight(FontWeight::MEDIUM);
            } else {
                // Inactive tab: fully rounded, subtle hover
                tab_el = tab_el
                    .rounded(px(6.0))
                    .mb(px(2.0))
                    .text_color(theme.muted_foreground.opacity(0.55))
                    .hover(|s| s.bg(theme.muted.opacity(0.08)));
            }

            let mut tab_content = div().flex().items_center().gap(px(6.0)).w_full().min_w_0();

            // Attention dot for tabs with pending agent activity
            if needs_attention {
                tab_content = tab_content.child(
                    div()
                        .size(px(6.0))
                        .rounded_full()
                        .flex_shrink_0()
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

            // Title — flexible, truncates with overflow
            tab_content = tab_content.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_x_hidden()
                    .whitespace_nowrap()
                    .child(display_title),
            );

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
                .size(px(26.0))
                .mb(px(3.0))
                .ml(px(4.0))
                .rounded(px(6.0))
                .cursor_pointer()
                .text_color(theme.muted_foreground)
                .hover(|s| s.bg(theme.muted.opacity(0.15)).text_color(theme.foreground))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.new_tab(&NewTab, window, cx);
                }))
                .child(
                    svg()
                        .path("phosphor/plus.svg")
                        .size(px(14.0))
                        .text_color(theme.muted_foreground),
                ),
        );

        // Spacer pushes right-side controls to the edge
        tab_bar = tab_bar.child(div().flex_1());

        // Input bar toggle button
        tab_bar = tab_bar.child(
            div()
                .id("toggle-input-bar")
                .flex()
                .items_center()
                .justify_center()
                .size(px(26.0))
                .mb(px(3.0))
                .rounded(px(6.0))
                .cursor_pointer()
                .hover(|s| s.bg(theme.muted.opacity(0.15)).text_color(theme.foreground))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_input_bar(&crate::ToggleInputBar, window, cx);
                }))
                .child(
                    svg()
                        .path("phosphor/square-half-bottom-fill.svg")
                        .size(px(14.0))
                        .text_color(if self.input_bar_visible {
                            theme.primary
                        } else {
                            theme.muted_foreground
                        }),
                ),
        );

        // Agent panel toggle button
        tab_bar = tab_bar.child(
            div()
                .id("toggle-agent-panel")
                .flex()
                .items_center()
                .justify_center()
                .size(px(26.0))
                .mb(px(3.0))
                .mr(px(4.0))
                .rounded(px(6.0))
                .cursor_pointer()
                .hover(|s| s.bg(theme.muted.opacity(0.15)).text_color(theme.foreground))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_agent_panel(&ToggleAgentPanel, window, cx);
                }))
                .child(
                    svg()
                        .path("phosphor/square-half-fill.svg")
                        .size(px(14.0))
                        .text_color(if self.agent_panel_open {
                            theme.primary
                        } else {
                            theme.muted_foreground
                        }),
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
                        let new_width = (start_width + delta)
                            .clamp(AGENT_PANEL_MIN_WIDTH, AGENT_PANEL_MAX_WIDTH);
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
                                    let panel_w = if this.agent_panel_open {
                                        this.agent_panel_width + 7.0
                                    } else {
                                        0.0
                                    };
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
            .on_action(cx.listener(Self::quit))
            .on_action(cx.listener(Self::toggle_agent_panel))
            .on_action(cx.listener(Self::toggle_input_bar))
            .on_action(cx.listener(Self::toggle_settings))
            .on_action(cx.listener(Self::toggle_command_palette))
            .on_action(cx.listener(Self::new_tab))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::split_right))
            .on_action(cx.listener(Self::split_down))
            .on_action(cx.listener(Self::focus_input))
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
            .child(main_area);

        if self.input_bar_visible {
            root = root.child(self.input_bar.clone());
        }

        // Skill autocomplete popup — rendered at workspace level above ghostty
        if has_skill_popup {
            let theme = cx.theme();
            let popup_width = px((window.bounds().size.width.as_f32() * 0.34).clamp(320.0, 480.0));
            let popup_bottom = self.input_bar.read(cx).skill_popup_offset(cx);
            let skills = self
                .input_bar
                .read(cx)
                .filtered_skills(cx)
                .into_iter()
                .map(|s| (s.name.clone(), s.description.clone()))
                .collect::<Vec<_>>();
            let sel = self.input_bar.read(cx).skill_selection();
            let sel = sel.min(skills.len().saturating_sub(1));

            let mut popup = div()
                .absolute()
                .bottom(popup_bottom)
                .left(px(24.0))
                .w(popup_width)
                .max_h(px(320.0))
                .flex()
                .flex_col()
                .rounded(px(10.0))
                .bg(theme.background.opacity(0.98))
                .border_1()
                .border_color(theme.muted.opacity(0.16))
                .py(px(6.0))
                .overflow_hidden()
                .font_family(".SystemUIFont");

            for (i, (name, desc)) in skills.iter().enumerate() {
                let is_sel = i == sel;
                let name_clone = name.clone();
                popup = popup.child(
                    div()
                        .id(SharedString::from(format!("skill-{name}")))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .mx(px(6.0))
                        .px(px(12.0))
                        .py(px(8.0))
                        .rounded(px(8.0))
                        .cursor_pointer()
                        .bg(if is_sel {
                            theme.primary.opacity(0.10)
                        } else {
                            theme.transparent
                        })
                        .hover(|s| s.bg(theme.primary.opacity(0.08)))
                        .on_mouse_down(MouseButton::Left, {
                            let input_bar = self.input_bar.clone();
                            cx.listener(move |_this, _, window, cx| {
                                input_bar.update(cx, |bar, cx| {
                                    bar.complete_skill(&name_clone, window, cx);
                                });
                            })
                        })
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if is_sel {
                                    theme.primary
                                } else {
                                    theme.foreground
                                })
                                .child(format!("/{name}")),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .line_height(px(16.0))
                                .text_color(theme.muted_foreground.opacity(0.68))
                                .child(desc.clone()),
                        ),
                );
            }
            root = root.child(popup);
        }

        // Inline skill popup — rendered above the agent panel's inline input
        if has_inline_skill_popup {
            let theme = cx.theme();
            let popup_bottom = self.agent_panel.read(cx).inline_skill_popup_offset(cx);
            let skills = self
                .agent_panel
                .read(cx)
                .filtered_inline_skills(cx)
                .into_iter()
                .map(|s| (s.name.clone(), s.description.clone()))
                .collect::<Vec<_>>();
            let sel = self.agent_panel.read(cx).inline_skill_selection();
            let sel = sel.min(skills.len().saturating_sub(1));

            let mut popup = div()
                .absolute()
                .bottom(popup_bottom)
                .left(px(24.0))
                .w(px(260.0))
                .max_h(px(280.0))
                .flex()
                .flex_col()
                .rounded(px(10.0))
                .bg(theme.background.opacity(0.98))
                .border_1()
                .border_color(theme.muted.opacity(0.16))
                .py(px(6.0))
                .overflow_hidden()
                .font_family(".SystemUIFont");

            for (i, (name, desc)) in skills.iter().enumerate() {
                let is_sel = i == sel;
                let name_clone = name.clone();
                popup = popup.child(
                    div()
                        .id(SharedString::from(format!("inline-skill-{name}")))
                        .flex()
                        .flex_col()
                        .gap(px(2.0))
                        .mx(px(6.0))
                        .px(px(12.0))
                        .py(px(8.0))
                        .rounded(px(8.0))
                        .cursor_pointer()
                        .bg(if is_sel {
                            theme.primary.opacity(0.10)
                        } else {
                            theme.transparent
                        })
                        .hover(|s| s.bg(theme.primary.opacity(0.08)))
                        .on_mouse_down(MouseButton::Left, {
                            let agent_panel = self.agent_panel.clone();
                            cx.listener(move |_this, _, window, cx| {
                                agent_panel.update(cx, |panel, cx| {
                                    panel.complete_inline_skill(&name_clone, window, cx);
                                });
                            })
                        })
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(if is_sel {
                                    theme.primary
                                } else {
                                    theme.foreground
                                })
                                .child(format!("/{name}")),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .line_height(px(16.0))
                                .text_color(theme.muted_foreground.opacity(0.68))
                                .child(desc.clone()),
                        ),
                );
            }
            root = root.child(popup);
        }

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
                if after.is_empty() {
                    before.to_string()
                } else {
                    shorten_path(after)
                }
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
        ) && path
            .parent()
            .and_then(|p| p.parent())
            .map_or(false, |pp| pp.parent().is_none());

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
