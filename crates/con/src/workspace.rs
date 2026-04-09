use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

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
    runtime_trackers: RefCell<HashMap<usize, con_agent::context::PaneRuntimeTracker>>,
}

/// The main workspace: tabs + agent panel + input bar + settings overlay
pub struct ConWorkspace {
    sidebar: Entity<SessionSidebar>,
    tabs: Vec<Tab>,
    active_tab: usize,
    font_size: f32,
    terminal_opacity: f32,
    ui_opacity: f32,
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
    /// Pending create-pane requests that need a window context to process.
    pending_create_pane_requests: Vec<PendingCreatePane>,
}

/// A deferred create-pane request waiting for a window-aware context.
struct PendingCreatePane {
    command: Option<String>,
    response_tx: crossbeam_channel::Sender<con_agent::PaneResponse>,
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
    fn clamp_terminal_opacity(value: f32) -> f32 {
        value.clamp(0.25, 1.0)
    }

    fn clamp_ui_opacity(value: f32) -> f32 {
        value.clamp(0.35, 1.0)
    }

    fn ui_surface_opacity(&self) -> f32 {
        Self::clamp_ui_opacity(self.ui_opacity)
    }

    fn elevated_ui_surface_opacity(&self) -> f32 {
        (self.ui_surface_opacity() + 0.03).min(0.98)
    }

    pub fn new(config: Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sidebar = cx.new(|cx| SessionSidebar::new(cx));
        let font_size = config.terminal.font_size;
        let terminal_opacity = Self::clamp_terminal_opacity(config.appearance.terminal_opacity);
        let ui_opacity = Self::clamp_ui_opacity(config.appearance.ui_opacity);
        let terminal_theme = TerminalTheme::by_name(&config.terminal.theme).unwrap_or_default();
        let session = Session::load().unwrap_or_default();
        let colors = theme_to_ghostty_colors(&terminal_theme);
        let ghostty_app =
            con_ghostty::GhosttyApp::new(Some(&colors), Some(font_size), Some(terminal_opacity))
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
                    runtime_trackers: RefCell::new(HashMap::new()),
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
                runtime_trackers: RefCell::new(HashMap::new()),
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
        agent_panel.update(cx, |panel, _cx| panel.set_ui_opacity(ui_opacity));
        input_bar.update(cx, |bar, _cx| bar.set_ui_opacity(ui_opacity));
        command_palette.update(cx, |palette, _cx| palette.set_ui_opacity(ui_opacity));
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
            terminal_opacity,
            ui_opacity,
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
            pending_create_pane_requests: Vec::new(),
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

    fn reconcile_runtime_trackers_for_tab(&self, tab_idx: usize) {
        let pane_ids: HashSet<usize> = self.tabs[tab_idx]
            .pane_tree
            .pane_terminals()
            .into_iter()
            .map(|(pane_id, _)| pane_id)
            .collect();
        self.tabs[tab_idx]
            .runtime_trackers
            .borrow_mut()
            .retain(|pane_id, _| pane_ids.contains(pane_id));
    }

    fn observe_terminal_runtime_for_tab(
        &self,
        tab_idx: usize,
        terminal: &TerminalPane,
        recent_output_lines: usize,
        cx: &App,
    ) -> (
        con_agent::context::PaneObservationFrame,
        con_agent::context::PaneRuntimeState,
    ) {
        let pane_tree = &self.tabs[tab_idx].pane_tree;
        let pane_id = pane_tree
            .pane_id_for_terminal(terminal)
            .unwrap_or(usize::MAX);
        let observation = terminal.observation_frame(recent_output_lines, cx);
        let runtime = {
            let mut trackers = self.tabs[tab_idx].runtime_trackers.borrow_mut();
            let tracker = trackers.entry(pane_id).or_default();
            tracker.observe(observation.clone())
        };
        (observation, runtime)
    }

    fn record_runtime_event_for_terminal(
        &self,
        tab_idx: usize,
        terminal: &TerminalPane,
        event: con_agent::context::PaneRuntimeEvent,
    ) {
        let pane_tree = &self.tabs[tab_idx].pane_tree;
        let pane_id = pane_tree
            .pane_id_for_terminal(terminal)
            .unwrap_or(usize::MAX);
        let mut trackers = self.tabs[tab_idx].runtime_trackers.borrow_mut();
        let tracker = trackers.entry(pane_id).or_default();
        tracker.record_action(event);
    }

    fn spawn_shell_anchor_command<F>(
        &self,
        _tab_idx: usize,
        pane: TerminalPane,
        pane_index: usize,
        command: String,
        timeout_secs: u64,
        parse_response: F,
        response_tx: crossbeam_channel::Sender<con_agent::PaneResponse>,
        cx: &mut Context<Self>,
    ) where
        F: Fn(Vec<String>) -> Result<con_agent::PaneResponse, String> + Send + 'static,
    {
        let _ = pane.take_command_finished(cx);
        pane.write(format!("{command}\n").as_bytes(), cx);

        cx.spawn(async move |this, cx| {
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(250))
                    .await;

                if std::time::Instant::now() >= deadline {
                    let _ = response_tx.send(con_agent::PaneResponse::Error(format!(
                        "Shell-anchor command timed out in pane {} after {}s.",
                        pane_index, timeout_secs
                    )));
                    return;
                }

                let finished = this
                    .update(cx, |_, cx| pane.take_command_finished(cx).is_some() || !pane.is_busy(cx))
                    .unwrap_or(false);
                if !finished {
                    continue;
                }

                let lines = this
                    .update(cx, |_, cx| pane.recent_lines(400, cx))
                    .unwrap_or_default();
                match parse_response(lines) {
                    Ok(response) => {
                        let _ = response_tx.send(response);
                    }
                    Err(err) => {
                        let excerpt = this
                            .update(cx, |_, cx| pane.recent_lines(120, cx).join("\n"))
                            .unwrap_or_default();
                        let _ = response_tx.send(con_agent::PaneResponse::Error(format!(
                            "Shell-anchor command in pane {} could not be parsed: {}\nRecent output:\n{}",
                            pane_index, err, excerpt
                        )));
                    }
                }
                return;
            }
        })
        .detach();
    }

    fn effective_remote_host_for_tab(
        &self,
        tab_idx: usize,
        terminal: &TerminalPane,
        cx: &App,
    ) -> Option<String> {
        self.observe_terminal_runtime_for_tab(tab_idx, terminal, 12, cx)
            .1
            .remote_host
    }

    /// Build agent context from the focused pane, including summaries of other panes.
    fn build_agent_context(&self, cx: &App) -> con_agent::TerminalContext {
        self.reconcile_runtime_trackers_for_tab(self.active_tab);
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
        let (focused_observation, focused_runtime) =
            self.observe_terminal_runtime_for_tab(self.active_tab, focused, 50, cx);

        let mut other_pane_summaries = Vec::new();
        if pane_tree.pane_count() > 1 {
            for (idx, terminal) in all_terminals.iter().enumerate() {
                if let Some(pid) = pane_tree.pane_id_for_terminal(terminal) {
                    if pid == focused_pid {
                        continue;
                    }
                    let (observation, runtime) =
                        self.observe_terminal_runtime_for_tab(self.active_tab, terminal, 10, cx);
                    let control = con_agent::control::PaneControlState::from_runtime(&runtime);
                    other_pane_summaries.push(con_agent::context::PaneSummary {
                        pane_index: idx + 1,
                        hostname: runtime.remote_host.clone(),
                        hostname_confidence: runtime.remote_host_confidence,
                        hostname_source: runtime.remote_host_source,
                        title: observation.title.clone(),
                        front_state: runtime.front_state,
                        mode: runtime.mode,
                        has_shell_integration: observation.has_shell_integration,
                        shell_metadata_fresh: runtime.shell_metadata_fresh,
                        observation_support: observation.support.clone(),
                        control,
                        agent_cli: runtime.agent_cli.clone(),
                        active_scope: runtime.active_scope.clone(),
                        evidence: runtime.evidence.clone(),
                        runtime_stack: runtime.scope_stack,
                        last_verified_runtime_stack: runtime.last_verified_scope_stack,
                        runtime_warnings: runtime.warnings,
                        tmux_session: runtime.tmux_session,
                        cwd: observation.cwd,
                        screen_hints: observation.screen_hints,
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
                self.effective_remote_host_for_tab(self.active_tab, self.active_terminal(), cx)
                    .is_some(),
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
                let hostname = self.effective_remote_host_for_tab(i, terminal, cx);
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
                self.toggle_agent_panel(&ToggleAgentPanel, window, cx);
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
        let appearance_config = settings.read(cx).appearance_config().clone();
        self.font_size = term_config.font_size;
        self.terminal_opacity = Self::clamp_terminal_opacity(appearance_config.terminal_opacity);
        self.ui_opacity = Self::clamp_ui_opacity(appearance_config.ui_opacity);
        self.agent_panel
            .update(cx, |panel, _cx| panel.set_ui_opacity(self.ui_opacity));
        self.input_bar
            .update(cx, |bar, _cx| bar.set_ui_opacity(self.ui_opacity));
        self.command_palette
            .update(cx, |palette, _cx| palette.set_ui_opacity(self.ui_opacity));

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
                terminal.set_theme(&theme, &colors, self.font_size, self.terminal_opacity, cx);
            }
        }
        if let Err(e) =
            self.ghostty_app
                .update_appearance(&colors, self.font_size, self.terminal_opacity)
        {
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
                let is_remote = self
                    .effective_remote_host_for_tab(self.active_tab, self.active_terminal(), cx)
                    .is_some();
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
        let (pane, target_pane_index) = if let Some(pane_index) = req.pane_index {
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
            (all_terminals[pane_index - 1].clone(), pane_index)
        } else {
            let pane_tree = &self.tabs[tab_idx].pane_tree;
            let focused = pane_tree.focused_terminal().clone();
            let focused_index = pane_tree
                .all_terminals()
                .iter()
                .position(|terminal| terminal.entity_id() == focused.entity_id())
                .map(|idx| idx + 1)
                .unwrap_or(1);
            (focused, focused_index)
        };

        // Safety: refuse to execute on a dead PTY.
        if !pane.is_alive(cx) {
            let _ = req.response_tx.send(TerminalExecResponse {
                output: "Pane PTY process has exited — cannot execute command.".to_string(),
                exit_code: Some(1),
            });
            return;
        }

        let (_, runtime) = self.observe_terminal_runtime_for_tab(tab_idx, &pane, 20, cx);
        let control = con_agent::control::PaneControlState::from_runtime(&runtime);
        if !control.allows_visible_shell_exec() {
            let active_scope = runtime
                .active_scope
                .as_ref()
                .map(con_agent::context::PaneRuntimeScope::summary)
                .unwrap_or_else(|| runtime.mode.as_str().to_string());
            let host = runtime.remote_host.unwrap_or_else(|| "unknown".to_string());
            let notes = if control.notes.is_empty() {
                String::new()
            } else {
                format!("\nnotes:\n- {}", control.notes.join("\n- "))
            };
            let suggestion = match control.visible_target.kind {
                con_agent::PaneVisibleTargetKind::TmuxSession => {
                    if control
                        .capabilities
                        .contains(&con_agent::PaneControlCapability::QueryTmux)
                        && control
                            .capabilities
                            .contains(&con_agent::PaneControlCapability::SendTmuxKeys)
                    {
                        format!(
                            "\n\nSUGGESTED APPROACH: This pane exposes native tmux control. Prefer tmux-native tools over outer-pane send_keys.\n\
                             1. tmux_list_targets(pane_index={idx}) to discover tmux windows/panes\n\
                             2. tmux_capture_pane(pane_index={idx}, target=\"%<pane>\") to inspect the exact tmux pane\n\
                             3. tmux_send_keys(pane_index={idx}, target=\"%<pane>\", literal_text=\"your_command\", append_enter=true) to act on that tmux pane",
                            idx = target_pane_index
                        )
                    } else {
                        format!(
                            "\n\nSUGGESTED APPROACH: tmux native control is not currently available here. Use outer-pane send_keys only as a fallback.\n\
                             1. read_pane(pane_index={idx}) to inspect the visible tmux screen\n\
                             2. send_keys(pane_index={idx}, keys=\"\\x02c\") to create a new tmux window with a shell, or use another tmux prefix sequence to reach a shell pane\n\
                             3. read_pane(pane_index={idx}) to verify you reached a shell prompt\n\
                             4. send_keys(pane_index={idx}, keys=\"your_command\\n\") to execute\n\
                             5. read_pane(pane_index={idx}) to see the output",
                            idx = target_pane_index
                        )
                    }
                }
                con_agent::PaneVisibleTargetKind::InteractiveApp => {
                    let label = control.visible_target.label.as_deref().unwrap_or("");
                    let lower = label.to_lowercase();
                    if lower.contains("vim") || lower.contains("nvim") || lower.contains("neovim") {
                        format!(
                            "\n\nSUGGESTED APPROACH: Use send_keys to interact with vim/nvim directly. To write content:\n\
                             1. send_keys(pane_index={idx}, keys=\"\\x1b\") to ensure normal mode\n\
                             2. send_keys(pane_index={idx}, keys=\":%d\\n\") to clear the buffer\n\
                             3. send_keys(pane_index={idx}, keys=\"i\") to enter insert mode\n\
                             4. send_keys(pane_index={idx}, keys=\"your content here\") to type content\n\
                             5. send_keys(pane_index={idx}, keys=\"\\x1b\") to return to normal mode\n\
                             6. send_keys(pane_index={idx}, keys=\":w\\n\") to save\n\
                             7. read_pane(pane_index={idx}) after each step to verify.",
                            idx = target_pane_index
                        )
                    } else {
                        format!(
                            "\n\nSUGGESTED APPROACH: Use read_pane(pane_index={idx}) to inspect the current screen, then \
                             send_keys(pane_index={idx}, ...) for keystroke-level interaction. \
                             Use \\x1b (Escape) or \\x03 (Ctrl-C) to exit to a shell if needed. \
                             Always verify with read_pane after each send_keys.",
                            idx = target_pane_index
                        )
                    }
                }
                _ => {
                    format!(
                        "\n\nSUGGESTED APPROACH: Use read_pane(pane_index={idx}) to inspect the visible app, then \
                         send_keys(pane_index={idx}, ...) for interaction. \
                         Always verify with read_pane after sending keys.",
                        idx = target_pane_index
                    )
                }
            };
            let output = format!(
                "Refused to execute shell command in pane {} because the visible target is not a proven shell.\n\
                 mode: {}\nactive_scope: {}\nhost: {}\nvisible_target: {}\n\
                 control_channels: {}\ncontrol_capabilities: {}{}{}",
                target_pane_index,
                runtime.mode.as_str(),
                active_scope,
                host,
                control.visible_target.summary(),
                control
                    .channels
                    .iter()
                    .map(|channel| channel.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                control
                    .capabilities
                    .iter()
                    .map(|capability| capability.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                notes,
                suggestion,
            );
            let _ = req.response_tx.send(TerminalExecResponse {
                output,
                exit_code: Some(2),
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
        self.record_runtime_event_for_terminal(
            tab_idx,
            &pane,
            con_agent::context::PaneRuntimeEvent::VisibleShellExec {
                command: req.command.clone(),
                input_generation: pane.input_generation(cx),
            },
        );

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
        &mut self,
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
                self.reconcile_runtime_trackers_for_tab(tab_idx);
                let panes: Vec<PaneInfo> = all_terminals
                    .iter()
                    .enumerate()
                    .map(|(idx, terminal)| {
                        let pid = pane_tree.pane_id_for_terminal(terminal).unwrap_or(idx);
                        let (observation, runtime) =
                            self.observe_terminal_runtime_for_tab(tab_idx, terminal, 12, cx);
                        let control = con_agent::control::PaneControlState::from_runtime(&runtime);
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
                            front_state: runtime.front_state,
                            mode: runtime.mode,
                            shell_metadata_fresh: runtime.shell_metadata_fresh,
                            shell_context_fresh: runtime.shell_context_fresh,
                            observation_support: observation.support.clone(),
                            address_space: control.address_space,
                            visible_target: control.visible_target.clone(),
                            target_stack: control.target_stack.clone(),
                            tmux_control: control.tmux.clone(),
                            control_attachments: control.attachments.clone(),
                            control_channels: control.channels.clone(),
                            control_capabilities: control.capabilities.clone(),
                            control_notes: control.notes.clone(),
                            active_scope: runtime.active_scope.clone(),
                            agent_cli: runtime.agent_cli.clone(),
                            evidence: runtime.evidence.clone(),
                            runtime_stack: runtime.scope_stack,
                            last_verified_runtime_stack: runtime.last_verified_scope_stack,
                            runtime_warnings: runtime.warnings,
                            shell_context: runtime.shell_context.clone(),
                            recent_actions: runtime.recent_actions.clone(),
                            screen_hints: observation.screen_hints,
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
                    self.record_runtime_event_for_terminal(
                        tab_idx,
                        terminal,
                        con_agent::context::PaneRuntimeEvent::RawInput {
                            keys: keys.clone(),
                            input_generation: terminal.input_generation(cx),
                        },
                    );
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
            PaneQuery::InspectTmux { pane_index } => {
                if pane_index == 0 || pane_index > all_terminals.len() {
                    PaneResponse::Error(format!(
                        "Invalid pane index {}. Use list_panes to see available panes (1-{}).",
                        pane_index,
                        all_terminals.len()
                    ))
                } else {
                    let terminal = &all_terminals[pane_index - 1];
                    let (_, runtime) =
                        self.observe_terminal_runtime_for_tab(tab_idx, terminal, 20, cx);
                    let control = con_agent::control::PaneControlState::from_runtime(&runtime);
                    if let Some(tmux) = control.tmux {
                        PaneResponse::TmuxInfo(tmux)
                    } else {
                        PaneResponse::Error(format!(
                            "Pane {} is not currently in a tmux scope.",
                            pane_index
                        ))
                    }
                }
            }
            PaneQuery::TmuxList { pane_index } => {
                if pane_index == 0 || pane_index > all_terminals.len() {
                    PaneResponse::Error(format!(
                        "Invalid pane index {}. Use list_panes to see available panes (1-{}).",
                        pane_index,
                        all_terminals.len()
                    ))
                } else {
                    let pane = all_terminals[pane_index - 1].clone();
                    let (_, runtime) =
                        self.observe_terminal_runtime_for_tab(tab_idx, &pane, 20, cx);
                    let control = con_agent::control::PaneControlState::from_runtime(&runtime);
                    let tmux_mode = control.tmux.as_ref().map(|tmux| tmux.mode);
                    if !control
                        .capabilities
                        .contains(&con_agent::PaneControlCapability::QueryTmux)
                    {
                        PaneResponse::Error(format!(
                            "Pane {} does not currently expose tmux native query capability.\nvisible_target: {}\ntmux_mode: {}\ncontrol_attachments: {}\ncontrol_capabilities: {}",
                            pane_index,
                            control.visible_target.summary(),
                            tmux_mode.map(|mode| mode.as_str()).unwrap_or("none"),
                            con_agent::control::format_control_attachments(&control.attachments),
                            control
                                .capabilities
                                .iter()
                                .map(|capability| capability.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                        ))
                    } else if pane.is_busy(cx) {
                        PaneResponse::Error(format!(
                            "Pane {} is busy. Wait for the current command to finish before running tmux-native queries from its shell anchor.",
                            pane_index
                        ))
                    } else {
                        let response_tx = req.response_tx;
                        let nonce = format!(
                            "tmux-list-{}-{}",
                            pane_index,
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|duration| duration.as_nanos())
                                .unwrap_or_default()
                        );
                        let command = con_agent::tmux::build_tmux_list_command(&nonce);
                        self.spawn_shell_anchor_command(
                            tab_idx,
                            pane,
                            pane_index,
                            command,
                            10,
                            move |lines| {
                                con_agent::tmux::parse_tmux_list_lines(&lines, &nonce)
                                    .map(con_agent::PaneResponse::TmuxList)
                            },
                            response_tx,
                            cx,
                        );
                        return;
                    }
                }
            }
            PaneQuery::TmuxCapture {
                pane_index,
                target,
                lines,
            } => {
                if pane_index == 0 || pane_index > all_terminals.len() {
                    PaneResponse::Error(format!(
                        "Invalid pane index {}. Use list_panes to see available panes (1-{}).",
                        pane_index,
                        all_terminals.len()
                    ))
                } else {
                    let pane = all_terminals[pane_index - 1].clone();
                    let (_, runtime) =
                        self.observe_terminal_runtime_for_tab(tab_idx, &pane, 20, cx);
                    let control = con_agent::control::PaneControlState::from_runtime(&runtime);
                    if !control
                        .capabilities
                        .contains(&con_agent::PaneControlCapability::QueryTmux)
                    {
                        PaneResponse::Error(format!(
                            "Pane {} does not currently expose tmux native query capability for capture.\nvisible_target: {}\ncontrol_capabilities: {}",
                            pane_index,
                            control.visible_target.summary(),
                            control
                                .capabilities
                                .iter()
                                .map(|capability| capability.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                        ))
                    } else if pane.is_busy(cx) {
                        PaneResponse::Error(format!(
                            "Pane {} is busy. Wait for the current command to finish before running tmux capture from its shell anchor.",
                            pane_index
                        ))
                    } else {
                        let response_tx = req.response_tx;
                        let nonce = format!(
                            "tmux-capture-{}-{}",
                            pane_index,
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|duration| duration.as_nanos())
                                .unwrap_or_default()
                        );
                        let command = con_agent::tmux::build_tmux_capture_command(
                            &nonce,
                            target.as_deref(),
                            lines,
                        );
                        self.spawn_shell_anchor_command(
                            tab_idx,
                            pane,
                            pane_index,
                            command,
                            10,
                            move |lines| {
                                con_agent::tmux::parse_tmux_capture_lines(
                                    &lines,
                                    &nonce,
                                    target.as_deref(),
                                )
                                .map(con_agent::PaneResponse::TmuxCapture)
                            },
                            response_tx,
                            cx,
                        );
                        return;
                    }
                }
            }
            PaneQuery::TmuxSendKeys {
                pane_index,
                target,
                literal_text,
                key_names,
                append_enter,
            } => {
                if pane_index == 0 || pane_index > all_terminals.len() {
                    PaneResponse::Error(format!(
                        "Invalid pane index {}. Use list_panes to see available panes (1-{}).",
                        pane_index,
                        all_terminals.len()
                    ))
                } else {
                    let pane = all_terminals[pane_index - 1].clone();
                    let (_, runtime) =
                        self.observe_terminal_runtime_for_tab(tab_idx, &pane, 20, cx);
                    let control = con_agent::control::PaneControlState::from_runtime(&runtime);
                    if !control
                        .capabilities
                        .contains(&con_agent::PaneControlCapability::SendTmuxKeys)
                    {
                        PaneResponse::Error(format!(
                            "Pane {} does not currently expose tmux native send-keys capability.\nvisible_target: {}\ncontrol_capabilities: {}",
                            pane_index,
                            control.visible_target.summary(),
                            control
                                .capabilities
                                .iter()
                                .map(|capability| capability.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                        ))
                    } else if pane.is_busy(cx) {
                        PaneResponse::Error(format!(
                            "Pane {} is busy. Wait for the current command to finish before sending tmux-native keys from its shell anchor.",
                            pane_index
                        ))
                    } else {
                        match con_agent::tmux::build_tmux_send_keys_command(
                            &target,
                            literal_text.as_deref(),
                            &key_names,
                            append_enter,
                        ) {
                            Ok(command) => {
                                let response_tx = req.response_tx;
                                self.spawn_shell_anchor_command(
                                    tab_idx,
                                    pane,
                                    pane_index,
                                    command,
                                    10,
                                    move |_lines| {
                                        Ok(con_agent::PaneResponse::Content(format!(
                                            "tmux send-keys delivered to target {}",
                                            target
                                        )))
                                    },
                                    response_tx,
                                    cx,
                                );
                                return;
                            }
                            Err(err) => PaneResponse::Error(err),
                        }
                    }
                }
            }
            PaneQuery::ProbeShellContext { pane_index } => {
                if pane_index == 0 || pane_index > all_terminals.len() {
                    PaneResponse::Error(format!(
                        "Invalid pane index {}. Use list_panes to see available panes (1-{}).",
                        pane_index,
                        all_terminals.len()
                    ))
                } else {
                    let pane = all_terminals[pane_index - 1].clone();
                    let (_, runtime) =
                        self.observe_terminal_runtime_for_tab(tab_idx, &pane, 20, cx);
                    let control = con_agent::control::PaneControlState::from_runtime(&runtime);
                    if !control.allows_shell_probe() {
                        PaneResponse::Error(format!(
                            "Pane {} does not currently expose the probe_shell_context capability. \
                             It must be a proven fresh shell prompt before shell-scoped probing is allowed.\n\
                             visible_target: {}\ncontrol_attachments: {}\ncontrol_capabilities: {}",
                            pane_index,
                            control.visible_target.summary(),
                            con_agent::control::format_control_attachments(&control.attachments),
                            control
                                .capabilities
                                .iter()
                                .map(|capability| capability.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                        ))
                    } else if pane.is_busy(cx) {
                        PaneResponse::Error(format!(
                            "Pane {} is busy. Wait for the current command to finish before running a shell probe.",
                            pane_index
                        ))
                    } else {
                        let response_tx = req.response_tx;
                        let nonce = format!(
                            "{}-{}",
                            pane_index,
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|duration| duration.as_nanos())
                                .unwrap_or_default()
                        );
                        let command = con_agent::shell_probe::build_shell_probe_command(&nonce);
                        let _ = pane.take_command_finished(cx);
                        pane.write(format!("{command}\n").as_bytes(), cx);

                        cx.spawn(async move |this, cx| {
                            let deadline = std::time::Instant::now()
                                + std::time::Duration::from_secs(10);

                            loop {
                                cx.background_executor()
                                    .timer(std::time::Duration::from_millis(250))
                                    .await;

                                if std::time::Instant::now() >= deadline {
                                    let _ = response_tx.send(PaneResponse::Error(format!(
                                        "Shell probe timed out in pane {} after 10s.",
                                        pane_index
                                    )));
                                    return;
                                }

                                let finished = this
                                    .update(cx, |_, cx| {
                                        pane.take_command_finished(cx).is_some() || !pane.is_busy(cx)
                                    })
                                    .unwrap_or(false);
                                if !finished {
                                    continue;
                                }

                                let lines = this
                                    .update(cx, |_, cx| pane.recent_lines(200, cx))
                                    .unwrap_or_default();
                                match con_agent::shell_probe::parse_shell_probe_lines(&lines, &nonce)
                                {
                                    Ok(result) => {
                                        let recorded_result = result.clone();
                                        let _ = this.update(cx, |workspace, cx| {
                                            workspace.record_runtime_event_for_terminal(
                                                tab_idx,
                                                &pane,
                                                con_agent::context::PaneRuntimeEvent::ShellProbe {
                                                    result: recorded_result,
                                                    captured_input_generation: pane.input_generation(cx),
                                                },
                                            );
                                        });
                                        let _ = response_tx.send(PaneResponse::ShellProbe(result));
                                    }
                                    Err(err) => {
                                        let excerpt = lines.join("\n");
                                        let _ = response_tx.send(PaneResponse::Error(format!(
                                            "Shell probe finished in pane {} but the probe output could not be parsed: {}\nRecent output:\n{}",
                                            pane_index, err, excerpt
                                        )));
                                    }
                                }
                                return;
                            }
                        })
                        .detach();
                        return;
                    }
                }
            }
            PaneQuery::CheckBusy { pane_index } => {
                if pane_index == 0 || pane_index > all_terminals.len() {
                    PaneResponse::Error(format!(
                        "Invalid pane index {}. Use list_panes to see available panes (1-{}).",
                        pane_index,
                        all_terminals.len()
                    ))
                } else {
                    let terminal = &all_terminals[pane_index - 1];
                    PaneResponse::BusyStatus {
                        is_busy: terminal.is_busy(cx),
                        has_shell_integration: terminal.has_shell_integration(cx),
                    }
                }
            }
            PaneQuery::WaitFor {
                pane_index,
                timeout_secs,
                pattern,
            } => {
                // Normalize empty pattern to None — "".contains("") is always true in Rust,
                // making empty pattern match instantly (useless). Treat as idle/quiescence.
                let pattern = pattern.filter(|p| !p.is_empty());

                log::info!(
                    "[wait_for] pane={} timeout={:?} pattern={:?}",
                    pane_index,
                    timeout_secs,
                    pattern,
                );

                if pane_index == 0 || pane_index > all_terminals.len() {
                    PaneResponse::Error(format!(
                        "Invalid pane index {}. Use list_panes to see available panes (1-{}).",
                        pane_index,
                        all_terminals.len()
                    ))
                } else {
                    let pane = all_terminals[pane_index - 1].clone();
                    let has_si = pane.has_shell_integration(cx);
                    let timeout = timeout_secs.unwrap_or(30).min(120);
                    let response_tx = req.response_tx;

                    log::info!("[wait_for] has_si={} is_busy={}", has_si, pane.is_busy(cx),);

                    // Check if already in target state before spawning async task
                    if pattern.is_none() && has_si && !pane.is_busy(cx) {
                        log::info!("[wait_for] → early idle return");
                        let output = pane.recent_lines(50, cx).join("\n");
                        let _ = response_tx.send(PaneResponse::WaitComplete {
                            status: "idle".into(),
                            output,
                        });
                        return;
                    }
                    if let Some(ref pat) = pattern {
                        let content = pane.recent_lines(50, cx).join("\n");
                        if content.contains(pat.as_str()) {
                            let _ = response_tx.send(PaneResponse::WaitComplete {
                                status: "matched".into(),
                                output: content,
                            });
                            return;
                        }
                    }

                    let use_quiescence = !has_si && pattern.is_none();

                    // Spawn async task — direct terminal access, no channel overhead
                    cx.spawn(async move |this, cx| {
                        let deadline = std::time::Instant::now()
                            + std::time::Duration::from_secs(timeout as u64);

                        // Three modes:
                        // 1. Shell integration idle: 100ms polling, check is_busy/command_finished
                        // 2. Pattern match: 500ms polling, check content
                        // 3. Quiescence (no SI, no pattern): 500ms polling, detect output stable for 2s
                        let interval: u64 = if has_si && pattern.is_none() { 100 } else { 500 };

                        // Normalize terminal output for stable comparison.
                        // ghostty_surface_read_text returns lines with trailing whitespace that
                        // varies with cursor position — trim each line to get content-only text.
                        let normalize_output = |lines: Vec<String>| -> String {
                            lines.iter().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n")
                        };

                        // For quiescence mode: capture baseline INSIDE async task to
                        // avoid race between snapshot and first poll.
                        // 4 polls × 500ms = 2s — fast enough for interactive use,
                        // long enough to avoid false positives from progress output.
                        const QUIET_THRESHOLD: u32 = 4;
                        let mut last_snapshot = if use_quiescence {
                            match this.update(cx, |_, cx| normalize_output(pane.recent_lines(50, cx))) {
                                Ok(s) if !s.is_empty() => s,
                                _ => {
                                    // Terminal not ready yet — use sentinel that won't match real output.
                                    // First real poll will set the actual baseline.
                                    log::info!("[wait_for] quiescence: empty baseline, deferring to first poll");
                                    String::new()
                                }
                            }
                        } else {
                            String::new()
                        };
                        let mut stable_count: u32 = 0;

                        loop {
                            cx.background_executor()
                                .timer(std::time::Duration::from_millis(interval))
                                .await;

                            if std::time::Instant::now() >= deadline {
                                let output = this
                                    .update(cx, |_, cx| pane.recent_lines(50, cx).join("\n"))
                                    .unwrap_or_default();
                                let _ = response_tx.send(PaneResponse::WaitComplete {
                                    status: "timeout".into(),
                                    output,
                                });
                                return;
                            }

                            let done = this
                                .update(cx, |_, cx| {
                                    if let Some(ref pat) = pattern {
                                        // Pattern mode
                                        let content = pane.recent_lines(50, cx).join("\n");
                                        if content.contains(pat.as_str()) {
                                            Some(("matched".to_string(), content))
                                        } else {
                                            None
                                        }
                                    } else if has_si {
                                        // Shell integration idle mode
                                        if pane.take_command_finished(cx).is_some()
                                            || !pane.is_busy(cx)
                                        {
                                            let output = pane.recent_lines(50, cx).join("\n");
                                            Some(("idle".to_string(), output))
                                        } else {
                                            None
                                        }
                                    } else {
                                        // Quiescence mode: output unchanged for QUIET_THRESHOLD polls
                                        let current = normalize_output(pane.recent_lines(50, cx));
                                        if current.is_empty() {
                                            // Terminal not producing output yet — don't count as stable
                                            None
                                        } else if last_snapshot.is_empty() {
                                            // First non-empty snapshot — set baseline, start counting
                                            log::info!("[wait_for] quiescence: baseline set ({} bytes)", current.len());
                                            last_snapshot = current;
                                            stable_count = 0;
                                            None
                                        } else if current == last_snapshot {
                                            stable_count += 1;
                                            log::info!("[wait_for] quiescence: stable {}/{}", stable_count, QUIET_THRESHOLD);
                                            if stable_count >= QUIET_THRESHOLD {
                                                Some(("idle".to_string(), current))
                                            } else {
                                                None
                                            }
                                        } else {
                                            log::info!("[wait_for] quiescence: output changed, resetting (stable was {})", stable_count);
                                            last_snapshot = current;
                                            stable_count = 0;
                                            None
                                        }
                                    }
                                })
                                .ok()
                                .flatten();

                            if let Some((status, output)) = done {
                                let _ = response_tx.send(PaneResponse::WaitComplete {
                                    status,
                                    output,
                                });
                                return;
                            }
                        }
                    })
                    .detach();
                    return; // Response sent by spawned task
                }
            }
            PaneQuery::CreatePane { command } => {
                // Creating a terminal requires a Window, which is not available
                // in the poll loop. Defer to the next render cycle.
                self.pending_create_pane_requests.push(PendingCreatePane {
                    command,
                    response_tx: req.response_tx,
                });
                cx.notify();
                return;
            }
        };

        let _ = req.response_tx.send(response);
    }

    fn toggle_agent_panel(
        &mut self,
        _: &ToggleAgentPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.agent_panel_open = !self.agent_panel_open;
        if self.agent_panel_open {
            if self.input_bar_visible {
                self.input_bar.focus_handle(cx).focus(window, cx);
            } else {
                let focused_inline = self
                    .agent_panel
                    .update(cx, |panel, cx| panel.focus_inline_input(window, cx));
                if !focused_inline {
                    self.focus_terminal(window, cx);
                }
            }
        } else {
            self.focus_terminal(window, cx);
        }
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
            runtime_trackers: RefCell::new(HashMap::new()),
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
        if index >= self.tabs.len() {
            return;
        }
        if self.tabs.len() <= 1 {
            // Last tab — quit the app (matches terminal process exit behavior)
            self.quit(&Quit, window, cx);
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
        entity: &Entity<GhosttyView>,
        _event: &GhosttyProcessExited,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Find which tab contains the dead pane (may not be the active tab).
        let entity_id = entity.entity_id();
        let tab_idx = self
            .tabs
            .iter()
            .position(|tab| tab.pane_tree.pane_id_for_entity(entity_id).is_some());
        let Some(tab_idx) = tab_idx else { return };
        if let Some(terminal) = self.tabs[tab_idx]
            .pane_tree
            .all_terminals()
            .into_iter()
            .find(|terminal| terminal.entity_id() == entity_id)
        {
            self.record_runtime_event_for_terminal(
                tab_idx,
                &terminal,
                con_agent::context::PaneRuntimeEvent::ProcessExited,
            );
        }

        let pane_tree = &mut self.tabs[tab_idx].pane_tree;
        if pane_tree.pane_count() > 1 {
            // Close the specific pane whose process exited, not the focused pane.
            if let Some(pane_id) = pane_tree.pane_id_for_entity(entity_id) {
                pane_tree.close_pane(pane_id);
            }
            if tab_idx == self.active_tab {
                pane_tree.focused_terminal().focus(window, cx);
            }
        } else if self.tabs.len() > 1 {
            // Last pane in this tab — close the tab.
            self.close_tab_by_index(tab_idx, window, cx);
        } else {
            // Last pane in last tab — quit the app.
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
        // Process any deferred create-pane requests that need window access.
        // Creates a split within the agent's tab so the new pane is addressable
        // by index alongside existing panes (not isolated in a separate tab).
        let pending = std::mem::take(&mut self.pending_create_pane_requests);
        for req in pending {
            let terminal = self.create_terminal(None, window, cx);
            self.tabs[self.active_tab]
                .pane_tree
                .split(SplitDirection::Horizontal, terminal.clone());
            terminal.focus(window, cx);

            if let Some(cmd) = &req.command {
                let cmd_with_newline = format!("{}\n", cmd);
                terminal.write(cmd_with_newline.as_bytes(), cx);
            }
            self.record_runtime_event_for_terminal(
                self.active_tab,
                &terminal,
                con_agent::context::PaneRuntimeEvent::PaneCreated {
                    startup_command: req.command.clone(),
                },
            );

            // Return the 1-indexed position of the new pane in the flat pane list
            let pane_index = self.tabs[self.active_tab].pane_tree.pane_count();
            let _ = req
                .response_tx
                .send(con_agent::PaneResponse::PaneCreated { pane_index });

            cx.notify();
        }

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
        self.reconcile_runtime_trackers_for_tab(self.active_tab);

        // Sync pane info and CWD to input bar
        let pane_tree = &self.tabs[self.active_tab].pane_tree;
        let focused_pane_id = pane_tree.focused_pane_id();
        let pane_infos: Vec<PaneInfo> = pane_tree
            .pane_terminals()
            .into_iter()
            .map(|(id, terminal)| {
                let (_, runtime) =
                    self.observe_terminal_runtime_for_tab(self.active_tab, &terminal, 12, cx);
                let hostname = runtime.remote_host.clone();
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
        let ui_surface_opacity = self.ui_surface_opacity();
        let elevated_ui_surface_opacity = self.elevated_ui_surface_opacity();

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
            // Draggable divider — invisible, only visible on hover
            main_area = main_area
                .child(
                    div()
                        .id("agent-panel-divider")
                        .w(px(1.0))
                        .h_full()
                        .flex_shrink_0()
                        .cursor_col_resize()
                        .hover(|s| s.bg(theme.primary.opacity(0.15)))
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

        // Tab bar — full-width tabs fill available space
        let tab_count = self.tabs.len();
        let mut tab_bar = div()
            .id("tab-bar")
            .flex()
            .h(px(36.0))
            .items_end()
            .pl(px(78.0)) // leave room for traffic lights
            .pr(px(6.0))
            .on_click(|event, window, _cx| {
                if event.click_count() == 2 {
                    window.titlebar_double_click();
                }
            });

        // Tabs container — each tab takes equal share of available width
        let mut tabs_container = div().flex().flex_1().min_w_0().items_end();

        for (index, tab) in self.tabs.iter().enumerate() {
            let is_active = index == self.active_tab;
            let needs_attention = tab.needs_attention && !is_active;
            let terminal = tab.pane_tree.focused_terminal();
            let title = terminal.title(cx).unwrap_or_else(|| tab.title.clone());

            // Truncate long titles
            let display_title: String = if title.len() > 24 {
                format!("{}…", &title[..22])
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
                    .size(px(16.0))
                    .flex_shrink_0()
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.muted.opacity(0.15)));
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
                                .size(px(10.0))
                                .text_color(theme.muted_foreground.opacity(0.5)),
                        ),
                )
            } else {
                None
            };

            let mut tab_el = div()
                .id(ElementId::Name(format!("tab-{}", index).into()))
                .group("tab")
                .flex()
                .flex_1()
                .min_w_0()
                .max_w(px(200.0))
                .items_center()
                .px(px(10.0))
                .h(px(30.0))
                .text_size(px(11.5))
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
                // Active tab — lifted surface, connects to content below
                tab_el = tab_el
                    .rounded_t(px(7.0))
                    .bg(theme.background.opacity(elevated_ui_surface_opacity))
                    .text_color(theme.foreground)
                    .font_weight(FontWeight::MEDIUM);
            } else {
                // Inactive tab — quiet, appears on hover
                tab_el = tab_el
                    .rounded_t(px(6.0))
                    .mb(px(1.0))
                    .text_color(theme.muted_foreground.opacity(0.45))
                    .hover(|s| s.bg(theme.muted.opacity(0.06)));
            }

            let mut tab_content = div().flex().items_center().gap(px(5.0)).w_full().min_w_0();

            // Attention dot for tabs with pending agent activity
            if needs_attention {
                tab_content = tab_content.child(
                    div()
                        .size(px(5.0))
                        .rounded_full()
                        .flex_shrink_0()
                        .bg(theme.primary),
                );
            }

            // Terminal icon
            tab_content = tab_content.child(
                svg()
                    .path("phosphor/terminal.svg")
                    .size(px(12.0))
                    .flex_shrink_0()
                    .text_color(if is_active {
                        theme.foreground.opacity(0.7)
                    } else {
                        theme.muted_foreground.opacity(0.4)
                    }),
            );

            // Title — fills remaining space, pushes close button to right
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

            tabs_container = tabs_container.child(tab_el.child(tab_content));
        }

        // "+" button — right of last tab, inside tabs container
        tabs_container = tabs_container.child(
            div()
                .id("tab-new")
                .flex()
                .items_center()
                .justify_center()
                .size(px(22.0))
                .mb(px(4.0))
                .ml(px(2.0))
                .rounded(px(5.0))
                .cursor_pointer()
                .flex_shrink_0()
                .hover(|s| s.bg(theme.muted.opacity(0.10)))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.new_tab(&NewTab, window, cx);
                }))
                .child(
                    svg()
                        .path("phosphor/plus.svg")
                        .size(px(12.0))
                        .text_color(theme.muted_foreground.opacity(0.45)),
                ),
        );

        tab_bar = tab_bar.child(tabs_container);

        // Right-side controls — compact row
        let mut tab_controls = div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .mb(px(4.0))
            .flex_shrink_0();

        // Input bar toggle
        tab_controls = tab_controls.child(
            div()
                .id("toggle-input-bar")
                .flex()
                .items_center()
                .justify_center()
                .size(px(22.0))
                .rounded(px(5.0))
                .cursor_pointer()
                .hover(|s| s.bg(theme.muted.opacity(0.10)))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_input_bar(&crate::ToggleInputBar, window, cx);
                }))
                .child(
                    svg()
                        .path("phosphor/square-half-bottom-fill.svg")
                        .size(px(12.0))
                        .text_color(if self.input_bar_visible {
                            theme.primary
                        } else {
                            theme.muted_foreground.opacity(0.4)
                        }),
                ),
        );

        // Agent panel toggle
        tab_controls = tab_controls.child(
            div()
                .id("toggle-agent-panel")
                .flex()
                .items_center()
                .justify_center()
                .size(px(22.0))
                .rounded(px(5.0))
                .cursor_pointer()
                .hover(|s| s.bg(theme.muted.opacity(0.10)))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_agent_panel(&ToggleAgentPanel, window, cx);
                }))
                .child(
                    svg()
                        .path("phosphor/square-half-fill.svg")
                        .size(px(12.0))
                        .text_color(if self.agent_panel_open {
                            theme.primary
                        } else {
                            theme.muted_foreground.opacity(0.4)
                        }),
                ),
        );

        tab_bar = tab_bar.child(tab_controls);

        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.title_bar.opacity(ui_surface_opacity))
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
                .bg(theme.background.opacity(elevated_ui_surface_opacity))
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
                .bg(theme.background.opacity(elevated_ui_surface_opacity))
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
/// 1. Proven remote hostname
/// 2. CWD directory name (skip bare home directories like `/Users/name`)
/// 3. Raw terminal title
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

    // Raw title from the visible surface
    if let Some(title) = title {
        let title = title.trim();
        if !title.is_empty() {
            return title.to_string();
        }
    }

    format!("Pane {}", pane_id + 1)
}
