use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

#[cfg(target_os = "macos")]
use cocoa::base::id;
#[cfg(target_os = "macos")]
use cocoa::foundation::NSSize;
use gpui::{prelude::FluentBuilder as _, *};
use gpui_component::{ActiveTheme, tooltip::Tooltip};
use serde_json::json;
use tokio::sync::oneshot;
#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl};

const AGENT_PANEL_DEFAULT_WIDTH: f32 = 400.0;
const AGENT_PANEL_MIN_WIDTH: f32 = 200.0;
const TERMINAL_MIN_CONTENT_WIDTH: f32 = 360.0;
const TOP_BAR_COMPACT_HEIGHT: f32 = 28.0;
const TOP_BAR_TABS_HEIGHT: f32 = 36.0;
const CHROME_TRANSITION_SEAM_COVER: f32 = 4.0;
const MAX_SHELL_HISTORY_PER_PANE: usize = 80;
const MAX_GLOBAL_SHELL_HISTORY: usize = 240;
const MAX_GLOBAL_INPUT_HISTORY: usize = 240;

fn perf_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("CON_GHOSTTY_PROFILE")
            .is_some_and(|v| !v.is_empty() && v != "0")
    })
}

fn chrome_tooltip(label: &str, stroke: Option<Keystroke>, window: &mut Window, cx: &mut App) -> AnyView {
    let label = label.to_string();
    Tooltip::element(move |_, cx| {
        let theme = cx.theme();
        let mut content = div()
            .flex()
            .items_center()
            .gap(px(7.0))
            .child(
                div()
                    .text_size(px(12.0))
                    .line_height(px(16.0))
                    .text_color(theme.popover_foreground)
                    .child(label.clone()),
            );

        if let Some(stroke) = stroke.as_ref() {
            content = content.child(crate::keycaps::keycaps_for_stroke(stroke, theme));
        }

        content
    })
    .build(window, cx)
}

fn max_agent_panel_width(window_width: f32) -> f32 {
    (window_width - TERMINAL_MIN_CONTENT_WIDTH).max(AGENT_PANEL_MIN_WIDTH)
}

/// Windows / Linux caption buttons (Min / Max+Restore / Close).
///
/// Each button is marked with `.window_control_area(..)` so GPUI's
/// platform layer hit-tests it during `WM_NCHITTEST` on Windows (or
/// the equivalent on Linux) and dispatches the OS-level action on
/// click — no explicit `minimize_window()` / `zoom_window()` /
/// `remove_window()` plumbing needed here.
///
/// Uses Phosphor SVGs instead of Segoe Fluent Icons so the bar
/// renders identically on hosts where Segoe Fluent Icons isn't
/// installed (Win10 without the 2022 optional feature, Linux,
/// tests). Size and hover colors mirror Windows 11's native caption
/// buttons: 36px wide, 45px min height doesn't apply here (we honour
/// the shared `top_bar_height` instead), red hover on Close.
#[cfg(not(target_os = "macos"))]
fn caption_buttons(
    window: &Window,
    theme: &gpui_component::theme::ThemeColor,
    height: f32,
) -> impl IntoElement {
    use gpui::{div, px, svg, Hsla, ParentElement, Rgba, Styled, WindowControlArea};

    let close_red: Hsla = Rgba {
        r: 232.0 / 255.0,
        g: 17.0 / 255.0,
        b: 32.0 / 255.0,
        a: 1.0,
    }
    .into();
    let fg = theme.muted_foreground.opacity(0.7);
    let hover_bg = theme.muted.opacity(0.12);

    let button = |id: &'static str,
                  icon: &'static str,
                  area: WindowControlArea,
                  close: bool| {
        let hover = if close { close_red } else { hover_bg };
        let text = if close { gpui::white() } else { fg };
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            // `.occlude()` is required so the parent top_bar's
            // `WindowControlArea::Drag` hit-test doesn't swallow these
            // child buttons on Windows (HTCLOSE/HTMAXBUTTON/HTMINBUTTON
            // won't fire without it). Matches Zed's platform_windows
            // caption-button implementation.
            .occlude()
            .w(px(36.0))
            .h(px(height))
            .flex_shrink_0()
            .window_control_area(area)
            .hover(move |s| s.bg(hover))
            .child(svg().path(icon).size(px(10.0)).text_color(text))
    };

    let (max_icon, max_area) = if window.is_maximized() {
        ("phosphor/copy.svg", WindowControlArea::Max)
    } else {
        ("phosphor/square.svg", WindowControlArea::Max)
    };

    div()
        .flex()
        .flex_row()
        .flex_shrink_0()
        .h(px(height))
        .child(button(
            "win-min",
            "phosphor/minus.svg",
            WindowControlArea::Min,
            false,
        ))
        .child(button("win-max", max_icon, max_area, false))
        .child(button(
            "win-close",
            "phosphor/x.svg",
            WindowControlArea::Close,
            true,
        ))
}

use crate::agent_panel::{
    AgentPanel, CancelRequest, DeleteConversation, InlineInputSubmit,
    InlineSkillAutocompleteChanged, LoadConversation, NewConversation, PanelState,
    RerunFromMessage, SelectSessionModel, SelectSessionProvider,
    SetAutoApprove,
};
use crate::command_palette::{
    CommandPalette, PaletteDismissed, PaletteSelect, ToggleCommandPalette,
};
use crate::input_bar::{
    EscapeInput, InputBar, InputEdited, InputMode, InputScopeChanged, PaneInfo,
    SkillAutocompleteChanged, SubmitInput, TogglePaneScopePicker as TogglePaneScopePickerRequested,
};
use crate::model_registry::ModelRegistry;
use crate::motion::MotionValue;
use crate::pane_tree::{PaneTree, SplitDirection, SplitPlacement};
use crate::settings_panel::{self, SaveSettings, SettingsPanel, ThemePreview};
use crate::sidebar::{NewSession, SessionEntry, SessionSidebar, SidebarSelect};
use crate::terminal_pane::{TerminalPane, subscribe_terminal_pane};
use con_terminal::TerminalTheme;

use crate::ghostty_view::{
    GhosttyFocusChanged, GhosttyProcessExited, GhosttySplitRequested, GhosttyTitleChanged,
    GhosttyView,
};
use crate::{
    CloseTab, CycleInputMode, FocusInput, NewTab, NextTab, PreviousTab, Quit, SplitDown,
    SplitRight, ToggleAgentPanel, TogglePaneScopePicker,
};
use con_agent::{Conversation, TerminalExecRequest, TerminalExecResponse};
use con_core::config::Config;
use con_core::control::{
    AgentAskResult, ControlCommand, ControlError, ControlRequestEnvelope, ControlResult,
    SystemIdentifyResult, TabInfo,
};
use con_core::harness::{AgentHarness, AgentSession, HarnessEvent, InputKind};
use con_core::session::{GlobalHistoryState, PaneLayoutState, PaneSplitDirection, Session};
use con_core::{SuggestionContext, SuggestionEngine};

struct Tab {
    pane_tree: PaneTree,
    title: String,
    needs_attention: bool,
    session: AgentSession,
    panel_state: PanelState,
    runtime_trackers: RefCell<HashMap<usize, con_agent::context::PaneRuntimeTracker>>,
    runtime_cache: RefCell<HashMap<usize, con_agent::context::PaneRuntimeState>>,
    shell_history: HashMap<usize, VecDeque<CommandSuggestionEntry>>,
}

#[derive(Clone)]
struct CommandSuggestionEntry {
    command: String,
    cwd: Option<String>,
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct NSOperatingSystemVersion {
    major_version: isize,
    minor_version: isize,
    patch_version: isize,
}

#[derive(Clone)]
struct ShellSuggestionResult {
    tab_idx: usize,
    pane_id: usize,
    prefix: String,
    completion: String,
}

enum LocalPathCompletion {
    Inline(String),
    Candidates(Vec<String>),
}

/// The main workspace: tabs + agent panel + input bar + settings overlay
pub struct ConWorkspace {
    sidebar: Entity<SessionSidebar>,
    tabs: Vec<Tab>,
    active_tab: usize,
    terminal_font_family: String,
    ui_font_family: String,
    ui_font_size: f32,
    font_size: f32,
    terminal_cursor_style: String,
    terminal_opacity: f32,
    terminal_blur: bool,
    ui_opacity: f32,
    background_image: Option<String>,
    background_image_opacity: f32,
    background_image_position: String,
    background_image_fit: String,
    background_image_repeat: bool,
    agent_panel: Entity<AgentPanel>,
    input_bar: Entity<InputBar>,
    settings_panel: Entity<SettingsPanel>,
    command_palette: Entity<CommandPalette>,
    model_registry: ModelRegistry,
    harness: AgentHarness,
    shell_suggestion_engine: SuggestionEngine,
    global_shell_history: VecDeque<CommandSuggestionEntry>,
    global_input_history: VecDeque<String>,
    pane_scope_picker_open: bool,
    agent_panel_open: bool,
    agent_panel_motion: MotionValue,
    agent_panel_width: f32,
    tab_strip_motion: MotionValue,
    input_bar_visible: bool,
    input_bar_motion: MotionValue,
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
    /// Last wake generation observed from Ghostty's embedded runtime.
    last_ghostty_wake_generation: u64,
    /// Last macOS content-resize increment applied to the window, in 1/1000th points.
    #[cfg(target_os = "macos")]
    last_window_resize_increment_millipoints: Option<(u32, u32)>,
    /// Pending create-pane requests that need a window context to process.
    pending_create_pane_requests: Vec<PendingCreatePane>,
    /// Pending window-aware control requests such as tab lifecycle mutations.
    pending_window_control_requests: Vec<PendingWindowControlRequest>,
    /// Control-plane requests from the external `con-cli` socket bridge.
    control_request_rx: crossbeam_channel::Receiver<ControlRequestEnvelope>,
    /// Keeps the Unix socket alive for this workspace instance.
    control_socket: Option<con_core::ControlSocketHandle>,
    /// Pending external agent requests keyed by 0-based tab index.
    pending_control_agent_requests: HashMap<usize, PendingControlAgentRequest>,
    shell_suggestion_rx: crossbeam_channel::Receiver<ShellSuggestionResult>,
    shell_suggestion_tx: crossbeam_channel::Sender<ShellSuggestionResult>,
    /// Monotonic request id for control-plane agent asks so stale timeout tasks cannot
    /// cancel a newer request on the same tab.
    next_control_agent_request_id: u64,
    /// Window handle used to re-enter a window-aware context from deferred control work.
    window_handle: AnyWindowHandle,
    /// Weak self handle for deferred window callbacks.
    workspace_handle: WeakEntity<ConWorkspace>,
    /// Ensures native window-close cleanup only runs once.
    window_close_prepared: bool,
    /// Ordered, coalescing session persistence worker.
    session_save_tx: crossbeam_channel::Sender<SessionSaveRequest>,
}

#[derive(Clone)]
struct ResolvedPaneTarget {
    pane: TerminalPane,
    pane_index: usize,
    pane_id: usize,
}

/// A deferred create-pane request waiting for a window-aware context.
struct PendingCreatePane {
    command: Option<String>,
    tab_idx: usize,
    location: con_agent::tools::PaneCreateLocation,
    response_tx: crossbeam_channel::Sender<con_agent::PaneResponse>,
}

enum PendingWindowControlRequest {
    TabsNew {
        response_tx: oneshot::Sender<ControlResult>,
    },
    TabsClose {
        tab_idx: usize,
        response_tx: oneshot::Sender<ControlResult>,
    },
}

struct PendingControlAgentRequest {
    request_id: u64,
    prompt: String,
    auto_approve_tools: bool,
    response_tx: tokio::sync::oneshot::Sender<ControlResult>,
}

enum SessionSaveRequest {
    Save(Session, GlobalHistoryState),
    Flush(Session, GlobalHistoryState, crossbeam_channel::Sender<()>),
}

fn spawn_session_save_worker() -> crossbeam_channel::Sender<SessionSaveRequest> {
    let (tx, rx) = crossbeam_channel::unbounded();
    std::thread::Builder::new()
        .name("con-session-save".into())
        .spawn(move || {
            loop {
                let request = match rx.recv() {
                    Ok(request) => request,
                    Err(_) => break,
                };

                let (mut latest_session, mut latest_history, mut flush_waiters) = match request {
                    SessionSaveRequest::Save(session, history) => {
                        (Some(session), Some(history), Vec::new())
                    }
                    SessionSaveRequest::Flush(session, history, waiter) => {
                        (Some(session), Some(history), vec![waiter])
                    }
                };

                while let Ok(request) = rx.try_recv() {
                    match request {
                        SessionSaveRequest::Save(session, history) => {
                            latest_session = Some(session);
                            latest_history = Some(history);
                        }
                        SessionSaveRequest::Flush(session, history, waiter) => {
                            latest_session = Some(session);
                            latest_history = Some(history);
                            flush_waiters.push(waiter);
                        }
                    }
                }

                if let Some(session) = latest_session
                    && let Err(err) = session.save()
                {
                    log::warn!("Failed to save session: {}", err);
                }
                if let Some(history) = latest_history
                    && let Err(err) = history.save()
                {
                    log::warn!("Failed to save command history: {}", err);
                }

                for waiter in flush_waiters {
                    let _ = waiter.send(());
                }
            }
        })
        .expect("failed to spawn session save worker");
    tx
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
    const SECONDARY_PANE_OBSERVATION_LINES: usize = 40;

    fn clamp_terminal_opacity(value: f32) -> f32 {
        value.clamp(0.25, 1.0)
    }

    fn clamp_ui_opacity(value: f32) -> f32 {
        value.clamp(0.35, 1.0)
    }

    fn clamp_background_image_opacity(value: f32) -> f32 {
        value.clamp(0.0, 1.0)
    }

    fn remap_opacity(value: f32, input_floor: f32, output_floor: f32, exponent: f32) -> f32 {
        let normalized = ((value - input_floor) / (1.0 - input_floor)).clamp(0.0, 1.0);
        output_floor + (1.0 - output_floor) * normalized.powf(exponent)
    }

    #[cfg(target_os = "macos")]
    fn macos_major_version() -> Option<isize> {
        use objc::{class, msg_send, sel, sel_impl};

        unsafe {
            let process_info: *mut objc::runtime::Object =
                msg_send![class!(NSProcessInfo), processInfo];
            if process_info.is_null() {
                return None;
            }

            let version: NSOperatingSystemVersion = msg_send![process_info, operatingSystemVersion];
            Some(version.major_version)
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn macos_major_version() -> Option<isize> {
        None
    }

    fn supports_terminal_glass() -> bool {
        Self::macos_major_version().is_none_or(|major| major >= 13)
    }

    fn effective_terminal_opacity(value: f32) -> f32 {
        if !Self::supports_terminal_glass() {
            return 1.0;
        }
        let clamped = Self::clamp_terminal_opacity(value);
        Self::remap_opacity(clamped, 0.25, 0.72, 1.55)
    }

    fn effective_terminal_blur(value: bool) -> bool {
        value && Self::supports_terminal_glass()
    }

    fn effective_ui_opacity(value: f32) -> f32 {
        if !Self::supports_terminal_glass() {
            return 1.0;
        }
        let clamped = Self::clamp_ui_opacity(value);
        Self::remap_opacity(clamped, 0.35, 0.84, 1.9)
    }

    fn ui_surface_opacity(&self) -> f32 {
        Self::effective_ui_opacity(self.ui_opacity)
    }

    fn has_active_tab(&self) -> bool {
        self.active_tab < self.tabs.len()
    }

    fn elevated_ui_surface_opacity(&self) -> f32 {
        (self.ui_surface_opacity() + 0.02).min(0.98)
    }

    pub fn from_session(
        config: Config,
        session: Session,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let sidebar = cx.new(|cx| SessionSidebar::new(cx));
        let terminal_font_family = config.terminal.font_family.clone();
        let ui_font_family = config.appearance.ui_font_family.clone();
        let ui_font_size = config.appearance.ui_font_size;
        let font_size = config.terminal.font_size;
        let terminal_cursor_style = config.terminal.cursor_style.clone();
        let terminal_opacity = Self::effective_terminal_opacity(config.appearance.terminal_opacity);
        let terminal_blur = Self::effective_terminal_blur(config.appearance.terminal_blur);
        let ui_opacity = Self::clamp_ui_opacity(config.appearance.ui_opacity);
        let effective_ui_opacity = Self::effective_ui_opacity(ui_opacity);
        let background_image = config.appearance.background_image.clone();
        let background_image_opacity =
            Self::clamp_background_image_opacity(config.appearance.background_image_opacity);
        let background_image_position = config.appearance.background_image_position.clone();
        let background_image_fit = config.appearance.background_image_fit.clone();
        let background_image_repeat = config.appearance.background_image_repeat;
        let terminal_theme = TerminalTheme::by_name(&config.terminal.theme).unwrap_or_default();
        let colors = theme_to_ghostty_colors(&terminal_theme);
        let ghostty_app = con_ghostty::GhosttyApp::new(
            Some(&colors),
            Some(&terminal_font_family),
            Some(font_size),
            Some(terminal_opacity),
            Some(terminal_blur),
            Some(&terminal_cursor_style),
            background_image.as_deref(),
            Some(background_image_opacity),
            Some(&background_image_position),
            Some(&background_image_fit),
            Some(background_image_repeat),
        )
        .map(std::sync::Arc::new)
        .unwrap_or_else(|e| panic!("Fatal: failed to initialize Ghostty: {}", e));
        let harness = AgentHarness::new(&config).unwrap_or_else(|e| {
            log::error!(
                "Failed to create agent harness: {}. Agent features disabled.",
                e
            );
            panic!("Fatal: agent harness initialization failed: {}", e);
        });
        harness.prewarm_input_classification();
        let shell_suggestion_engine = harness.suggestion_engine(180);
        let session_save_tx = spawn_session_save_worker();
        let (control_request_tx, control_request_rx) = crossbeam_channel::unbounded();
        let (shell_suggestion_tx, shell_suggestion_rx) = crossbeam_channel::unbounded();
        let control_socket = match con_core::spawn_control_socket_server(
            harness.runtime_handle(),
            control_request_tx,
        ) {
            Ok(handle) => Some(handle),
            Err(err) => {
                log::error!("Failed to start con control socket: {}", err);
                None
            }
        };
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
                let pane_tree = if let Some(layout) = &tab_state.layout {
                    let mut restore_terminal =
                        |restore_cwd: Option<&str>| make_terminal(restore_cwd, window, cx);
                    PaneTree::from_state(layout, tab_state.focused_pane_id, &mut restore_terminal)
                } else {
                    let cwd = tab_state.cwd.as_deref();
                    PaneTree::new(make_terminal(cwd, window, cx))
                };
                Tab {
                    pane_tree,
                    title: if tab_state.title.is_empty() {
                        format!("Terminal {}", i + 1)
                    } else {
                        tab_state.title.clone()
                    },
                    needs_attention: false,
                    session: agent_session,
                    panel_state,
                    runtime_trackers: RefCell::new(HashMap::new()),
                    runtime_cache: RefCell::new(HashMap::new()),
                    shell_history: Self::restore_shell_history(tab_state),
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
                runtime_cache: RefCell::new(HashMap::new()),
                shell_history: HashMap::new(),
            });
        }
        let active_tab = session.active_tab.min(tabs.len() - 1);
        let persisted_history = GlobalHistoryState::load().unwrap_or_else(|err| {
            log::warn!("Failed to load command history: {}", err);
            GlobalHistoryState::default()
        });
        let global_shell_history = Self::restore_global_shell_history(&session, &tabs);
        let global_shell_history =
            Self::merge_shell_histories(global_shell_history, &persisted_history);
        let global_input_history =
            Self::restore_global_input_history(&session, &persisted_history, &global_shell_history);
        let agent_panel_open = session.agent_panel_open;
        let agent_panel_width = session
            .agent_panel_width
            .unwrap_or(AGENT_PANEL_DEFAULT_WIDTH);
        // Take the active tab's restored panel state for the AgentPanel
        let initial_panel_state =
            std::mem::replace(&mut tabs[active_tab].panel_state, PanelState::new());
        let agent_panel = cx.new(|cx| {
            let mut panel = AgentPanel::with_state(initial_panel_state, window, cx);
            panel.set_auto_approve(config.agent.auto_approve_tools);
            panel
        });
        let input_bar = cx.new(|cx| InputBar::new(window, cx));
        let registry = model_registry.clone();
        let oauth_runtime = harness.runtime_handle();
        let settings_panel =
            cx.new(|cx| SettingsPanel::new(&config, registry, oauth_runtime, window, cx));
        let command_palette = cx.new(|cx| CommandPalette::new(window, cx));
        let initial_recent_inputs = global_input_history
            .iter()
            .rev()
            .take(80)
            .cloned()
            .collect::<Vec<_>>();
        agent_panel.update(cx, |panel, _cx| {
            panel.set_ui_opacity(effective_ui_opacity);
            panel.set_recent_inputs(initial_recent_inputs.clone());
        });
        input_bar.update(cx, |bar, _cx| {
            bar.set_ui_opacity(effective_ui_opacity);
            bar.set_recent_commands(initial_recent_inputs);
        });
        command_palette.update(cx, |palette, _cx| {
            palette.set_ui_opacity(effective_ui_opacity)
        });
        cx.subscribe_in(&input_bar, window, Self::on_input_submit)
            .detach();
        cx.subscribe_in(&input_bar, window, Self::on_input_edited)
            .detach();
        cx.subscribe_in(&input_bar, window, Self::on_input_scope_changed)
            .detach();
        cx.subscribe_in(&input_bar, window, Self::on_input_escape)
            .detach();
        cx.subscribe_in(&input_bar, window, Self::on_skill_autocomplete_changed)
            .detach();
        cx.subscribe_in(
            &input_bar,
            window,
            Self::on_toggle_pane_scope_picker_requested,
        )
        .detach();
        cx.subscribe_in(&settings_panel, window, Self::on_settings_saved)
            .detach();
        cx.subscribe_in(&settings_panel, window, Self::on_theme_preview)
            .detach();
        // Re-render workspace when settings panel visibility changes (e.g. X close button)
        cx.observe(&settings_panel, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(&command_palette, window, Self::on_palette_select)
            .detach();
        cx.subscribe_in(&command_palette, window, Self::on_palette_dismissed)
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
        cx.subscribe_in(&agent_panel, window, Self::on_set_auto_approve)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_select_session_provider)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_select_session_model)
            .detach();
        cx.subscribe_in(&agent_panel, window, Self::on_rerun_from_message)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_select)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_new_session)
            .detach();
        let workspace_handle = cx.weak_entity();
        window.on_window_should_close(cx, move |_window, cx| {
            let _ = workspace_handle.update(cx, |workspace, cx| {
                workspace.prepare_window_close(cx);
            });
            // On Windows, closing the last window does NOT automatically
            // exit the app — gpui_windows' event loop keeps running with
            // no visible windows, which manifests as a "hang" (the user
            // has to Ctrl+C the launching terminal to kill the process).
            // macOS / NSApplication has its own convention (apps keep
            // running without windows), so leave that platform alone.
            // Con today is a single-window app; the first
            // should_close that returns `true` is always the last
            // window, so quitting here is correct.
            #[cfg(target_os = "windows")]
            {
                cx.quit();
            }
            true
        });

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
                            let suppress_event =
                                workspace.handle_pending_control_agent_event(tab_idx, &event);
                            if suppress_event {
                                continue;
                            }
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

                    while let Ok(request) = workspace.control_request_rx.try_recv() {
                        got_event = true;
                        workspace.handle_control_request(request, cx);
                    }

                    while let Ok(result) = workspace.shell_suggestion_rx.try_recv() {
                        got_event = true;
                        workspace.apply_shell_suggestion(result, cx);
                    }

                    if workspace.pump_ghostty_views(cx) {
                        got_event = true;
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
        let initial_terminal = tabs[active_tab].pane_tree.focused_terminal().clone();
        initial_terminal.focus(window, cx);
        Self::schedule_terminal_bootstrap_reassert(
            &initial_terminal,
            true,
            window.window_handle(),
            cx.weak_entity(),
            cx,
        );

        // Hide non-active tabs' ghostty NSViews so only the active tab is visible
        for (i, tab) in tabs.iter().enumerate() {
            if i != active_tab {
                for terminal in tab.pane_tree.all_terminals() {
                    terminal.set_native_view_visible(false, cx);
                }
            }
        }

        let has_multiple_tabs = tabs.len() > 1;
        let last_ghostty_wake_generation = ghostty_app.wake_generation();

        Self {
            sidebar,
            tabs,
            active_tab,
            terminal_font_family,
            ui_font_family,
            ui_font_size,
            font_size,
            terminal_cursor_style,
            terminal_opacity,
            terminal_blur,
            ui_opacity,
            background_image,
            background_image_opacity,
            background_image_position,
            background_image_fit,
            background_image_repeat,
            agent_panel,
            input_bar,
            settings_panel,
            command_palette,
            model_registry,
            harness,
            shell_suggestion_engine,
            global_shell_history,
            global_input_history,
            pane_scope_picker_open: false,
            agent_panel_open,
            agent_panel_motion: MotionValue::new(if agent_panel_open { 1.0 } else { 0.0 }),
            agent_panel_width,
            tab_strip_motion: MotionValue::new(if has_multiple_tabs { 1.0 } else { 0.0 }),
            input_bar_visible: session.input_bar_visible,
            input_bar_motion: MotionValue::new(if session.input_bar_visible { 1.0 } else { 0.0 }),
            modal_was_open: false,
            ghostty_hidden: false,
            pending_drag_init: std::sync::Arc::new(std::sync::Mutex::new(None)),
            agent_panel_drag: None,
            terminal_theme,
            ghostty_app,
            last_ghostty_wake_generation,
            #[cfg(target_os = "macos")]
            last_window_resize_increment_millipoints: None,
            pending_create_pane_requests: Vec::new(),
            pending_window_control_requests: Vec::new(),
            control_request_rx,
            control_socket,
            pending_control_agent_requests: HashMap::new(),
            shell_suggestion_rx,
            shell_suggestion_tx,
            next_control_agent_request_id: 1,
            window_handle: window.window_handle(),
            workspace_handle: cx.weak_entity(),
            window_close_prepared: false,
            session_save_tx,
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

    fn sync_tab_strip_motion(&mut self) {
        self.tab_strip_motion.set_target(
            if self.tabs.len() > 1 { 1.0 } else { 0.0 },
            std::time::Duration::from_millis(180),
        );
    }

    fn current_top_bar_height(&self) -> f32 {
        if self.tab_strip_motion.is_animating() || self.tabs.len() > 1 {
            TOP_BAR_TABS_HEIGHT
        } else {
            TOP_BAR_COMPACT_HEIGHT
        }
    }

    fn active_terminal(&self) -> &TerminalPane {
        self.tabs[self.active_tab].pane_tree.focused_terminal()
    }

    fn sync_active_terminal_focus_states(&self, cx: &mut App) {
        if !self.has_active_tab() {
            return;
        }

        let pane_tree = &self.tabs[self.active_tab].pane_tree;
        let pane_terminals = pane_tree.pane_terminals();
        let focused_id = pane_tree.focused_pane_id();
        let (mode, is_broadcast, is_focused, selected_ids) = {
            let input_bar = self.input_bar.read(cx);
            (
                input_bar.mode(),
                input_bar.is_broadcast_scope(),
                input_bar.is_focused_scope(),
                input_bar.scope_selected_ids(),
            )
        };
        let all_ids = pane_terminals
            .iter()
            .map(|(pane_id, _)| *pane_id)
            .collect::<HashSet<_>>();

        let target_ids: HashSet<usize> = if mode == InputMode::Agent || is_focused {
            [focused_id].into_iter().collect()
        } else if is_broadcast {
            all_ids
        } else {
            let selected = selected_ids
                .into_iter()
                .filter(|pane_id| all_ids.contains(pane_id))
                .collect::<HashSet<_>>();
            if selected.is_empty() {
                [focused_id].into_iter().collect()
            } else {
                selected
            }
        };

        for (pane_id, terminal) in pane_terminals {
            terminal.set_focus_state(target_ids.contains(&pane_id), cx);
        }
    }

    fn pump_ghostty_views(&mut self, cx: &mut Context<Self>) -> bool {
        let started = perf_trace_enabled().then(std::time::Instant::now);
        let mut changed = false;
        let mut terminal_count = 0usize;
        let mut drain_count = 0usize;

        for tab in &self.tabs {
            for terminal in tab.pane_tree.all_terminals() {
                terminal_count += 1;
                changed |= terminal.pump_surface_deferred_work(cx);
            }
        }

        let generation = self.ghostty_app.wake_generation();
        if generation == self.last_ghostty_wake_generation {
            if let Some(started) = started {
                if changed {
                    log::info!(
                        target: "con::perf",
                        "pump_ghostty_views generation_unchanged terminals={} changed=1 elapsed_ms={:.3}",
                        terminal_count,
                        started.elapsed().as_secs_f64() * 1000.0
                    );
                }
            }
            return changed;
        }

        self.last_ghostty_wake_generation = generation;
        for tab in &self.tabs {
            for terminal in tab.pane_tree.all_terminals() {
                drain_count += 1;
                changed |= terminal.drain_surface_state(cx);
            }
        }

        if let Some(started) = started {
            log::info!(
                target: "con::perf",
                "pump_ghostty_views generation={} terminals={} drains={} changed={} elapsed_ms={:.3}",
                generation,
                terminal_count,
                drain_count,
                changed,
                started.elapsed().as_secs_f64() * 1000.0
            );
        }

        changed
    }

    #[cfg(target_os = "macos")]
    fn sync_window_resize_increments(&mut self, window: &mut Window, cx: &App) {
        let Some((cell_width_px, cell_height_px)) = self.active_terminal().cell_size_px(cx) else {
            return;
        };
        if cell_width_px == 0 || cell_height_px == 0 {
            return;
        }

        let scale = window.scale_factor().max(1.0);
        let width_pt = (cell_width_px as f32 / scale).max(1.0);
        let height_pt = (cell_height_px as f32 / scale).max(1.0);
        let key = (
            (width_pt * 1000.0).round() as u32,
            (height_pt * 1000.0).round() as u32,
        );
        if self.last_window_resize_increment_millipoints == Some(key) {
            return;
        }

        let raw_handle: raw_window_handle::WindowHandle<'_> =
            match <Window as raw_window_handle::HasWindowHandle>::window_handle(window) {
            Ok(handle) => handle,
            Err(_) => return,
        };
        let gpui_nsview = match raw_handle.as_raw() {
            raw_window_handle::RawWindowHandle::AppKit(handle) => handle.ns_view.as_ptr() as id,
            _ => return,
        };

        unsafe {
            let nswindow: id = msg_send![gpui_nsview, window];
            if nswindow.is_null() {
                return;
            }
            let _: () = msg_send![
                nswindow,
                setContentResizeIncrements: NSSize::new(f64::from(width_pt), f64::from(height_pt))
            ];
        }
        self.last_window_resize_increment_millipoints = Some(key);
    }

    #[cfg(not(target_os = "macos"))]
    fn sync_window_resize_increments(&mut self, _window: &mut Window, _cx: &App) {}

    fn schedule_terminal_bootstrap_reassert(
        terminal: &TerminalPane,
        should_focus: bool,
        window_handle: AnyWindowHandle,
        workspace_handle: WeakEntity<Self>,
        cx: &mut Context<Self>,
    ) {
        let terminal = terminal.clone();
        cx.spawn(async move |_, cx| {
            for attempt in 0..8 {
                let delay_ms = if attempt == 0 { 16 } else { 250 };
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(delay_ms))
                    .await;
                let ready = window_handle
                    .update(cx, |_root, window, cx| {
                        let Some(workspace) = workspace_handle.upgrade() else {
                            return false;
                        };
                        workspace.update(cx, |_workspace, cx| {
                            terminal.ensure_surface(window, cx);
                            terminal.notify(cx);
                            terminal.set_native_view_visible(true, cx);
                            if should_focus {
                                _workspace.sync_active_terminal_focus_states(cx);
                                terminal.focus(window, cx);
                            } else {
                                terminal.set_focus_state(false, cx);
                            }
                            if terminal.surface_ready(cx) {
                                terminal.recover_shell_prompt_state(cx);
                                true
                            } else {
                                false
                            }
                        })
                    })
                    .unwrap_or(false);
                if ready {
                    break;
                }
            }
        })
        .detach();
    }

    fn schedule_pending_create_pane_flush(&self, cx: &mut Context<Self>) {
        let window_handle = self.window_handle;
        let workspace_handle = self.workspace_handle.clone();
        cx.defer(move |cx| {
            let result = window_handle.update(cx, |_root, window, cx| {
                if let Some(workspace) = workspace_handle.upgrade() {
                    let _ = workspace.update(cx, |workspace, cx| {
                        workspace.flush_pending_window_control_requests(window, cx);
                        workspace.flush_pending_create_pane_requests(window, cx);
                    });
                }
            });
            if let Err(err) = result {
                log::warn!(
                    "[control] failed to flush deferred pane creation in a window-aware context: {err}"
                );
            }
        });
    }

    fn flush_pending_window_control_requests(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pending = std::mem::take(&mut self.pending_window_control_requests);
        if pending.is_empty() {
            return;
        }

        for request in pending {
            match request {
                PendingWindowControlRequest::TabsNew { response_tx } => {
                    self.new_tab(&NewTab, window, cx);
                    let result = Ok(json!({
                        "active_tab_index": self.active_tab + 1,
                        "tab_count": self.tabs.len(),
                        "focused_pane_id": self.tabs[self.active_tab].pane_tree.focused_pane_id(),
                    }));
                    Self::send_control_result(response_tx, result);
                }
                PendingWindowControlRequest::TabsClose {
                    tab_idx,
                    response_tx,
                } => {
                    if self.tabs.len() <= 1 {
                        Self::send_control_result(
                            response_tx,
                            Err(ControlError::invalid_params(
                                "refusing to close the last tab over the control plane",
                            )),
                        );
                        continue;
                    }
                    if tab_idx >= self.tabs.len() {
                        Self::send_control_result(
                            response_tx,
                            Err(ControlError::invalid_params(format!(
                                "Tab index {} is out of range. Valid tabs are 1..={}.",
                                tab_idx + 1,
                                self.tabs.len()
                            ))),
                        );
                        continue;
                    }
                    self.close_tab_by_index(tab_idx, window, cx);
                    let result = Ok(json!({
                        "active_tab_index": self.active_tab + 1,
                        "tab_count": self.tabs.len(),
                        "closed_tab_index": tab_idx + 1,
                    }));
                    Self::send_control_result(response_tx, result);
                }
            }
        }
    }

    fn flush_pending_create_pane_requests(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pending = std::mem::take(&mut self.pending_create_pane_requests);
        if pending.is_empty() {
            return;
        }

        for req in pending {
            let terminal = self.create_terminal(None, window, cx);
            let direction = match req.location {
                con_agent::tools::PaneCreateLocation::Right => SplitDirection::Horizontal,
                con_agent::tools::PaneCreateLocation::Down => SplitDirection::Vertical,
            };
            self.tabs[req.tab_idx]
                .pane_tree
                .split(direction, terminal.clone());
            terminal.ensure_surface(window, cx);
            terminal.notify(cx);
            let should_focus = req.tab_idx == self.active_tab;
            if should_focus {
                terminal.focus(window, cx);
                self.sync_active_terminal_focus_states(cx);
            } else {
                terminal.set_focus_state(false, cx);
            }
            Self::schedule_terminal_bootstrap_reassert(
                &terminal,
                should_focus,
                self.window_handle,
                self.workspace_handle.clone(),
                cx,
            );

            if let Some(cmd) = &req.command {
                let cmd_with_newline = format!("{}\n", cmd);
                terminal.write(cmd_with_newline.as_bytes(), cx);
            }
            self.record_runtime_event_for_terminal(
                req.tab_idx,
                &terminal,
                con_agent::context::PaneRuntimeEvent::PaneCreated {
                    startup_command: req.command.clone(),
                },
            );

            let pane_tree = &self.tabs[req.tab_idx].pane_tree;
            let pane_index = pane_tree
                .all_terminals()
                .iter()
                .enumerate()
                .find(|(_, pane)| pane.entity_id() == terminal.entity_id())
                .map(|(idx, _)| idx + 1)
                .unwrap_or_else(|| pane_tree.pane_count());
            let pane_id = pane_tree
                .pane_id_for_terminal(&terminal)
                .unwrap_or(pane_index);
            let _ = req.response_tx.send(con_agent::PaneResponse::PaneCreated {
                pane_index,
                pane_id,
                surface_ready: terminal.surface_ready(cx),
                is_alive: terminal.is_alive(cx),
                has_shell_integration: terminal.has_shell_integration(cx),
            });
        }

        cx.notify();
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
        self.tabs[tab_idx]
            .runtime_cache
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
        self.tabs[tab_idx]
            .runtime_cache
            .borrow_mut()
            .insert(pane_id, runtime.clone());
        (observation, runtime)
    }

    fn cached_runtime_for_tab(
        &self,
        tab_idx: usize,
        terminal: &TerminalPane,
    ) -> Option<con_agent::context::PaneRuntimeState> {
        let pane_id = self.tabs[tab_idx]
            .pane_tree
            .pane_id_for_terminal(terminal)?;
        self.tabs[tab_idx]
            .runtime_cache
            .borrow()
            .get(&pane_id)
            .cloned()
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

    fn resolve_pane_target_for_tab(
        &self,
        tab_idx: usize,
        selector: con_agent::tools::PaneSelector,
    ) -> Result<ResolvedPaneTarget, String> {
        let pane_tree = &self.tabs[tab_idx].pane_tree;
        let all_terminals = pane_tree.all_terminals();
        let focused_pane = pane_tree.focused_terminal().clone();
        let focused_pane_id = pane_tree.focused_pane_id();
        let focused_pane_index = all_terminals
            .iter()
            .enumerate()
            .find(|(_, terminal)| pane_tree.pane_id_for_terminal(terminal) == Some(focused_pane_id))
            .map(|(idx, _)| idx + 1)
            .unwrap_or(1);

        let by_id = match selector.pane_id {
            Some(pane_id) => {
                let pane = all_terminals
                    .iter()
                    .enumerate()
                    .find_map(|(idx, terminal)| {
                        (pane_tree.pane_id_for_terminal(terminal) == Some(pane_id)).then(|| {
                            ResolvedPaneTarget {
                                pane: (*terminal).clone(),
                                pane_index: idx + 1,
                                pane_id,
                            }
                        })
                    })
                    .ok_or_else(|| {
                        format!(
                            "Pane id {} is no longer available in this tab. That pane was likely closed. Re-run list_panes to choose a new target.",
                            pane_id
                        )
                    })?;
                Some(pane)
            }
            None => None,
        };

        let by_index = match selector.pane_index {
            Some(index) => {
                if index == 0 || index > all_terminals.len() {
                    if let Some(from_id) = by_id.clone() {
                        log::warn!(
                            "Pane selector index {} is stale; continuing with pane_id {}",
                            index,
                            from_id.pane_id
                        );
                        None
                    } else {
                        return Err(format!(
                            "Pane index {} is no longer valid in the current layout. The pane layout changed or that pane was closed. Re-run list_panes and prefer pane_id for follow-up targeting.",
                            index,
                        ));
                    }
                } else {
                    let pane = all_terminals[index - 1].clone();
                    let pane_id = match pane_tree.pane_id_for_terminal(&pane) {
                        Some(pane_id) => pane_id,
                        None if by_id.is_some() => {
                            let from_id = by_id.as_ref().expect("checked is_some");
                            log::warn!(
                                "Pane selector index {} no longer resolves to a live pane; continuing with pane_id {}",
                                index,
                                from_id.pane_id
                            );
                            return Ok(from_id.clone());
                        }
                        None => {
                            return Err(format!(
                                "Pane index {} no longer resolves to a live pane. Re-run list_panes and prefer pane_id for follow-up targeting.",
                                index
                            ));
                        }
                    };
                    Some(ResolvedPaneTarget {
                        pane,
                        pane_index: index,
                        pane_id,
                    })
                }
            }
            None => None,
        };

        match (by_index, by_id) {
            (Some(from_index), Some(from_id)) => {
                if from_index.pane_id != from_id.pane_id {
                    log::warn!(
                        "Pane selector mismatch: pane_index {} resolved to pane_id {}, but caller also supplied pane_id {}; continuing with pane_id",
                        from_index.pane_index,
                        from_index.pane_id,
                        from_id.pane_id
                    );
                    return Ok(from_id);
                }
                Ok(from_id)
            }
            (Some(target), None) => Ok(target),
            (None, Some(target)) => Ok(target),
            (None, None) => Ok(ResolvedPaneTarget {
                pane: focused_pane,
                pane_index: focused_pane_index,
                pane_id: focused_pane_id,
            }),
        }
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
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(250))
                    .await;

                let lines = this
                    .update(cx, |_, cx| pane.recent_lines(400, cx))
                    .unwrap_or_default();

                match parse_response(lines.clone()) {
                    Ok(response) => {
                        let _ = this.update(cx, |_, cx| {
                            pane.recover_shell_prompt_state(cx);
                        });
                        let _ = response_tx.send(response);
                        return;
                    }
                    Err(_) => {}
                }

                if std::time::Instant::now() >= deadline {
                    let excerpt = this
                        .update(cx, |_, cx| pane.recent_lines(120, cx).join("\n"))
                        .unwrap_or_default();
                    let _ = response_tx.send(con_agent::PaneResponse::Error(format!(
                        "Shell-anchor command timed out in pane {} after {}s.\nRecent output:\n{}",
                        pane_index, timeout_secs, excerpt
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

                let excerpt = this
                    .update(cx, |_, cx| pane.recent_lines(120, cx).join("\n"))
                    .unwrap_or_default();
                let parse_err = parse_response(lines).err().unwrap_or_else(|| {
                    "shell-anchor markers were not observed in pane output".to_string()
                });
                let _ = response_tx.send(con_agent::PaneResponse::Error(format!(
                    "Shell-anchor command in pane {} could not be parsed: {}\nRecent output:\n{}",
                    pane_index, parse_err, excerpt
                )));
                return;
            }
        })
        .detach();
    }

    fn pane_blocks_shell_anchor_control(pane: &TerminalPane, cx: &App) -> bool {
        if !pane.is_busy(cx) {
            return false;
        }

        let observation = pane.observation_frame(40, cx);
        let prompt_like = observation
            .screen_hints
            .iter()
            .any(|hint| hint.kind == con_agent::context::PaneObservationHintKind::PromptLikeInput);

        if prompt_like {
            pane.recover_shell_prompt_state(cx);
            return false;
        }

        true
    }

    fn effective_remote_host_for_tab(
        &self,
        tab_idx: usize,
        terminal: &TerminalPane,
        cx: &App,
    ) -> Option<String> {
        self.cached_runtime_for_tab(tab_idx, terminal)
            .map(|runtime| runtime.remote_host)
            .unwrap_or_else(|| {
                self.observe_terminal_runtime_for_tab(tab_idx, terminal, 12, cx)
                    .1
                    .remote_host
            })
    }

    /// Build agent context from a tab's focused pane, including summaries of peer panes.
    fn build_agent_context_for_tab(&self, tab_idx: usize, cx: &App) -> con_agent::TerminalContext {
        self.reconcile_runtime_trackers_for_tab(tab_idx);
        let pane_tree = &self.tabs[tab_idx].pane_tree;
        let focused = pane_tree.focused_terminal();

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
            self.observe_terminal_runtime_for_tab(tab_idx, focused, 50, cx);

        let mut other_pane_summaries = Vec::new();
        if pane_tree.pane_count() > 1 {
            for (idx, terminal) in all_terminals.iter().enumerate() {
                if let Some(pid) = pane_tree.pane_id_for_terminal(terminal) {
                    if pid == focused_pid {
                        continue;
                    }
                    let (observation, runtime) = self.observe_terminal_runtime_for_tab(
                        tab_idx,
                        terminal,
                        Self::SECONDARY_PANE_OBSERVATION_LINES,
                        cx,
                    );
                    let control = con_agent::control::PaneControlState::from_runtime(&runtime);
                    let remote_workspace =
                        con_agent::context::remote_workspace_anchor(&runtime, &observation);
                    let workspace_cwd_hint = con_agent::context::workspace_cwd_hint(
                        observation.cwd.as_deref(),
                        &runtime.recent_actions,
                    );
                    other_pane_summaries.push(con_agent::context::PaneSummary {
                        pane_index: idx + 1,
                        pane_id: pid,
                        hostname: runtime.remote_host.clone(),
                        hostname_confidence: runtime.remote_host_confidence,
                        hostname_source: runtime.remote_host_source,
                        remote_workspace,
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
                        workspace_cwd_hint,
                        workspace_agent_cli_hint: con_agent::context::workspace_agent_cli_hint(
                            runtime.agent_cli.as_deref(),
                            &runtime.recent_actions,
                        ),
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
            focused_pid,
            &focused_observation,
            &focused_runtime,
            other_pane_summaries,
        )
    }

    /// Build agent context from the active tab.
    fn build_agent_context(&self, cx: &App) -> con_agent::TerminalContext {
        self.build_agent_context_for_tab(self.active_tab, cx)
    }

    fn resolve_control_tab_index(&self, tab_index: Option<usize>) -> Result<usize, ControlError> {
        match tab_index {
            Some(index) if index == 0 || index > self.tabs.len() => {
                Err(ControlError::invalid_params(format!(
                    "Tab index {} is out of range. Valid tabs are 1..={}.",
                    index,
                    self.tabs.len()
                )))
            }
            Some(index) => Ok(index - 1),
            None => Ok(self.active_tab),
        }
    }

    fn pane_selector_from_target(target: con_core::PaneTarget) -> con_agent::tools::PaneSelector {
        con_agent::tools::PaneSelector::new(target.pane_index, target.pane_id)
    }

    fn send_control_result(
        response_tx: tokio::sync::oneshot::Sender<ControlResult>,
        result: ControlResult,
    ) {
        let _ = response_tx.send(result);
    }

    fn pane_response_to_control_result(
        tab_idx: usize,
        response: con_agent::PaneResponse,
    ) -> ControlResult {
        match response {
            con_agent::PaneResponse::PaneList(panes) => Ok(json!({
                "tab_index": tab_idx + 1,
                "panes": panes,
            })),
            con_agent::PaneResponse::Content(content) => Ok(json!({ "content": content })),
            con_agent::PaneResponse::KeysSent => Ok(json!({ "status": "sent" })),
            con_agent::PaneResponse::TmuxInfo(tmux) => Ok(json!({
                "tab_index": tab_idx + 1,
                "tmux": tmux,
            })),
            con_agent::PaneResponse::TmuxList(snapshot) => Ok(json!({
                "tab_index": tab_idx + 1,
                "snapshot": snapshot,
            })),
            con_agent::PaneResponse::TmuxCapture(capture) => Ok(json!({
                "tab_index": tab_idx + 1,
                "capture": capture,
            })),
            con_agent::PaneResponse::TmuxExec(exec) => Ok(json!({
                "tab_index": tab_idx + 1,
                "exec": exec,
            })),
            con_agent::PaneResponse::ShellProbe(shell) => Ok(json!({
                "tab_index": tab_idx + 1,
                "shell": shell,
            })),
            con_agent::PaneResponse::SearchResults(matches) => Ok(json!({
                "matches": matches,
            })),
            con_agent::PaneResponse::BusyStatus {
                surface_ready,
                is_alive,
                is_busy,
                has_shell_integration,
            } => Ok(json!({
                "surface_ready": surface_ready,
                "is_alive": is_alive,
                "is_busy": is_busy,
                "has_shell_integration": has_shell_integration,
            })),
            con_agent::PaneResponse::WaitComplete { status, output } => Ok(json!({
                "status": status,
                "output": output,
            })),
            con_agent::PaneResponse::PaneCreated {
                pane_index,
                pane_id,
                surface_ready,
                is_alive,
                has_shell_integration,
            } => Ok(json!({
                "tab_index": tab_idx + 1,
                "pane_index": pane_index,
                "pane_id": pane_id,
                "surface_ready": surface_ready,
                "is_alive": is_alive,
                "has_shell_integration": has_shell_integration,
            })),
            con_agent::PaneResponse::Error(err) => Err(ControlError::invalid_params(err)),
        }
    }

    fn terminal_exec_response_to_control_result(response: TerminalExecResponse) -> ControlResult {
        Ok(json!({
            "output": response.output,
            "exit_code": response.exit_code,
        }))
    }

    fn spawn_control_pane_query(
        &mut self,
        tab_idx: usize,
        query: con_agent::PaneQuery,
        response_tx: tokio::sync::oneshot::Sender<ControlResult>,
        cx: &mut Context<Self>,
    ) {
        let (pane_response_tx, pane_response_rx) = crossbeam_channel::bounded(1);
        self.handle_pane_request_for_tab(
            tab_idx,
            con_agent::PaneRequest {
                query,
                response_tx: pane_response_tx,
            },
            cx,
        );

        self.harness.spawn_detached(async move {
            let result = match tokio::task::spawn_blocking(move || {
                pane_response_rx.recv_timeout(std::time::Duration::from_secs(240))
            })
            .await
            {
                Ok(Ok(response)) => Self::pane_response_to_control_result(tab_idx, response),
                Ok(Err(_)) => Err(ControlError::internal(
                    "Timed out waiting for the pane operation to finish",
                )),
                Err(err) => Err(ControlError::internal(format!(
                    "Pane operation join failed: {err}"
                ))),
            };
            Self::send_control_result(response_tx, result);
        });
    }

    fn spawn_control_terminal_exec(
        &mut self,
        tab_idx: usize,
        command: String,
        target: con_core::PaneTarget,
        response_tx: tokio::sync::oneshot::Sender<ControlResult>,
        cx: &mut Context<Self>,
    ) {
        let (exec_response_tx, exec_response_rx) = crossbeam_channel::bounded(1);
        self.handle_terminal_exec_request_for_tab(
            tab_idx,
            TerminalExecRequest {
                command,
                working_dir: None,
                target: Self::pane_selector_from_target(target),
                response_tx: exec_response_tx,
            },
            cx,
        );

        self.harness.spawn_detached(async move {
            let result = match tokio::task::spawn_blocking(move || {
                exec_response_rx.recv_timeout(std::time::Duration::from_secs(240))
            })
            .await
            {
                Ok(Ok(response)) => Self::terminal_exec_response_to_control_result(response),
                Ok(Err(_)) => Err(ControlError::internal(
                    "Timed out waiting for the visible shell command to finish",
                )),
                Err(err) => Err(ControlError::internal(format!(
                    "Visible shell join failed: {err}"
                ))),
            };
            Self::send_control_result(response_tx, result);
        });
    }

    fn handle_pending_control_agent_event(&mut self, tab_idx: usize, event: &HarnessEvent) -> bool {
        let auto_approve = self
            .pending_control_agent_requests
            .get(&tab_idx)
            .map(|pending| pending.auto_approve_tools)
            .unwrap_or(false);

        if auto_approve {
            if let HarnessEvent::ToolApprovalNeeded {
                call_id,
                approval_tx,
                ..
            } = event
            {
                let _ = approval_tx.send(con_agent::ToolApprovalDecision {
                    call_id: call_id.clone(),
                    allowed: true,
                    reason: Some("auto-approved by con-cli".to_string()),
                });
                return true;
            }
        }

        match event {
            HarnessEvent::ResponseComplete(message) => {
                if let Some(pending) = self.pending_control_agent_requests.remove(&tab_idx) {
                    let result = serde_json::to_value(AgentAskResult {
                        tab_index: tab_idx + 1,
                        conversation_id: self.tabs[tab_idx].session.conversation_id(),
                        prompt: pending.prompt,
                        message: message.clone(),
                    })
                    .map_err(|err| {
                        ControlError::internal(format!(
                            "Failed to serialize agent response for control output: {err}"
                        ))
                    });
                    Self::send_control_result(pending.response_tx, result);
                }
            }
            HarnessEvent::Error(err) => {
                if err.starts_with("Retrying (") {
                    return false;
                }
                if let Some(pending) = self.pending_control_agent_requests.remove(&tab_idx) {
                    Self::send_control_result(
                        pending.response_tx,
                        Err(ControlError::internal(err.clone())),
                    );
                }
            }
            _ => {}
        }

        false
    }

    fn handle_control_request(&mut self, request: ControlRequestEnvelope, cx: &mut Context<Self>) {
        let ControlRequestEnvelope {
            command,
            response_tx,
        } = request;

        match command {
            ControlCommand::SystemIdentify => {
                let result = serde_json::to_value(SystemIdentifyResult {
                    app: "con".to_string(),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    socket_path: self
                        .control_socket
                        .as_ref()
                        .map(|handle| handle.path().display().to_string())
                        .unwrap_or_else(|| con_core::control_socket_path().display().to_string()),
                    active_tab_index: self.active_tab + 1,
                    tab_count: self.tabs.len(),
                    methods: con_core::control_methods(),
                })
                .map_err(|err| ControlError::internal(err.to_string()));
                Self::send_control_result(response_tx, result);
            }
            ControlCommand::SystemCapabilities => {
                Self::send_control_result(
                    response_tx,
                    Ok(json!({ "methods": con_core::control_methods() })),
                );
            }
            ControlCommand::TabsList => {
                let tabs = self
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(idx, tab)| TabInfo {
                        index: idx + 1,
                        title: tab
                            .pane_tree
                            .focused_terminal()
                            .title(cx)
                            .unwrap_or_else(|| tab.title.clone()),
                        is_active: idx == self.active_tab,
                        pane_count: tab.pane_tree.pane_count(),
                        focused_pane_id: tab.pane_tree.focused_pane_id(),
                        needs_attention: tab.needs_attention,
                        conversation_id: tab.session.conversation_id(),
                    })
                    .collect::<Vec<_>>();
                Self::send_control_result(
                    response_tx,
                    Ok(json!({
                        "active_tab_index": self.active_tab + 1,
                        "tabs": tabs,
                    })),
                );
            }
            ControlCommand::TabsNew => {
                self.pending_window_control_requests
                    .push(PendingWindowControlRequest::TabsNew { response_tx });
                self.schedule_pending_create_pane_flush(cx);
            }
            ControlCommand::TabsClose { tab_index } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => {
                        self.pending_window_control_requests.push(
                            PendingWindowControlRequest::TabsClose {
                                tab_idx,
                                response_tx,
                            },
                        );
                        self.schedule_pending_create_pane_flush(cx);
                    }
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::PanesList { tab_index } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => {
                        self.spawn_control_pane_query(
                            tab_idx,
                            con_agent::PaneQuery::List,
                            response_tx,
                            cx,
                        );
                    }
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::PanesRead {
                tab_index,
                target,
                lines,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => self.spawn_control_pane_query(
                    tab_idx,
                    con_agent::PaneQuery::ReadContent {
                        target: Self::pane_selector_from_target(target),
                        lines,
                    },
                    response_tx,
                    cx,
                ),
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::PanesExec {
                tab_index,
                target,
                command,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => {
                    self.spawn_control_terminal_exec(tab_idx, command, target, response_tx, cx)
                }
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::PanesSendKeys {
                tab_index,
                target,
                keys,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => self.spawn_control_pane_query(
                    tab_idx,
                    con_agent::PaneQuery::SendKeys {
                        target: Self::pane_selector_from_target(target),
                        keys,
                    },
                    response_tx,
                    cx,
                ),
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::PanesCreate {
                tab_index,
                location,
                command,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => self.spawn_control_pane_query(
                    tab_idx,
                    con_agent::PaneQuery::CreatePane { command, location },
                    response_tx,
                    cx,
                ),
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::PanesWait {
                tab_index,
                target,
                timeout_secs,
                pattern,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => self.spawn_control_pane_query(
                    tab_idx,
                    con_agent::PaneQuery::WaitFor {
                        target: Self::pane_selector_from_target(target),
                        timeout_secs,
                        pattern,
                    },
                    response_tx,
                    cx,
                ),
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::PanesProbeShell { tab_index, target } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => self.spawn_control_pane_query(
                        tab_idx,
                        con_agent::PaneQuery::ProbeShellContext {
                            target: Self::pane_selector_from_target(target),
                        },
                        response_tx,
                        cx,
                    ),
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::TmuxInspect { tab_index, target } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => self.spawn_control_pane_query(
                        tab_idx,
                        con_agent::PaneQuery::InspectTmux {
                            target: Self::pane_selector_from_target(target),
                        },
                        response_tx,
                        cx,
                    ),
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::TmuxList { tab_index, target } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => self.spawn_control_pane_query(
                        tab_idx,
                        con_agent::PaneQuery::TmuxList {
                            pane: Self::pane_selector_from_target(target),
                        },
                        response_tx,
                        cx,
                    ),
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::TmuxCapture {
                tab_index,
                pane,
                target,
                lines,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => self.spawn_control_pane_query(
                    tab_idx,
                    con_agent::PaneQuery::TmuxCapture {
                        pane: Self::pane_selector_from_target(pane),
                        target,
                        lines,
                    },
                    response_tx,
                    cx,
                ),
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::TmuxSendKeys {
                tab_index,
                pane,
                target,
                literal_text,
                key_names,
                append_enter,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => self.spawn_control_pane_query(
                    tab_idx,
                    con_agent::PaneQuery::TmuxSendKeys {
                        pane: Self::pane_selector_from_target(pane),
                        target,
                        literal_text,
                        key_names,
                        append_enter,
                    },
                    response_tx,
                    cx,
                ),
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::TmuxRun {
                tab_index,
                pane,
                target,
                location,
                command,
                window_name,
                cwd,
                detached,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => self.spawn_control_pane_query(
                    tab_idx,
                    con_agent::PaneQuery::TmuxRunCommand {
                        pane: Self::pane_selector_from_target(pane),
                        target,
                        location,
                        command,
                        window_name,
                        cwd,
                        detached,
                    },
                    response_tx,
                    cx,
                ),
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::AgentNewConversation { tab_index } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => {
                        self.tabs[tab_idx].session.new_conversation();
                        if tab_idx == self.active_tab {
                            self.agent_panel.update(cx, |panel, cx| {
                                panel.clear_messages(cx);
                            });
                        } else {
                            self.tabs[tab_idx].panel_state.clear();
                        }
                        self.save_session(cx);
                        Self::send_control_result(
                            response_tx,
                            Ok(json!({
                                "tab_index": tab_idx + 1,
                                "conversation_id": self.tabs[tab_idx].session.conversation_id(),
                            })),
                        );
                    }
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::AgentAsk {
                tab_index,
                prompt,
                auto_approve_tools,
                timeout_secs,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => {
                    if prompt.trim().is_empty() {
                        Self::send_control_result(
                            response_tx,
                            Err(ControlError::invalid_params(
                                "agent.ask requires a non-empty prompt",
                            )),
                        );
                        return;
                    }
                    if self.pending_control_agent_requests.contains_key(&tab_idx) {
                        Self::send_control_result(
                            response_tx,
                            Err(ControlError::invalid_params(format!(
                                "Tab {} already has a pending con-cli agent request",
                                tab_idx + 1
                            ))),
                        );
                        return;
                    }

                    if tab_idx == self.active_tab {
                        self.agent_panel.update(cx, |panel, cx| {
                            panel.add_message("user", &prompt, cx);
                        });
                    } else {
                        self.tabs[tab_idx].panel_state.add_message("user", &prompt);
                    }

                    let context = self.build_agent_context_for_tab(tab_idx, cx);
                    let session = &self.tabs[tab_idx].session;
                    let request_id = self.next_control_agent_request_id;
                    self.next_control_agent_request_id =
                        self.next_control_agent_request_id.wrapping_add(1);
                    self.pending_control_agent_requests.insert(
                        tab_idx,
                        PendingControlAgentRequest {
                            request_id,
                            prompt: prompt.clone(),
                            auto_approve_tools,
                            response_tx,
                        },
                    );
                    if let Some(timeout_secs) = timeout_secs.map(|secs| secs.clamp(5, 600)) {
                        self.spawn_control_agent_request_timeout(
                            tab_idx,
                            request_id,
                            timeout_secs,
                            cx,
                        );
                    }
                    self.harness.send_message(session, prompt, context);
                }
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
        }
    }

    fn spawn_control_agent_request_timeout(
        &self,
        tab_idx: usize,
        request_id: u64,
        timeout_secs: u64,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(timeout_secs))
                .await;

            let _ = this.update(cx, |workspace, _| {
                let is_current_request = workspace
                    .pending_control_agent_requests
                    .get(&tab_idx)
                    .is_some_and(|pending| pending.request_id == request_id);
                if is_current_request {
                    let pending = workspace
                        .pending_control_agent_requests
                        .remove(&tab_idx)
                        .expect("pending request must exist");
                    Self::send_control_result(
                        pending.response_tx,
                        Err(ControlError::internal(format!(
                            "agent.ask timed out after {timeout_secs}s"
                        ))),
                    );
                }
            });
        })
        .detach();
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
                    state.restore_message("user", &visible, None, None);
                }
                con_agent::MessageRole::Assistant => {
                    state.restore_message(
                        "assistant",
                        &msg.content,
                        msg.model.as_deref(),
                        msg.duration_ms,
                    );
                    state.restore_last_assistant_trace(msg.thinking.as_deref(), &msg.steps);
                }
                con_agent::MessageRole::System | con_agent::MessageRole::Tool => {
                    state.restore_message("system", &msg.content, None, None);
                }
            }
        }
        state
    }

    fn snapshot_session(&self, cx: &App) -> Session {
        let tabs: Vec<con_core::session::TabState> = self
            .tabs
            .iter()
            .map(|tab| {
                let terminal = tab.pane_tree.focused_terminal();
                let cwd = terminal.current_dir(cx);
                let title = terminal.title(cx).unwrap_or_else(|| tab.title.clone());
                let pane_layout = tab.pane_tree.to_state(cx);
                let pane_states = tab
                    .pane_tree
                    .pane_terminals()
                    .into_iter()
                    .map(|(_, terminal)| con_core::session::PaneState {
                        cwd: terminal.current_dir(cx),
                    })
                    .collect();
                let shell_history = tab
                    .shell_history
                    .iter()
                    .map(
                        |(pane_id, entries)| con_core::session::PaneCommandHistoryState {
                            pane_id: Some(*pane_id),
                            entries: entries
                                .iter()
                                .map(|entry| con_core::session::CommandHistoryEntryState {
                                    command: entry.command.clone(),
                                    cwd: entry.cwd.clone(),
                                })
                                .collect(),
                        },
                    )
                    .collect();
                con_core::session::TabState {
                    title,
                    cwd,
                    layout: Some(pane_layout),
                    focused_pane_id: Some(tab.pane_tree.focused_pane_id()),
                    panes: pane_states,
                    shell_history,
                    conversation_id: Some(tab.session.conversation_id()),
                }
            })
            .collect();

        Session {
            tabs,
            active_tab: self.active_tab,
            agent_panel_open: self.agent_panel_open,
            agent_panel_width: Some(self.agent_panel_width),
            input_bar_visible: self.input_bar_visible,
            global_shell_history: self
                .global_shell_history
                .iter()
                .map(|entry| con_core::session::CommandHistoryEntryState {
                    command: entry.command.clone(),
                    cwd: entry.cwd.clone(),
                })
                .collect(),
            input_history: self.global_input_history.iter().cloned().collect(),
            conversation_id: None, // deprecated — per-tab now
        }
    }

    fn snapshot_global_history(&self) -> GlobalHistoryState {
        GlobalHistoryState {
            global_shell_history: self
                .global_shell_history
                .iter()
                .map(|entry| con_core::session::CommandHistoryEntryState {
                    command: entry.command.clone(),
                    cwd: entry.cwd.clone(),
                })
                .collect(),
            input_history: self.global_input_history.iter().cloned().collect(),
        }
    }

    fn save_session(&self, cx: &App) {
        let session = self.snapshot_session(cx);
        let history = self.snapshot_global_history();
        if let Err(err) = self
            .session_save_tx
            .send(SessionSaveRequest::Save(session, history))
        {
            log::warn!("Failed to queue session save: {}", err);
        }
    }

    fn flush_session_save(&self, cx: &App) {
        let session = self.snapshot_session(cx);
        let history = self.snapshot_global_history();
        let (done_tx, done_rx) = crossbeam_channel::bounded(1);
        if let Err(err) = self
            .session_save_tx
            .send(SessionSaveRequest::Flush(
                session.clone(),
                history.clone(),
                done_tx,
            ))
        {
            log::warn!("Failed to flush session save queue: {}", err);
            if let Err(save_err) = session.save() {
                log::warn!("Failed to save session directly during flush: {}", save_err);
            }
            if let Err(save_err) = history.save() {
                log::warn!(
                    "Failed to save command history directly during flush: {}",
                    save_err
                );
            }
            return;
        }

        if let Err(err) = done_rx.recv_timeout(Duration::from_secs(2)) {
            log::warn!("Timed out waiting for session save flush: {}", err);
            if let Err(save_err) = session.save() {
                log::warn!("Failed to save session directly after flush timeout: {}", save_err);
            }
            if let Err(save_err) = history.save() {
                log::warn!(
                    "Failed to save command history directly after flush timeout: {}",
                    save_err
                );
            }
        }
    }

    fn restore_shell_history(
        tab_state: &con_core::session::TabState,
    ) -> HashMap<usize, VecDeque<CommandSuggestionEntry>> {
        let mut restored = HashMap::new();

        for pane_history in &tab_state.shell_history {
            let Some(pane_id) = pane_history.pane_id else {
                continue;
            };
            let entries = pane_history
                .entries
                .iter()
                .filter(|entry| !entry.command.trim().is_empty())
                .map(|entry| CommandSuggestionEntry {
                    command: entry.command.trim().to_string(),
                    cwd: entry.cwd.clone(),
                })
                .collect::<VecDeque<_>>();
            if !entries.is_empty() {
                restored.insert(pane_id, entries);
            }
        }

        restored
    }

    fn restore_global_shell_history(
        session: &con_core::session::Session,
        tabs: &[Tab],
    ) -> VecDeque<CommandSuggestionEntry> {
        let from_session: VecDeque<_> = session
            .global_shell_history
            .iter()
            .filter_map(|entry| {
                let command = entry.command.trim();
                (!command.is_empty()).then(|| CommandSuggestionEntry {
                    command: command.to_string(),
                    cwd: entry.cwd.clone(),
                })
            })
            .collect();
        if !from_session.is_empty() {
            return from_session;
        }

        let mut aggregated = VecDeque::new();
        for tab in tabs {
            for entries in tab.shell_history.values() {
                for entry in entries {
                    if let Some(existing_idx) =
                        aggregated
                            .iter()
                            .position(|existing: &CommandSuggestionEntry| {
                                existing.command == entry.command
                            })
                    {
                        aggregated.remove(existing_idx);
                    }
                    aggregated.push_back(entry.clone());
                    while aggregated.len() > MAX_GLOBAL_SHELL_HISTORY {
                        aggregated.pop_front();
                    }
                }
            }
        }
        aggregated
    }

    fn merge_shell_histories(
        mut restored: VecDeque<CommandSuggestionEntry>,
        persisted_history: &GlobalHistoryState,
    ) -> VecDeque<CommandSuggestionEntry> {
        for entry in &persisted_history.global_shell_history {
            let command = entry.command.trim();
            if command.is_empty() {
                continue;
            }
            if let Some(existing_idx) = restored
                .iter()
                .position(|existing| existing.command == command)
            {
                restored.remove(existing_idx);
            }
            restored.push_back(CommandSuggestionEntry {
                command: command.to_string(),
                cwd: entry.cwd.clone(),
            });
            while restored.len() > MAX_GLOBAL_SHELL_HISTORY {
                restored.pop_front();
            }
        }
        restored
    }

    fn restore_global_input_history(
        session: &con_core::session::Session,
        persisted_history: &GlobalHistoryState,
        shell_history: &VecDeque<CommandSuggestionEntry>,
    ) -> VecDeque<String> {
        let mut restored = VecDeque::new();
        for entry in session
            .input_history
            .iter()
            .chain(persisted_history.input_history.iter())
        {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(existing_idx) = restored
                .iter()
                .position(|existing: &String| existing == trimmed)
            {
                restored.remove(existing_idx);
            }
            restored.push_back(trimmed.to_string());
            while restored.len() > MAX_GLOBAL_INPUT_HISTORY {
                restored.pop_front();
            }
        }

        if !restored.is_empty() {
            return restored;
        }

        shell_history
            .iter()
            .filter_map(|entry| {
                let trimmed = entry.command.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            })
            .collect()
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

    fn on_set_auto_approve(
        &mut self,
        _panel: &Entity<AgentPanel>,
        event: &SetAutoApprove,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.harness.set_auto_approve(event.enabled);
    }

    fn on_select_session_model(
        &mut self,
        _panel: &Entity<AgentPanel>,
        event: &SelectSessionModel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut config = self.harness.config().clone();
        let provider = config.provider.clone();
        let mut provider_config = config.providers.get_or_default(&provider);
        provider_config.model = Some(event.model.clone());
        config.providers.set(&provider, provider_config);
        self.harness.update_config(config.clone());

        self.agent_panel.update(cx, |panel, cx| {
            panel.set_session_provider_options(
                AgentPanel::configured_session_providers(&config),
                window,
                cx,
            );
            panel.set_model_name(event.model.clone());
            panel.set_session_model_options(self.model_registry.models_for(&provider), window, cx);
        });

        self.settings_panel.update(cx, |settings, _cx| {
            let agent = settings.agent_config_mut();
            agent.provider = config.provider.clone();
            agent.providers = config.providers.clone();
        });

        cx.notify();
    }

    fn on_select_session_provider(
        &mut self,
        _panel: &Entity<AgentPanel>,
        event: &SelectSessionProvider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut config = self.harness.config().clone();
        config.provider = event.provider.clone();
        self.harness.update_config(config.clone());

        let provider = config.provider.clone();
        let model_name = self.harness.active_model_name();
        let available_models = self.model_registry.models_for(&provider);

        self.agent_panel.update(cx, |panel, cx| {
            panel.set_session_provider_options(
                AgentPanel::configured_session_providers(&config),
                window,
                cx,
            );
            panel.set_provider_name(provider.clone(), window, cx);
            panel.set_model_name(model_name);
            panel.set_session_model_options(available_models, window, cx);
        });

        self.settings_panel.update(cx, |settings, _cx| {
            let agent = settings.agent_config_mut();
            agent.provider = config.provider.clone();
            agent.providers = config.providers.clone();
        });

        cx.notify();
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
            "new-window" => {
                cx.dispatch_action(&crate::NewWindow);
            }
            "toggle-agent" => {
                self.toggle_agent_panel(&ToggleAgentPanel, window, cx);
            }
            "settings" => {
                // Route through the full toggle_settings action so
                // `sync_pane_visibility_for_modals` fires. Opening the
                // panel directly (via settings_panel.update.toggle)
                // leaves pane WS_CHILD HWNDs visible on Windows, and
                // they sit above the GPUI-drawn modal and swallow its
                // clicks.
                self.toggle_settings(&settings_panel::ToggleSettings, window, cx);
            }
            "new-tab" => {
                self.new_tab(&NewTab, window, cx);
            }
            "next-tab" => {
                self.next_tab(&NextTab, window, cx);
            }
            "previous-tab" => {
                self.previous_tab(&PreviousTab, window, cx);
            }
            "close-tab" => {
                self.close_tab(&CloseTab, window, cx);
            }
            "split-right" => {
                self.split_pane(
                    SplitDirection::Horizontal,
                    SplitPlacement::After,
                    window,
                    cx,
                );
            }
            "split-down" => {
                self.split_pane(SplitDirection::Vertical, SplitPlacement::After, window, cx);
            }
            "clear-terminal" => {
                if self.has_active_tab() {
                    self.active_terminal().clear_scrollback(cx);
                }
            }
            "focus-terminal" => {
                if self.has_active_tab() {
                    self.active_terminal().focus(window, cx);
                }
            }
            "toggle-input-bar" => {
                self.toggle_input_bar(&crate::ToggleInputBar, window, cx);
            }
            "cycle-input-mode" => {
                self.input_bar.update(cx, |bar, cx| {
                    bar.cycle_mode(window, cx);
                });
                self.sync_active_terminal_focus_states(cx);
            }
            "quit" => {
                self.cancel_all_sessions();
                cx.quit();
            }
            _ => {}
        }
        cx.notify();
    }

    fn on_palette_dismissed(
        &mut self,
        _palette: &Entity<CommandPalette>,
        _event: &PaletteDismissed,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.restore_terminal_focus_after_modal(window, cx);
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
        self.shell_suggestion_engine = self.harness.suggestion_engine(180);
        self.shell_suggestion_engine.clear_cache();

        // Sync auto-approve to agent panel UI
        self.agent_panel.update(cx, |panel, cx| {
            panel.set_auto_approve(auto_approve);
            panel.set_session_provider_options(
                AgentPanel::configured_session_providers(self.harness.config()),
                window,
                cx,
            );
        });

        // Apply updated skills paths (forces rescan on next cwd check)
        let skills_config = settings.read(cx).skills_config().clone();
        self.harness.update_skills_config(skills_config);
        if self.has_active_tab() {
            if let Some(cwd) = self.active_terminal().current_dir(cx) {
                self.harness.scan_skills(&cwd);
            }
        }

        let term_config = settings.read(cx).terminal_config().clone();
        let appearance_config = settings.read(cx).appearance_config().clone();
        self.terminal_font_family = term_config.font_family.clone();
        self.ui_font_family = appearance_config.ui_font_family.clone();
        self.ui_font_size = appearance_config.ui_font_size;
        self.font_size = term_config.font_size;
        self.terminal_cursor_style = term_config.cursor_style.clone();
        self.terminal_opacity =
            Self::effective_terminal_opacity(appearance_config.terminal_opacity);
        self.terminal_blur = Self::effective_terminal_blur(appearance_config.terminal_blur);
        self.ui_opacity = Self::clamp_ui_opacity(appearance_config.ui_opacity);
        let effective_ui_opacity = Self::effective_ui_opacity(self.ui_opacity);
        self.background_image = appearance_config.background_image.clone();
        self.background_image_opacity =
            Self::clamp_background_image_opacity(appearance_config.background_image_opacity);
        self.background_image_position = appearance_config.background_image_position.clone();
        self.background_image_fit = appearance_config.background_image_fit.clone();
        self.background_image_repeat = appearance_config.background_image_repeat;
        self.agent_panel
            .update(cx, |panel, _cx| panel.set_ui_opacity(effective_ui_opacity));
        self.input_bar
            .update(cx, |bar, _cx| bar.set_ui_opacity(effective_ui_opacity));
        self.command_palette.update(cx, |palette, _cx| {
            palette.set_ui_opacity(effective_ui_opacity)
        });

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
        crate::bind_app_keybindings(cx, &kb);
        #[cfg(target_os = "macos")]
        crate::global_hotkey::update_from_keybindings(&kb);

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
                terminal.set_theme(
                    &theme,
                    &colors,
                    &self.terminal_font_family,
                    self.font_size,
                    self.terminal_opacity,
                    self.terminal_blur,
                    &self.terminal_cursor_style,
                    self.background_image.as_deref(),
                    self.background_image_opacity,
                    Some(&self.background_image_position),
                    Some(&self.background_image_fit),
                    self.background_image_repeat,
                    cx,
                );
            }
        }
        if let Err(e) = self.ghostty_app.update_appearance(
            &colors,
            &self.terminal_font_family,
            self.font_size,
            self.terminal_opacity,
            self.terminal_blur,
            &self.terminal_cursor_style,
            self.background_image.as_deref(),
            self.background_image_opacity,
            Some(&self.background_image_position),
            Some(&self.background_image_fit),
            self.background_image_repeat,
        ) {
            log::error!("Failed to update Ghostty appearance: {}", e);
        }
        for tab in &self.tabs {
            for terminal in tab.pane_tree.all_terminals() {
                terminal.sync_window_background_blur(cx);
            }
        }
        // Sync GPUI UI theme colors with terminal theme
        crate::theme::sync_gpui_theme(
            &theme,
            &self.terminal_font_family,
            &self.ui_font_family,
            self.ui_font_size,
            window,
            cx,
        );
    }

    fn on_input_escape(
        &mut self,
        _input_bar: &Entity<InputBar>,
        _event: &EscapeInput,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pane_scope_picker_open {
            self.pane_scope_picker_open = false;
            cx.notify();
        }
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

    fn on_toggle_pane_scope_picker_requested(
        &mut self,
        _input_bar: &Entity<InputBar>,
        _event: &TogglePaneScopePickerRequested,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_pane_scope_picker(&TogglePaneScopePicker, window, cx);
    }

    fn on_input_edited(
        &mut self,
        _input_bar: &Entity<InputBar>,
        _event: &InputEdited,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_input_suggestion(window, cx);
        cx.notify();
    }

    fn on_input_scope_changed(
        &mut self,
        _input_bar: &Entity<InputBar>,
        _event: &InputScopeChanged,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_active_terminal_focus_states(cx);
        cx.notify();
    }

    fn on_input_submit(
        &mut self,
        input_bar: &Entity<InputBar>,
        _event: &SubmitInput,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pane_scope_picker_open = false;
        let (content, mode) = input_bar.update(cx, |bar, cx| {
            let content = bar.take_content(window, cx);
            bar.clear_completion_ui();
            (content, bar.mode())
        });

        if content.trim().is_empty() {
            return;
        }

        self.record_input_history(&content);
        let recent_inputs = self.recent_input_history(80);
        input_bar.update(cx, |bar, _cx| bar.set_recent_commands(recent_inputs));

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
                let step_text = crate::agent_panel::describe_agent_step(&step);
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
        let resolved = match self.resolve_pane_target_for_tab(tab_idx, req.target) {
            Ok(target) => target,
            Err(err) => {
                let _ = req.response_tx.send(TerminalExecResponse {
                    output: err,
                    exit_code: Some(1),
                });
                return;
            }
        };
        let pane = resolved.pane;
        let target_pane_index = resolved.pane_index;

        // Safety: refuse to execute on a dead PTY.
        if !pane.is_alive(cx) {
            let _ = req.response_tx.send(TerminalExecResponse {
                output: "Pane PTY process has exited — cannot execute command.".to_string(),
                exit_code: Some(1),
            });
            return;
        }

        let (observation, runtime) = self.observe_terminal_runtime_for_tab(tab_idx, &pane, 20, cx);
        let control = con_agent::control::PaneControlState::from_runtime(&runtime);
        let managed_remote_workspace =
            con_agent::context::remote_workspace_anchor(&runtime, &observation);
        if !control.allows_visible_shell_exec() && managed_remote_workspace.is_none() {
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
                    {
                        format!(
                            "\n\nSUGGESTED APPROACH: This pane exposes native tmux control. Prefer tmux-native tools over outer-pane send_keys.\n\
                             1. tmux_list_targets(pane_index={idx}) to discover tmux windows/panes\n\
                             2. tmux_capture_pane(pane_index={idx}, target=\"%<pane>\") to inspect the exact tmux pane\n\
                             3. tmux_run_command(pane_index={idx}, location=\"new_window\", command=\"bash\", window_name=\"scratch\") to create a fresh shell target when needed\n\
                             4. tmux_send_keys(pane_index={idx}, target=\"%<pane>\", literal_text=\"your_command\", append_enter=true) to act on an existing tmux pane",
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
                    format!(
                        "\n\nSUGGESTED APPROACH: Use read_pane(pane_index={idx}) to inspect the current screen, then \
                         send_keys(pane_index={idx}, ...) for keystroke-level interaction. \
                         Use \\x1b (Escape) or \\x03 (Ctrl-C) to exit to a shell if needed. \
                         Always verify with read_pane after each send_keys.",
                        idx = target_pane_index
                    )
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
        if let Some(pane_id) = self.tabs[tab_idx].pane_tree.pane_id_for_terminal(&pane) {
            self.record_shell_command(tab_idx, pane_id, &req.command, pane.current_dir(cx));
        }
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
            enum VisibleExecPoll {
                Finished {
                    output: String,
                    exit_code: Option<i32>,
                },
                Observe {
                    output: String,
                    prompt_like: bool,
                },
            }

            const PROMPT_STABLE_POLLS: u32 = 2;
            let mut last_prompt_snapshot = String::new();
            let mut stable_prompt_polls = 0u32;

            cx.background_executor()
                .timer(std::time::Duration::from_millis(500))
                .await;

            for _ in 0..29 {
                let poll = _this
                    .update(cx, |_ws, cx| {
                        if let Some((exit_code, _duration)) =
                            pane_for_fallback.take_command_finished(cx)
                        {
                            return VisibleExecPoll::Finished {
                                output: pane_for_fallback.recent_lines(50, cx).join("\n"),
                                exit_code,
                            };
                        }

                        let observation = pane_for_fallback.observation_frame(50, cx);
                        let output = observation
                            .recent_output
                            .iter()
                            .map(|line| line.trim_end())
                            .collect::<Vec<_>>()
                            .join("\n");
                        let prompt_like = observation.screen_hints.iter().any(|hint| {
                            matches!(
                                hint.kind,
                                con_agent::context::PaneObservationHintKind::PromptLikeInput
                            )
                        });

                        VisibleExecPoll::Observe {
                            output,
                            prompt_like,
                        }
                    })
                    .ok();

                match poll {
                    Some(VisibleExecPoll::Finished { output, exit_code }) => {
                        let _ = fallback_response_tx
                            .try_send(TerminalExecResponse { output, exit_code });
                        return;
                    }
                    Some(VisibleExecPoll::Observe {
                        output,
                        prompt_like,
                    }) if prompt_like => {
                        if !output.is_empty() && output == last_prompt_snapshot {
                            stable_prompt_polls += 1;
                        } else {
                            last_prompt_snapshot = output;
                            stable_prompt_polls = 0;
                        }

                        if stable_prompt_polls >= PROMPT_STABLE_POLLS {
                            let output = _this
                                .update(cx, |_ws, cx| {
                                    pane_for_fallback.recover_shell_prompt_state(cx);
                                    pane_for_fallback.recent_lines(50, cx).join("\n")
                                })
                                .unwrap_or_else(|_| last_prompt_snapshot.clone());
                            let _ = fallback_response_tx.try_send(TerminalExecResponse {
                                output,
                                exit_code: None,
                            });
                            return;
                        }
                    }
                    Some(VisibleExecPoll::Observe { output, .. }) => {
                        last_prompt_snapshot = output;
                        stable_prompt_polls = 0;
                    }
                    None => return,
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
                        let (observation, runtime) = self.observe_terminal_runtime_for_tab(
                            tab_idx,
                            terminal,
                            Self::SECONDARY_PANE_OBSERVATION_LINES,
                            cx,
                        );
                        let control = con_agent::control::PaneControlState::from_runtime(&runtime);
                        let remote_workspace =
                            con_agent::context::remote_workspace_anchor(&runtime, &observation);
                        let title = observation
                            .title
                            .clone()
                            .unwrap_or_else(|| format!("Pane {}", idx + 1));
                        let (cols, rows) = terminal.grid_size(cx);
                        PaneInfo {
                            index: idx + 1,
                            pane_id: pid,
                            title,
                            cwd: observation.cwd.clone(),
                            is_focused: pid == focused_pid,
                            rows,
                            cols,
                            surface_ready: terminal.surface_ready(cx),
                            is_alive: terminal.is_alive(cx),
                            hostname: runtime.remote_host.clone(),
                            hostname_confidence: runtime.remote_host_confidence,
                            hostname_source: runtime.remote_host_source,
                            remote_workspace,
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
            PaneQuery::ReadContent { target, lines } => {
                match self.resolve_pane_target_for_tab(tab_idx, target) {
                    Ok(resolved) => {
                        let content = resolved.pane.recent_lines(lines, cx).join("\n");
                        PaneResponse::Content(content)
                    }
                    Err(err) => PaneResponse::Error(err),
                }
            }
            PaneQuery::SendKeys { target, keys } => {
                match self.resolve_pane_target_for_tab(tab_idx, target) {
                    Ok(resolved) => {
                        resolved.pane.write(keys.as_bytes(), cx);
                        self.record_runtime_event_for_terminal(
                            tab_idx,
                            &resolved.pane,
                            con_agent::context::PaneRuntimeEvent::RawInput {
                                keys: keys.clone(),
                                input_generation: resolved.pane.input_generation(cx),
                            },
                        );
                        PaneResponse::KeysSent
                    }
                    Err(err) => PaneResponse::Error(err),
                }
            }
            PaneQuery::SearchText {
                target,
                pattern,
                max_matches,
            } => {
                let targets: Vec<(usize, TerminalPane)> =
                    if target.pane_index.is_none() && target.pane_id.is_none() {
                        all_terminals
                            .iter()
                            .enumerate()
                            .map(|(i, t)| (i + 1, (*t).clone()))
                            .collect()
                    } else {
                        match self.resolve_pane_target_for_tab(tab_idx, target) {
                            Ok(resolved) => vec![(resolved.pane_index, resolved.pane)],
                            Err(err) => {
                                return {
                                    let _ = req.response_tx.send(PaneResponse::Error(err));
                                };
                            }
                        }
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
            PaneQuery::InspectTmux { target } => {
                match self.resolve_pane_target_for_tab(tab_idx, target) {
                    Ok(resolved) => {
                        let (_, runtime) =
                            self.observe_terminal_runtime_for_tab(tab_idx, &resolved.pane, 20, cx);
                        let control = con_agent::control::PaneControlState::from_runtime(&runtime);
                        if let Some(tmux) = control.tmux {
                            PaneResponse::TmuxInfo(tmux)
                        } else {
                            PaneResponse::Error(format!(
                                "Pane {} (id {}) is not currently in a tmux scope.",
                                resolved.pane_index, resolved.pane_id
                            ))
                        }
                    }
                    Err(err) => PaneResponse::Error(err),
                }
            }
            PaneQuery::TmuxList { pane: target } => match self
                .resolve_pane_target_for_tab(tab_idx, target)
            {
                Ok(resolved) => {
                    let (_, runtime) =
                        self.observe_terminal_runtime_for_tab(tab_idx, &resolved.pane, 20, cx);
                    let control = con_agent::control::PaneControlState::from_runtime(&runtime);
                    let tmux_mode = control.tmux.as_ref().map(|tmux| tmux.mode);
                    if !control
                        .capabilities
                        .contains(&con_agent::PaneControlCapability::QueryTmux)
                    {
                        PaneResponse::Error(format!(
                            "Pane {} (id {}) does not currently expose tmux native query capability.\nvisible_target: {}\ntmux_mode: {}\ncontrol_attachments: {}\ncontrol_capabilities: {}",
                            resolved.pane_index,
                            resolved.pane_id,
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
                    } else if Self::pane_blocks_shell_anchor_control(&resolved.pane, cx) {
                        PaneResponse::Error(format!(
                            "Pane {} (id {}) is busy. Wait for the current command to finish before running tmux-native queries from its shell anchor.",
                            resolved.pane_index, resolved.pane_id
                        ))
                    } else {
                        let response_tx = req.response_tx;
                        let nonce = format!(
                            "tmux-list-{}-{}",
                            resolved.pane_id,
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|duration| duration.as_nanos())
                                .unwrap_or_default()
                        );
                        let command = con_agent::tmux::build_tmux_list_command(&nonce);
                        self.spawn_shell_anchor_command(
                            tab_idx,
                            resolved.pane,
                            resolved.pane_index,
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
                Err(err) => PaneResponse::Error(err),
            },
            PaneQuery::TmuxCapture {
                pane: pane_target,
                target,
                lines,
            } => match self.resolve_pane_target_for_tab(tab_idx, pane_target) {
                Ok(resolved) => {
                    let (_, runtime) =
                        self.observe_terminal_runtime_for_tab(tab_idx, &resolved.pane, 20, cx);
                    let control = con_agent::control::PaneControlState::from_runtime(&runtime);
                    if !control
                        .capabilities
                        .contains(&con_agent::PaneControlCapability::QueryTmux)
                    {
                        PaneResponse::Error(format!(
                            "Pane {} (id {}) does not currently expose tmux native query capability for capture.\nvisible_target: {}\ncontrol_capabilities: {}",
                            resolved.pane_index,
                            resolved.pane_id,
                            control.visible_target.summary(),
                            control
                                .capabilities
                                .iter()
                                .map(|capability| capability.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                        ))
                    } else if Self::pane_blocks_shell_anchor_control(&resolved.pane, cx) {
                        PaneResponse::Error(format!(
                            "Pane {} (id {}) is busy. Wait for the current command to finish before running tmux capture from its shell anchor.",
                            resolved.pane_index, resolved.pane_id
                        ))
                    } else {
                        let response_tx = req.response_tx;
                        let nonce = format!(
                            "tmux-capture-{}-{}",
                            resolved.pane_id,
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
                            resolved.pane,
                            resolved.pane_index,
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
                Err(err) => PaneResponse::Error(err),
            },
            PaneQuery::TmuxSendKeys {
                pane: pane_target,
                target,
                literal_text,
                key_names,
                append_enter,
            } => match self.resolve_pane_target_for_tab(tab_idx, pane_target) {
                Ok(resolved) => {
                    let (_, runtime) =
                        self.observe_terminal_runtime_for_tab(tab_idx, &resolved.pane, 20, cx);
                    let control = con_agent::control::PaneControlState::from_runtime(&runtime);
                    if !control
                        .capabilities
                        .contains(&con_agent::PaneControlCapability::SendTmuxKeys)
                    {
                        PaneResponse::Error(format!(
                            "Pane {} (id {}) does not currently expose tmux native send-keys capability.\nvisible_target: {}\ncontrol_capabilities: {}",
                            resolved.pane_index,
                            resolved.pane_id,
                            control.visible_target.summary(),
                            control
                                .capabilities
                                .iter()
                                .map(|capability| capability.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                        ))
                    } else if Self::pane_blocks_shell_anchor_control(&resolved.pane, cx) {
                        PaneResponse::Error(format!(
                            "Pane {} (id {}) is busy. Wait for the current command to finish before sending tmux-native keys from its shell anchor.",
                            resolved.pane_index, resolved.pane_id
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
                                    resolved.pane,
                                    resolved.pane_index,
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
                Err(err) => PaneResponse::Error(err),
            },
            PaneQuery::TmuxRunCommand {
                pane: pane_target,
                target,
                location,
                command,
                window_name,
                cwd,
                detached,
            } => match self.resolve_pane_target_for_tab(tab_idx, pane_target) {
                Ok(resolved) => {
                    let (_, runtime) =
                        self.observe_terminal_runtime_for_tab(tab_idx, &resolved.pane, 20, cx);
                    let control = con_agent::control::PaneControlState::from_runtime(&runtime);
                    if !control
                        .capabilities
                        .contains(&con_agent::PaneControlCapability::ExecTmuxCommand)
                    {
                        PaneResponse::Error(format!(
                            "Pane {} (id {}) does not currently expose tmux native run-command capability.\nvisible_target: {}\ncontrol_capabilities: {}",
                            resolved.pane_index,
                            resolved.pane_id,
                            control.visible_target.summary(),
                            control
                                .capabilities
                                .iter()
                                .map(|capability| capability.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                        ))
                    } else if Self::pane_blocks_shell_anchor_control(&resolved.pane, cx) {
                        PaneResponse::Error(format!(
                            "Pane {} (id {}) is busy. Wait for the current command to finish before launching tmux-native commands from its shell anchor.",
                            resolved.pane_index, resolved.pane_id
                        ))
                    } else {
                        let response_tx = req.response_tx;
                        let nonce = format!(
                            "tmux-exec-{}-{}",
                            resolved.pane_id,
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|duration| duration.as_nanos())
                                .unwrap_or_default()
                        );
                        let shell_command = con_agent::tmux::build_tmux_exec_command(
                            &nonce,
                            location,
                            target.as_deref(),
                            &command,
                            window_name.as_deref(),
                            cwd.as_deref(),
                            detached,
                        );
                        self.spawn_shell_anchor_command(
                            tab_idx,
                            resolved.pane,
                            resolved.pane_index,
                            shell_command,
                            12,
                            move |lines| {
                                con_agent::tmux::parse_tmux_exec_lines(
                                    &lines, &nonce, location, detached,
                                )
                                .map(con_agent::PaneResponse::TmuxExec)
                            },
                            response_tx,
                            cx,
                        );
                        return;
                    }
                }
                Err(err) => PaneResponse::Error(err),
            },
            PaneQuery::ProbeShellContext { target } => match self
                .resolve_pane_target_for_tab(tab_idx, target)
            {
                Ok(resolved) => {
                    let pane_index = resolved.pane_index;
                    let pane_id = resolved.pane_id;
                    let pane = resolved.pane;
                    let (_, runtime) =
                        self.observe_terminal_runtime_for_tab(tab_idx, &pane, 20, cx);
                    let control = con_agent::control::PaneControlState::from_runtime(&runtime);
                    if !control.allows_shell_probe() {
                        PaneResponse::Error(format!(
                            "Pane {} (id {}) does not currently expose the probe_shell_context capability. \
                             It must be a proven fresh shell prompt before shell-scoped probing is allowed.\n\
                             visible_target: {}\ncontrol_attachments: {}\ncontrol_capabilities: {}",
                            pane_index,
                            pane_id,
                            control.visible_target.summary(),
                            con_agent::control::format_control_attachments(&control.attachments),
                            control
                                .capabilities
                                .iter()
                                .map(|capability| capability.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                        ))
                    } else if Self::pane_blocks_shell_anchor_control(&pane, cx) {
                        PaneResponse::Error(format!(
                            "Pane {} (id {}) is busy. Wait for the current command to finish before running a shell probe.",
                            pane_index, pane_id
                        ))
                    } else {
                        let response_tx = req.response_tx;
                        let nonce = format!(
                            "{}-{}",
                            pane_id,
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
                                        "Shell probe timed out in pane {} (id {}) after 10s.",
                                        pane_index, pane_id
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
                                            "Shell probe finished in pane {} (id {}) but the probe output could not be parsed: {}\nRecent output:\n{}",
                                            pane_index, pane_id, err, excerpt
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
                Err(err) => PaneResponse::Error(err),
            },
            PaneQuery::CheckBusy { target } => {
                match self.resolve_pane_target_for_tab(tab_idx, target) {
                    Ok(resolved) => PaneResponse::BusyStatus {
                        surface_ready: resolved.pane.surface_ready(cx),
                        is_alive: resolved.pane.is_alive(cx),
                        is_busy: resolved.pane.is_busy(cx),
                        has_shell_integration: resolved.pane.has_shell_integration(cx),
                    },
                    Err(err) => PaneResponse::Error(err),
                }
            }
            PaneQuery::WaitFor {
                target,
                timeout_secs,
                pattern,
            } => {
                // Normalize empty pattern to None — "".contains("") is always true in Rust,
                // making empty pattern match instantly (useless). Treat as idle/quiescence.
                let pattern = pattern.filter(|p| !p.is_empty());

                log::info!(
                    "[wait_for] target={} timeout={:?} pattern={:?}",
                    target.describe(),
                    timeout_secs,
                    pattern,
                );

                match self.resolve_pane_target_for_tab(tab_idx, target) {
                    Ok(resolved) => {
                        let pane = resolved.pane;
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
                    Err(err) => PaneResponse::Error(err),
                }
            }
            PaneQuery::CreatePane { command, location } => {
                // Creating a terminal requires a Window, so defer into an explicit
                // window-aware callback instead of depending on a later render.
                self.pending_create_pane_requests.push(PendingCreatePane {
                    command,
                    tab_idx,
                    location,
                    response_tx: req.response_tx,
                });
                self.schedule_pending_create_pane_flush(cx);
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
        self.agent_panel_motion.set_target(
            if self.agent_panel_open { 1.0 } else { 0.0 },
            std::time::Duration::from_millis(if self.agent_panel_open { 290 } else { 220 }),
        );
        if self.agent_panel_open {
            if self.input_bar_visible {
                self.input_bar.focus_handle(cx).focus(window, cx);
            } else {
                let focused_inline = self
                    .agent_panel
                    .update(cx, |panel, cx| panel.focus_inline_input(window, cx));
                if !focused_inline {
                    self.focus_agent_inline_input_next_frame(window, cx);
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
        if !self.input_bar_visible {
            self.pane_scope_picker_open = false;
        }
        self.input_bar_motion.set_target(
            if self.input_bar_visible { 1.0 } else { 0.0 },
            std::time::Duration::from_millis(if self.input_bar_visible { 210 } else { 160 }),
        );
        if self.input_bar_visible {
            self.input_bar.focus_handle(cx).focus(window, cx);
        } else if self.agent_panel_open {
            let focused_inline = self
                .agent_panel
                .update(cx, |panel, cx| panel.focus_inline_input(window, cx));
            if !focused_inline {
                self.focus_agent_inline_input_next_frame(window, cx);
            }
        } else {
            self.active_terminal().focus(window, cx);
        }
        self.save_session(cx);
        cx.notify();
    }

    fn toggle_pane_scope_picker(
        &mut self,
        _: &TogglePaneScopePicker,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.input_bar.read(cx).mode() == InputMode::Agent
            || self.input_bar.read(cx).pane_infos().len() <= 1
        {
            return;
        }

        if !self.input_bar_visible {
            self.input_bar_visible = true;
            self.input_bar_motion
                .set_target(1.0, std::time::Duration::from_millis(180));
        }

        self.pane_scope_picker_open = !self.pane_scope_picker_open;
        if self.pane_scope_picker_open {
            self.input_bar.focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn close_pane_scope_picker(&mut self, cx: &mut Context<Self>) {
        if self.pane_scope_picker_open {
            self.pane_scope_picker_open = false;
            cx.notify();
        }
    }

    fn set_scope_broadcast(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input_bar.update(cx, |bar, cx| {
            bar.set_broadcast_scope(window, cx);
        });
        self.sync_active_terminal_focus_states(cx);
        self.input_bar.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn set_scope_focused(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input_bar.update(cx, |bar, cx| {
            bar.set_focused_scope(cx);
        });
        self.sync_active_terminal_focus_states(cx);
        self.input_bar.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn toggle_scope_pane_by_id(
        &mut self,
        pane_id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.input_bar.update(cx, |bar, cx| {
            bar.toggle_scope_pane(pane_id, window, cx);
        });
        self.sync_active_terminal_focus_states(cx);
        self.input_bar.focus_handle(cx).focus(window, cx);
    }

    fn toggle_scope_pane_by_index(
        &mut self,
        pane_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let panes = self.input_bar.read(cx).pane_infos();
        if let Some(pane) = panes.get(pane_index) {
            self.toggle_scope_pane_by_id(pane.id, window, cx);
        }
    }

    fn active_pane_layout(&self, cx: &App) -> PaneLayoutState {
        self.tabs[self.active_tab].pane_tree.to_state(cx)
    }

    fn release_active_terminal_mouse_selection(&self, cx: &App) {
        if self.active_tab >= self.tabs.len() {
            return;
        }
        for terminal in self.tabs[self.active_tab].pane_tree.all_terminals() {
            terminal.release_mouse_selection(cx);
        }
    }

    fn render_scope_leaf(
        &self,
        pane_id: usize,
        pane: &PaneInfo,
        display_indices: &HashMap<usize, usize>,
        selected_ids: &HashSet<usize>,
        focused_id: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let is_selected = selected_ids.contains(&pane_id);
        let is_focused = pane_id == focused_id;
        let display_index = display_indices.get(&pane_id).copied().unwrap_or(0);
        let status_color = if !pane.is_alive {
            theme.danger
        } else if pane.is_busy {
            theme.warning
        } else {
            theme.success
        };
        let label = if let Some(host) = &pane.hostname {
            host.clone()
        } else if pane.name.is_empty() {
            format!("Pane {}", pane.id)
        } else {
            pane.name.clone()
        };
        let status_text = if !pane.is_alive {
            Some("offline")
        } else if pane.is_busy {
            Some("busy")
        } else if pane.hostname.is_some() {
            Some("remote")
        } else {
            None
        };
        let base_tile_surface = if theme.is_dark() {
            theme.title_bar.opacity(if is_focused { 0.84 } else { 0.72 })
        } else {
            theme.background.opacity(if is_focused { 0.96 } else { 0.90 })
        };
        let hover_tile_surface = if theme.is_dark() {
            theme.title_bar.opacity(0.90)
        } else {
            theme.background.opacity(0.98)
        };

        div()
            .id(SharedString::from(format!("scope-pane-{pane_id}")))
            .h_full()
            .w_full()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .overflow_hidden()
            .px(px(11.0))
            .py(px(10.0))
            .rounded(px(10.0))
            .cursor_pointer()
            .bg(if is_selected {
                theme.primary.opacity(0.12)
            } else {
                base_tile_surface
            })
            .hover(|s| {
                s.bg(if is_selected {
                    theme.primary.opacity(0.16)
                } else {
                    hover_tile_surface
                })
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    this.toggle_scope_pane_by_id(pane_id, window, cx);
                }),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .justify_between()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.0))
                            .min_w_0()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .child(div().size(px(6.0)).rounded_full().bg(status_color))
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .line_height(px(14.0))
                                            .font_family(theme.mono_font_family.clone())
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(if is_selected {
                                                theme.primary
                                            } else {
                                                theme.foreground
                                            })
                                            .min_w_0()
                                            .overflow_hidden()
                                            .overflow_x_hidden()
                                            .whitespace_nowrap()
                                            .text_ellipsis()
                                            .child(label),
                                    ),
                            )
                            .children(status_text.map(|text| {
                                div()
                                    .text_size(px(10.5))
                                    .line_height(px(13.0))
                                    .font_family(theme.mono_font_family.clone())
                                    .text_color(theme.muted_foreground.opacity(0.62))
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(text)
                            })),
                    )
                    .child(
                        div().flex().items_center().gap(px(4.0)).child(
                            div()
                                .size(px(20.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(6.0))
                                .bg(if is_selected {
                                    theme.primary.opacity(0.13)
                                } else {
                                    theme.title_bar_border.opacity(0.18)
                                })
                                .text_size(px(10.0))
                                .font_family(theme.mono_font_family.clone())
                                .text_color(if is_selected {
                                    theme.primary
                                } else {
                                    theme.muted_foreground.opacity(0.58)
                                })
                                .child(format!("{}", display_index + 1)),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render_scope_node(
        &self,
        layout: &PaneLayoutState,
        panes: &HashMap<usize, PaneInfo>,
        display_indices: &HashMap<usize, usize>,
        selected_ids: &HashSet<usize>,
        focused_id: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match layout {
            PaneLayoutState::Leaf { pane_id, .. } => panes
                .get(pane_id)
                .map(|pane| {
                    self.render_scope_leaf(
                        *pane_id,
                        pane,
                        display_indices,
                        selected_ids,
                        focused_id,
                        cx,
                    )
                })
                .unwrap_or_else(|| {
                    div()
                        .h_full()
                        .w_full()
                        .rounded(px(9.0))
                        .bg(cx.theme().muted.opacity(0.06))
                        .into_any_element()
                }),
            PaneLayoutState::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let theme = cx.theme();
                let make_pane = |child: AnyElement, basis: f32| {
                    div()
                        .flex_grow()
                        .flex_shrink()
                        .flex_basis(relative(basis.clamp(0.15, 0.85)))
                        .overflow_hidden()
                        .child(child)
                };
                let divider = div()
                    .bg(theme.title_bar_border.opacity(0.75))
                    .map(|divider| match direction {
                        PaneSplitDirection::Horizontal => divider.w(px(1.0)).h_full(),
                        PaneSplitDirection::Vertical => divider.h(px(1.0)).w_full(),
                    });
                let mut container = div().flex().size_full().gap(px(6.0));
                container = match direction {
                    PaneSplitDirection::Horizontal => container.flex_row(),
                    PaneSplitDirection::Vertical => container.flex_col(),
                };
                container
                    .child(make_pane(
                        self.render_scope_node(
                            first,
                            panes,
                            display_indices,
                            selected_ids,
                            focused_id,
                            cx,
                        ),
                        *ratio,
                    ))
                    .child(divider)
                    .child(make_pane(
                        self.render_scope_node(
                            second,
                            panes,
                            display_indices,
                            selected_ids,
                            focused_id,
                            cx,
                        ),
                        1.0 - *ratio,
                    ))
                    .into_any_element()
            }
        }
    }

    fn quit(&mut self, _: &Quit, _window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_all_sessions();
        self.flush_session_save(cx);
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

    fn cycle_input_mode(
        &mut self,
        _: &CycleInputMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.input_bar.update(cx, |bar, cx| {
            bar.cycle_mode(window, cx);
        });
        if self.input_bar.read(cx).mode() == InputMode::Agent {
            self.pane_scope_picker_open = false;
        }
        self.sync_active_terminal_focus_states(cx);
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
        self.sync_pane_visibility_for_modals(cx);
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
        self.sync_pane_visibility_for_modals(cx);
        cx.notify();
    }

    fn is_modal_open(&self, cx: &App) -> bool {
        self.settings_panel.read(cx).is_visible() || self.command_palette.read(cx).is_visible()
    }

    /// Hide every pane's native surface while a modal is visible, and
    /// restore it once all modals have closed. On Windows this matters
    /// because each pane is a WS_CHILD HWND that always paints on top
    /// of the GPUI-drawn modal and also steals mouse clicks before the
    /// modal can see them. On macOS `set_visible` on NSView is
    /// cheap / idempotent, so this is a no-op in practice.
    fn sync_pane_visibility_for_modals(&self, cx: &App) {
        let modal_open = self.is_modal_open(cx);
        let want_visible = !modal_open;
        for tab in &self.tabs {
            for t in tab.pane_tree.all_terminals() {
                t.set_native_view_visible(want_visible, cx);
            }
        }
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
            runtime_trackers: RefCell::new(HashMap::new()),
            runtime_cache: RefCell::new(HashMap::new()),
            shell_history: HashMap::new(),
        });
        self.sync_tab_strip_motion();
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

        for t in self.tabs[old_active].pane_tree.all_terminals() {
            t.set_focus_state(false, cx);
        }
        for terminal in self.tabs[self.active_tab].pane_tree.all_terminals() {
            terminal.set_native_view_visible(true, cx);
            terminal.ensure_surface(window, cx);
        }
        let old_terminals: Vec<TerminalPane> = self.tabs[old_active]
            .pane_tree
            .all_terminals()
            .into_iter()
            .cloned()
            .collect();
        cx.on_next_frame(window, move |_workspace, _window, cx| {
            for terminal in &old_terminals {
                terminal.set_native_view_visible(false, cx);
            }
        });
        terminal.focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        Self::schedule_terminal_bootstrap_reassert(
            &terminal,
            true,
            self.window_handle,
            self.workspace_handle.clone(),
            cx,
        );
        self.save_session(cx);
        cx.notify();
    }

    fn focus_agent_inline_input_next_frame(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.on_next_frame(window, |workspace, window, cx| {
            if !workspace.agent_panel_open || workspace.input_bar_visible {
                return;
            }
            let focused = workspace
                .agent_panel
                .update(cx, |panel, cx| panel.focus_inline_input(window, cx));
            if !focused {
                workspace.focus_terminal(window, cx);
            }
        });
    }

    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        // If the active tab has multiple panes, close the focused pane first.
        // Only close the entire tab when it's down to a single pane.
        if self.tabs[self.active_tab].pane_tree.pane_count() > 1 {
            let (closing, surviving_terminals, new_focus) = {
                let tab = &mut self.tabs[self.active_tab];
                let closing = tab.pane_tree.focused_terminal().clone();
                tab.pane_tree.close_focused();
                let surviving_terminals: Vec<TerminalPane> =
                    tab.pane_tree.all_terminals().into_iter().cloned().collect();
                let new_focus = tab.pane_tree.focused_terminal().clone();
                (closing, surviving_terminals, new_focus)
            };

            for terminal in &surviving_terminals {
                terminal.set_native_view_visible(true, cx);
                terminal.ensure_surface(window, cx);
                terminal.notify(cx);
            }

            new_focus.focus(window, cx);
            self.sync_active_terminal_focus_states(cx);
            cx.on_next_frame(window, move |_workspace, _window, cx| {
                closing.shutdown_surface(cx);
                for terminal in &surviving_terminals {
                    terminal.notify(cx);
                }
            });
            self.save_session(cx);
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
            self.close_window_from_last_tab(window, cx);
            return;
        }
        let closing_terminals: Vec<TerminalPane> = self.tabs[index]
            .pane_tree
            .all_terminals()
            .into_iter()
            .cloned()
            .collect();
        // Save the closing tab's conversation
        {
            let conv = self.tabs[index].session.conversation();
            let _ = conv.lock().save();
        }
        let was_active = index == self.active_tab;
        self.reindex_pending_control_agent_requests_after_tab_close(index);
        self.tabs.remove(index);
        self.sync_tab_strip_motion();
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
            t.ensure_surface(window, cx);
        }
        cx.on_next_frame(window, move |_workspace, _window, cx| {
            for terminal in &closing_terminals {
                terminal.shutdown_surface(cx);
            }
        });
        let focused = self.tabs[self.active_tab].pane_tree.focused_terminal();
        focused.focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        Self::schedule_terminal_bootstrap_reassert(
            focused,
            true,
            self.window_handle,
            self.workspace_handle.clone(),
            cx,
        );
        self.sync_sidebar(cx);
        self.save_session(cx);
        cx.notify();
    }

    fn close_window_from_last_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.defer_in(window, |workspace, window, cx| {
            workspace.prepare_window_close(cx);
            window.remove_window();
        });
    }

    fn prepare_window_close(&mut self, cx: &mut Context<Self>) {
        if self.window_close_prepared {
            return;
        }
        self.window_close_prepared = true;

        self.cancel_all_sessions();
        self.flush_session_save(cx);

        for request in std::mem::take(&mut self.pending_window_control_requests) {
            match request {
                PendingWindowControlRequest::TabsNew { response_tx } => {
                    Self::send_control_result(
                        response_tx,
                        Err(ControlError::internal(
                            "window closed while tabs.new was pending".to_string(),
                        )),
                    );
                }
                PendingWindowControlRequest::TabsClose { response_tx, .. } => {
                    Self::send_control_result(
                        response_tx,
                        Err(ControlError::internal(
                            "window closed while tabs.close was pending".to_string(),
                        )),
                    );
                }
            }
        }

        for (tab_idx, pending) in std::mem::take(&mut self.pending_control_agent_requests) {
            Self::send_control_result(
                pending.response_tx,
                Err(ControlError::internal(format!(
                    "window closed while agent.ask was still pending for tab {}",
                    tab_idx + 1
                ))),
            );
        }

        for tab in &self.tabs {
            let conv = tab.session.conversation();
            let _ = conv.lock().save();

            for terminal in tab.pane_tree.all_terminals() {
                terminal.shutdown_surface(cx);
            }
        }
    }

    fn reindex_pending_control_agent_requests_after_tab_close(&mut self, closed_tab_idx: usize) {
        let mut shifted = HashMap::new();
        for (tab_idx, pending) in std::mem::take(&mut self.pending_control_agent_requests) {
            if tab_idx == closed_tab_idx {
                Self::send_control_result(
                    pending.response_tx,
                    Err(ControlError::internal(format!(
                        "Tab {} was closed while agent.ask was still pending",
                        closed_tab_idx + 1
                    ))),
                );
                continue;
            }

            let next_idx = if tab_idx > closed_tab_idx {
                tab_idx - 1
            } else {
                tab_idx
            };
            shifted.insert(next_idx, pending);
        }
        self.pending_control_agent_requests = shifted;
    }

    fn record_shell_command(
        &mut self,
        tab_idx: usize,
        pane_id: usize,
        command: &str,
        cwd: Option<String>,
    ) {
        let trimmed = command.trim();
        if trimmed.is_empty() || tab_idx >= self.tabs.len() {
            return;
        }

        let history = self.tabs[tab_idx].shell_history.entry(pane_id).or_default();
        if let Some(existing_idx) = history.iter().position(|entry| entry.command == trimmed) {
            history.remove(existing_idx);
        }
        history.push_back(CommandSuggestionEntry {
            command: trimmed.to_string(),
            cwd: cwd.clone(),
        });
        while history.len() > MAX_SHELL_HISTORY_PER_PANE {
            history.pop_front();
        }

        if let Some(existing_idx) = self
            .global_shell_history
            .iter()
            .position(|entry| entry.command == trimmed)
        {
            self.global_shell_history.remove(existing_idx);
        }
        self.global_shell_history.push_back(CommandSuggestionEntry {
            command: trimmed.to_string(),
            cwd,
        });
        while self.global_shell_history.len() > MAX_GLOBAL_SHELL_HISTORY {
            self.global_shell_history.pop_front();
        }
    }

    fn record_input_history(&mut self, input: &str) {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return;
        }

        if let Some(existing_idx) = self
            .global_input_history
            .iter()
            .position(|entry| entry == trimmed)
        {
            self.global_input_history.remove(existing_idx);
        }
        self.global_input_history.push_back(trimmed.to_string());
        while self.global_input_history.len() > MAX_GLOBAL_INPUT_HISTORY {
            self.global_input_history.pop_front();
        }
    }

    fn recent_shell_commands(&self, limit: usize) -> Vec<String> {
        self.global_shell_history
            .iter()
            .rev()
            .take(limit)
            .map(|entry| entry.command.clone())
            .collect()
    }

    fn recent_input_history(&self, limit: usize) -> Vec<String> {
        self.global_input_history
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    fn history_completion_for_prefix(
        &self,
        prefix: &str,
        cwd: Option<&str>,
        mode: InputMode,
        is_remote: bool,
    ) -> Option<String> {
        let mut fallback: Option<String> = None;

        for entry in self.global_shell_history.iter().rev() {
            if entry.command == prefix || !entry.command.starts_with(prefix) {
                continue;
            }

            if cwd.is_some() && entry.cwd.as_deref() == cwd {
                return Some(entry.command.clone());
            }

            if fallback.is_none() {
                fallback = Some(entry.command.clone());
            }
        }

        if fallback.is_some() {
            return fallback;
        }

        self.global_input_history
            .iter()
            .rev()
            .find(|entry| {
                entry.as_str() != prefix
                    && entry.starts_with(prefix)
                    && (mode == InputMode::Shell
                        || matches!(
                            self.harness.classify_input(entry, is_remote),
                            InputKind::ShellCommand(_)
                        ))
            })
            .cloned()
    }

    fn local_path_completion_for_prefix(
        &self,
        tab_idx: usize,
        pane_id: usize,
        input: &str,
        cx: &App,
    ) -> Option<LocalPathCompletion> {
        let pane_tree = &self.tabs.get(tab_idx)?.pane_tree;
        let terminal = pane_tree
            .pane_terminals()
            .into_iter()
            .find_map(|(id, terminal)| (id == pane_id).then_some(terminal))?;

        if self
            .effective_remote_host_for_tab(tab_idx, &terminal, cx)
            .is_some()
        {
            return None;
        }

        let cwd = terminal.current_dir(cx)?;
        let token_start = input
            .char_indices()
            .rev()
            .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx + ch.len_utf8()))
            .unwrap_or(0);
        let token = &input[token_start..];
        if token.is_empty() {
            return None;
        }

        let head = input[..token_start].trim_end();
        let first_word = head.split_whitespace().next().unwrap_or_default();
        let completes_path = first_word == "cd"
            || token.starts_with('~')
            || token.starts_with('.')
            || token.contains('/');
        if !completes_path {
            return None;
        }

        let directories_only = first_word == "cd";
        let home_dir = dirs::home_dir();
        let (search_dir, dir_prefix, search_prefix) = if let Some(stripped) =
            token.strip_prefix("~/")
        {
            let home = home_dir?;
            match stripped.rsplit_once('/') {
                Some((dir, prefix)) => (home.join(dir), format!("~/{dir}/"), prefix.to_string()),
                None => (home, "~/".to_string(), stripped.to_string()),
            }
        } else if token == "~" {
            let home = home_dir?;
            (home, String::new(), "~".to_string())
        } else if let Some((dir, prefix)) = token.rsplit_once('/') {
            let base = if dir.is_empty() {
                PathBuf::from("/")
            } else if Path::new(dir).is_absolute() {
                PathBuf::from(dir)
            } else {
                PathBuf::from(&cwd).join(dir)
            };
            (base, format!("{dir}/"), prefix.to_string())
        } else {
            (PathBuf::from(&cwd), String::new(), token.to_string())
        };

        let mut matches = std::fs::read_dir(&search_dir)
            .ok()?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let file_type = entry.file_type().ok()?;
                if directories_only && !file_type.is_dir() {
                    return None;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                name.starts_with(&search_prefix)
                    .then_some((name, file_type.is_dir()))
            })
            .collect::<Vec<_>>();
        if matches.is_empty() {
            return None;
        }
        matches.sort_by(|a, b| a.0.cmp(&b.0));

        let matched_name = if matches.len() == 1 {
            let (name, is_dir) = &matches[0];
            let mut single = name.clone();
            if *is_dir {
                single.push('/');
            }
            single
        } else {
            let prefix = longest_common_prefix(matches.iter().map(|(name, _)| name.as_str()));
            if prefix.chars().count() <= search_prefix.chars().count() {
                let candidates = matches
                    .into_iter()
                    .map(|(name, is_dir)| {
                        let mut candidate = if token == "~" {
                            name
                        } else {
                            format!("{dir_prefix}{name}")
                        };
                        if is_dir {
                            candidate.push('/');
                        }
                        format!("{}{}", &input[..token_start], candidate)
                    })
                    .collect::<Vec<_>>();
                return Some(LocalPathCompletion::Candidates(candidates));
            }
            prefix
        };

        let completed_token = if token == "~" {
            matched_name
        } else {
            format!("{dir_prefix}{matched_name}")
        };

        Some(LocalPathCompletion::Inline(format!(
            "{}{}",
            &input[..token_start],
            completed_token
        )))
    }

    fn refresh_input_suggestion(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_active_tab() {
            log::debug!(target: "con::suggestions", "skip suggestion: no active tab");
            self.shell_suggestion_engine.cancel();
            self.input_bar
                .update(cx, |bar, _cx| bar.clear_completion_ui());
            return;
        }

        let (text, mode, target_ids) = self.input_bar.update(cx, |bar, cx| {
            (bar.current_text(cx), bar.mode(), bar.target_pane_ids())
        });

        let trimmed = text.trim();
        if trimmed.is_empty()
            || text.contains('\n')
            || trimmed.starts_with('/')
            || target_ids.len() != 1
        {
            log::debug!(
                target: "con::suggestions",
                "skip suggestion: empty={} multiline={} slash={} targets={}",
                trimmed.is_empty(),
                text.contains('\n'),
                trimmed.starts_with('/'),
                target_ids.len()
            );
            self.shell_suggestion_engine.cancel();
            self.input_bar
                .update(cx, |bar, _cx| bar.clear_completion_ui());
            return;
        }

        let pane_id = target_ids[0];
        let pane_tree = &self.tabs[self.active_tab].pane_tree;
        let pane = pane_tree
            .pane_terminals()
            .into_iter()
            .find_map(|(id, terminal)| (id == pane_id).then_some(terminal));
        let cwd = pane.as_ref().and_then(|pane| pane.current_dir(cx));
        let is_remote = pane
            .as_ref()
            .is_some_and(|terminal| {
                self.effective_remote_host_for_tab(self.active_tab, terminal, cx)
                    .is_some()
            });

        if let Some(path_match) =
            self.local_path_completion_for_prefix(self.active_tab, pane_id, &text, cx)
        {
            self.shell_suggestion_engine.cancel();
            match path_match {
                LocalPathCompletion::Inline(path_match) => {
                    log::debug!(
                        target: "con::suggestions",
                        "use path suggestion prefix={:?} completion={:?}",
                        text,
                        path_match
                    );
                    self.input_bar.update(cx, |bar, _cx| {
                        bar.set_path_inline_suggestion(&text, &path_match);
                    });
                }
                LocalPathCompletion::Candidates(candidates) => {
                    log::debug!(
                        target: "con::suggestions",
                        "use path candidates prefix={:?} count={}",
                        text,
                        candidates.len()
                    );
                    self.input_bar.update(cx, |bar, _cx| {
                        bar.set_path_completion_candidates(&text, candidates);
                    });
                }
            }
            return;
        }

        if let Some(history_match) =
            self.history_completion_for_prefix(&text, cwd.as_deref(), mode, is_remote)
        {
            log::debug!(
                target: "con::suggestions",
                "use history suggestion prefix={:?} completion={:?}",
                text,
                history_match
            );
            self.shell_suggestion_engine.cancel();
            self.input_bar.update(cx, |bar, _cx| {
                bar.set_history_inline_suggestion(&text, &history_match);
            });
            return;
        }

        self.input_bar
            .update(cx, |bar, _cx| bar.clear_completion_ui());

        let shell_probe_too_short = mode == InputMode::Smart && trimmed.chars().count() < 2;
        let is_shell_mode = match mode {
            InputMode::Shell => true,
            InputMode::Smart => {
                if shell_probe_too_short {
                    log::debug!(
                        target: "con::suggestions",
                        "skip ai suggestion: smart-mode probe too short prefix={:?}",
                        text
                    );
                    false
                } else {
                    matches!(
                        self.harness.classify_input(&text, is_remote),
                        InputKind::ShellCommand(_)
                    )
                }
            }
            InputMode::Agent => false,
        };

        if !is_shell_mode {
            log::debug!(
                target: "con::suggestions",
                "skip ai suggestion: input classified as non-shell prefix={:?}",
                text
            );
            self.shell_suggestion_engine.cancel();
            self.input_bar
                .update(cx, |bar, _cx| bar.clear_path_completion_candidates());
            return;
        }

        if !self.harness.config().suggestion_model.enabled {
            log::debug!(
                target: "con::suggestions",
                "skip ai suggestion: disabled in config prefix={:?}",
                text
            );
            self.shell_suggestion_engine.cancel();
            self.input_bar
                .update(cx, |bar, _cx| bar.clear_path_completion_candidates());
            return;
        }

        let suggestion_tx = self.shell_suggestion_tx.clone();
        let prefix = text.clone();
        let callback_prefix = prefix.clone();
        let tab_idx = self.active_tab;
        let recent_commands = self.recent_shell_commands(6);
        self.shell_suggestion_engine.request(
            &prefix,
            SuggestionContext {
                cwd,
                recent_commands,
            },
            move |completion| {
                let _ = suggestion_tx.send(ShellSuggestionResult {
                    tab_idx,
                    pane_id,
                    prefix: callback_prefix.clone(),
                    completion,
                });
            },
        );
    }

    fn apply_shell_suggestion(&mut self, result: ShellSuggestionResult, cx: &mut Context<Self>) {
        if result.tab_idx != self.active_tab {
            log::debug!(
                target: "con::suggestions",
                "drop ai suggestion for inactive tab={} active={}",
                result.tab_idx,
                self.active_tab
            );
            return;
        }

        let (text, mode, target_ids) = self.input_bar.update(cx, |bar, cx| {
            (bar.current_text(cx), bar.mode(), bar.target_pane_ids())
        });

        if text != result.prefix
            || matches!(mode, InputMode::Agent)
            || target_ids.as_slice() != [result.pane_id]
            || text.trim().starts_with('/')
            || text.contains('\n')
        {
            log::debug!(
                target: "con::suggestions",
                "drop ai suggestion prefix={:?}: text/mode/target changed",
                result.prefix
            );
            return;
        }

        let pane_tree = &self.tabs[self.active_tab].pane_tree;
        let cwd = pane_tree
            .pane_terminals()
            .into_iter()
            .find_map(|(id, terminal)| (id == result.pane_id).then(|| terminal.current_dir(cx)))
            .flatten();

        let is_remote = self
            .tabs
            .get(self.active_tab)
            .and_then(|tab| {
                tab.pane_tree
                    .pane_terminals()
                    .into_iter()
                    .find_map(|(id, terminal)| {
                        (id == result.pane_id).then(|| {
                            self.effective_remote_host_for_tab(self.active_tab, &terminal, cx)
                                .is_some()
                        })
                    })
            })
            .unwrap_or(false);

        if self
            .history_completion_for_prefix(&text, cwd.as_deref(), mode, is_remote)
            .is_some()
        {
            log::debug!(
                target: "con::suggestions",
                "drop ai suggestion prefix={:?}: history match became available",
                result.prefix
            );
            return;
        }

        log::debug!(
            target: "con::suggestions",
            "apply ai suggestion prefix={:?} completion={:?}",
            result.prefix,
            result.completion
        );
        self.input_bar.update(cx, |bar, _cx| {
            bar.set_ai_inline_suggestion(&result.prefix, &result.completion);
        });
        cx.notify();
    }

    fn execute_shell(&mut self, cmd: &str, window: &mut Window, cx: &mut Context<Self>) {
        let target_ids = self.input_bar.read(cx).target_pane_ids();
        let pane_tree = &self.tabs[self.active_tab].pane_tree;
        let all_terminals = pane_tree.all_terminals();
        let mut history_records = Vec::new();

        for terminal in &all_terminals {
            if all_terminals.len() == 1
                || target_ids
                    .iter()
                    .any(|&tid| pane_tree.terminal_has_pane_id(terminal, tid))
            {
                terminal.write(format!("{}\n", cmd).as_bytes(), cx);
                if let Some(pane_id) = pane_tree.pane_id_for_terminal(terminal) {
                    history_records.push((pane_id, terminal.current_dir(cx)));
                }
            }
        }

        for (pane_id, cwd) in history_records {
            self.record_shell_command(self.active_tab, pane_id, cmd, cwd);
        }

        self.input_bar.update(cx, |bar, _cx| {
            bar.clear_completion_ui();
        });
        self.save_session(cx);

        // Always keep focus on input bar after sending a command —
        // the terminal output is visible, and the user can click to focus it.
        self.input_bar.focus_handle(cx).focus(window, cx);
    }

    fn send_to_agent(&mut self, content: &str, cx: &mut Context<Self>) {
        if !self.has_active_tab() {
            return;
        }
        self.record_input_history(content);
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
        self.save_session(cx);
    }

    fn split_pane(
        &mut self,
        direction: SplitDirection,
        placement: SplitPlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }
        let cwd = self.active_terminal().current_dir(cx);
        let terminal = self.create_terminal(cwd.as_deref(), window, cx);
        self.tabs[self.active_tab].pane_tree.split_with_placement(
            direction,
            placement,
            terminal.clone(),
        );
        self.record_runtime_event_for_terminal(
            self.active_tab,
            &terminal,
            con_agent::context::PaneRuntimeEvent::PaneCreated {
                startup_command: None,
            },
        );
        terminal.ensure_surface(window, cx);
        terminal.notify(cx);
        terminal.focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        Self::schedule_terminal_bootstrap_reassert(
            &terminal,
            true,
            self.window_handle,
            self.workspace_handle.clone(),
            cx,
        );
        self.save_session(cx);
        cx.notify();
    }

    fn split_right(&mut self, _: &SplitRight, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_active_tab() {
            self.split_pane(
                SplitDirection::Horizontal,
                SplitPlacement::After,
                window,
                cx,
            );
        }
    }

    fn split_down(&mut self, _: &SplitDown, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_active_tab() {
            self.split_pane(
                SplitDirection::Vertical,
                SplitPlacement::After,
                window,
                cx,
            );
        }
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

        self.active_tab = index;
        self.tabs[index].needs_attention = false;

        // Show new tab's ghostty NSViews and focus active surface
        for terminal in self.tabs[index].pane_tree.all_terminals() {
            terminal.set_native_view_visible(true, cx);
            terminal.ensure_surface(window, cx);
        }
        for terminal in self.tabs[old_active].pane_tree.all_terminals() {
            terminal.set_focus_state(false, cx);
        }
        let old_terminals: Vec<TerminalPane> = self.tabs[old_active]
            .pane_tree
            .all_terminals()
            .into_iter()
            .cloned()
            .collect();
        cx.on_next_frame(window, move |_workspace, _window, cx| {
            for terminal in &old_terminals {
                terminal.set_native_view_visible(false, cx);
            }
        });
        let focused = self.tabs[index].pane_tree.focused_terminal();
        focused.focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        Self::schedule_terminal_bootstrap_reassert(
            focused,
            true,
            self.window_handle,
            self.workspace_handle.clone(),
            cx,
        );

        self.save_session(cx);
        cx.notify();
    }

    fn next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }
        let next = (self.active_tab + 1) % self.tabs.len();
        self.activate_tab(next, window, cx);
    }

    fn previous_tab(&mut self, _: &PreviousTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }
        let prev = if self.active_tab == 0 {
            self.tabs.len() - 1
        } else {
            self.active_tab - 1
        };
        self.activate_tab(prev, window, cx);
    }

    /// Focus the active terminal (used after modal close, etc.)
    fn focus_terminal(&self, window: &mut Window, cx: &mut App) {
        self.active_terminal().focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
    }

    fn restore_terminal_focus_after_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_active_tab() {
            return;
        }
        self.focus_terminal(window, cx);
        cx.on_next_frame(window, |workspace, window, cx| {
            if workspace.has_active_tab() && !workspace.settings_panel.read(cx).is_visible() {
                workspace.focus_terminal(window, cx);
                cx.notify();
            }
        });
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
            self.sync_active_terminal_focus_states(cx);
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

        if self.tabs[tab_idx].pane_tree.pane_count() > 1 {
            let mut focus_after_close = None;
            {
                let pane_tree = &mut self.tabs[tab_idx].pane_tree;
                // Close the specific pane whose process exited, not the focused pane.
                if let Some(pane_id) = pane_tree.pane_id_for_entity(entity_id) {
                    let closing = pane_tree
                        .all_terminals()
                        .into_iter()
                        .find(|terminal| terminal.entity_id() == entity_id)
                        .cloned();
                    pane_tree.close_pane(pane_id);
                    let surviving_terminals: Vec<TerminalPane> =
                        pane_tree.all_terminals().into_iter().cloned().collect();
                    for terminal in &surviving_terminals {
                        terminal.set_native_view_visible(true, cx);
                        terminal.ensure_surface(window, cx);
                        terminal.notify(cx);
                    }
                    if let Some(closing) = closing {
                        cx.on_next_frame(window, move |_workspace, _window, cx| {
                            closing.shutdown_surface(cx);
                            for terminal in &surviving_terminals {
                                terminal.notify(cx);
                            }
                        });
                    }
                }
                if tab_idx == self.active_tab {
                    focus_after_close = Some(pane_tree.focused_terminal().clone());
                }
            }

            if let Some(focused) = focus_after_close {
                focused.focus(window, cx);
                self.sync_active_terminal_focus_states(cx);
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

    pub(crate) fn on_terminal_split_requested(
        &mut self,
        entity: &Entity<GhosttyView>,
        event: &GhosttySplitRequested,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let entity_id = entity.entity_id();
        let Some(tab_idx) = self
            .tabs
            .iter()
            .position(|tab| tab.pane_tree.pane_id_for_entity(entity_id).is_some())
        else {
            return;
        };
        let Some(origin_pane_id) = self.tabs[tab_idx].pane_tree.pane_id_for_entity(entity_id)
        else {
            return;
        };

        let (direction, placement) = match event.0 {
            con_ghostty::GhosttySplitDirection::Right => {
                (SplitDirection::Horizontal, SplitPlacement::After)
            }
            con_ghostty::GhosttySplitDirection::Down => {
                (SplitDirection::Vertical, SplitPlacement::After)
            }
            con_ghostty::GhosttySplitDirection::Left => {
                (SplitDirection::Horizontal, SplitPlacement::Before)
            }
            con_ghostty::GhosttySplitDirection::Up => {
                (SplitDirection::Vertical, SplitPlacement::Before)
            }
        };

        let origin_terminal = self.tabs[tab_idx]
            .pane_tree
            .all_terminals()
            .into_iter()
            .find(|terminal| terminal.entity_id() == entity_id)
            .cloned();
        let cwd = origin_terminal
            .as_ref()
            .and_then(|terminal| terminal.current_dir(cx));

        let terminal = self.create_terminal(cwd.as_deref(), window, cx);
        self.tabs[tab_idx].pane_tree.split_pane_with_placement(
            origin_pane_id,
            direction,
            placement,
            terminal.clone(),
        );
        terminal.ensure_surface(window, cx);
        terminal.notify(cx);
        terminal.focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        Self::schedule_terminal_bootstrap_reassert(
            &terminal,
            true,
            self.window_handle,
            self.workspace_handle.clone(),
            cx,
        );
        self.active_tab = tab_idx;
        self.record_runtime_event_for_terminal(
            tab_idx,
            &terminal,
            con_agent::context::PaneRuntimeEvent::PaneCreated {
                startup_command: None,
            },
        );
        self.save_session(cx);
        cx.notify();
    }
}

impl Render for ConWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.flush_pending_create_pane_requests(window, cx);

        if !self.has_active_tab() {
            return div().size_full().into_any_element();
        }

        self.sync_window_resize_increments(window, cx);

        let active_terminal = self.active_terminal().clone();

        // If a modal was dismissed internally (escape/backdrop), restore terminal focus
        let is_modal_open = self.is_modal_open(cx);
        let has_skill_popup = !self.input_bar.read(cx).filtered_skills(cx).is_empty();
        let has_path_popup = self.input_bar.read(cx).has_path_completion_candidates();
        let has_inline_skill_popup = self.agent_panel_open
            && !self.input_bar_visible
            && !self
                .agent_panel
                .read(cx)
                .filtered_inline_skills(cx)
                .is_empty();
        let needs_ghostty_hidden = false;

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
            let terminals: Vec<TerminalPane> = self.tabs[self.active_tab]
                .pane_tree
                .all_terminals()
                .into_iter()
                .cloned()
                .collect();
            cx.on_next_frame(window, move |_workspace, _window, cx| {
                for terminal in &terminals {
                    terminal.refresh_surface(cx);
                }
            });
        }
        self.modal_was_open = is_modal_open;

        if !needs_ghostty_hidden {
            for terminal in self.tabs[self.active_tab].pane_tree.all_terminals() {
                terminal.set_native_view_visible(true, cx);
            }
        }

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
                let hostname = self
                    .cached_runtime_for_tab(self.active_tab, &terminal)
                    .and_then(|runtime| runtime.remote_host);
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
        self.input_bar.update(cx, |bar, cx| {
            bar.set_panes(pane_infos, focused_pane_id, window, cx);
            bar.set_cwd(display_cwd);
            bar.set_skills(skill_entries);
        });
        // Up/Down is command-bar recall, not shell suggestion ranking. Keep it
        // backed by the global submitted-input history across all modes.
        let recent_commands = self.recent_input_history(80);
        self.input_bar
            .update(cx, |bar, _cx| bar.set_recent_commands(recent_commands));

        // Sync model name, inline input, and skills to agent panel
        let model_name = self.harness.active_model_name();
        let provider = self.harness.config().provider.clone();
        let available_models = self.model_registry.models_for(&provider);
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
        self.agent_panel.update(cx, |panel, cx| {
            panel.set_session_provider_options(
                AgentPanel::configured_session_providers(self.harness.config()),
                window,
                cx,
            );
            panel.set_provider_name(provider, window, cx);
            panel.set_model_name(model_name);
            panel.set_session_model_options(available_models, window, cx);
            panel.set_show_inline_input(show_inline);
            panel.set_skills(panel_skills);
            panel.set_recent_inputs(self.recent_input_history(80));
        });

        let agent_panel_progress = self.agent_panel_motion.value(window);
        let input_bar_progress = self.input_bar_motion.value(window);
        let tab_strip_progress = self.tab_strip_motion.value(window);
        let agent_panel_transitioning = self.agent_panel_motion.is_animating();
        let input_bar_transitioning = self.input_bar_motion.is_animating();
        let theme = cx.theme();
        let ui_surface_opacity = self.ui_surface_opacity();
        let elevated_ui_surface_opacity = self.elevated_ui_surface_opacity();
        let agent_panel_content_progress = ((agent_panel_progress - 0.16) / 0.84)
            .clamp(0.0, 1.0)
            .powf(0.9);
        let input_bar_content_progress = ((input_bar_progress - 0.08) / 0.92)
            .clamp(0.0, 1.0)
            .powf(0.92);
        let compact_titlebar_progress = 1.0 - tab_strip_progress;
        let effective_agent_panel_width = self
            .agent_panel_width
            .min(max_agent_panel_width(window.bounds().size.width.as_f32()));
        let animated_panel_width = effective_agent_panel_width * agent_panel_progress;

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
            .bg(theme.transparent)
            .child(pane_tree_rendered);

        let mut main_area = div().flex().flex_1().min_h_0().child(terminal_area);

        if agent_panel_progress > 0.01 {
            main_area = main_area.child(
                div()
                    .w(px(animated_panel_width + 1.0))
                    .h_full()
                    .overflow_hidden()
                    .flex_shrink_0()
                    .flex()
                    .flex_row()
                    .bg(theme.background.opacity(elevated_ui_surface_opacity))
                    .child(
                        div()
                            .id("agent-panel-divider")
                            .relative()
                            .w(px(1.0))
                            .h_full()
                            .flex_shrink_0()
                            .bg(theme.title_bar_border)
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left(px(-2.0))
                                    .w(px(5.0))
                                    .cursor_col_resize()
                                    .bg(theme.background.opacity(elevated_ui_surface_opacity))
                                    .hover(|s| {
                                        s.bg(theme
                                            .background
                                            .opacity((elevated_ui_surface_opacity + 0.08).min(1.0)))
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                                            this.release_active_terminal_mouse_selection(cx);
                                            this.agent_panel_drag = Some((
                                                f32::from(event.position.x),
                                                effective_agent_panel_width,
                                            ));
                                        }),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .opacity(agent_panel_content_progress)
                            .child(self.agent_panel.clone()),
                    ),
            );
        }

        // Top bar — compact titlebar for one tab, full strip for many
        let tab_count = self.tabs.len();
        let top_bar_height = self.current_top_bar_height();
        let top_bar_controls_offset = 1.0 + (3.0 * tab_strip_progress);

        // macOS: leave 78px for the system traffic-light cluster that
        // the OS paints over our content. Windows / Linux: start flush
        // at the left; the Min/Max/Close cluster gets appended at the
        // right end of the bar below. Marking the whole bar as a
        // `Drag` control area makes it move the window on non-macOS
        // (GPUI's hit-test walks buttons first so child clickables
        // still work) and still lets macOS react to
        // `titlebar_double_click`.
        let leading_pad = if cfg!(target_os = "macos") {
            78.0
        } else {
            8.0
        };
        let mut top_bar = div()
            .id("tab-bar")
            .flex()
            .h(px(top_bar_height))
            .items_end()
            .pl(px(leading_pad))
            .pr(px(6.0))
            .bg(theme.title_bar.opacity(ui_surface_opacity))
            .window_control_area(WindowControlArea::Drag)
            .on_click(|event, window, _cx| {
                if event.click_count() == 2 {
                    window.titlebar_double_click();
                }
            });

        // Tabs container — appears only when there is real tab selection to do
        let mut tabs_container = div().flex().flex_1().min_w_0().items_end();

        if tab_count > 1 {
            for (index, tab) in self.tabs.iter().enumerate() {
                let is_active = index == self.active_tab;
                let needs_attention = tab.needs_attention && !is_active;
                let terminal = tab.pane_tree.focused_terminal();
                let title = terminal.title(cx).unwrap_or_else(|| tab.title.clone());

                let display_title: String = if title.chars().count() > 24 {
                    format!("{}…", &title[..title.floor_char_boundary(22)])
                } else {
                    title
                };

                let close_id = ElementId::Name(format!("tab-close-{}", index).into());

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
                if !is_active {
                    close_el = close_el.invisible().group_hover("tab", |s| s.visible());
                }
                let close_button = close_el
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
                    );

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
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(move |this, _, window, cx| {
                            this.close_tab_by_index(index, window, cx);
                        }),
                    );

                if is_active {
                    tab_el = tab_el
                        .rounded_t(px(7.0))
                        .bg(theme.background.opacity(elevated_ui_surface_opacity))
                        .text_color(theme.foreground)
                        .font_weight(FontWeight::MEDIUM);
                } else {
                    tab_el = tab_el
                        .rounded_t(px(6.0))
                        .mb(px(1.0))
                        .bg(theme.background.opacity(0.14))
                        .text_color(theme.muted_foreground.opacity(0.72))
                        .hover(|s| {
                            s.bg(theme.background.opacity(0.20))
                                .text_color(theme.foreground.opacity(0.82))
                        });
                }

                let mut tab_content = div().flex().items_center().gap(px(5.0)).w_full().min_w_0();

                if needs_attention {
                    tab_content = tab_content.child(
                        div()
                            .size(px(5.0))
                            .rounded_full()
                            .flex_shrink_0()
                            .bg(theme.primary),
                    );
                }

                tab_content = tab_content.child(
                    svg()
                        .path("phosphor/terminal.svg")
                        .size(px(12.0))
                        .flex_shrink_0()
                        .text_color(if is_active {
                            theme.foreground.opacity(0.74)
                        } else {
                            theme.muted_foreground.opacity(0.56)
                        }),
                );

                tab_content = tab_content.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_x_hidden()
                        .whitespace_nowrap()
                        .child(display_title),
                );

                tab_content = tab_content.child(close_button);
                tabs_container = tabs_container.child(tab_el.child(tab_content));
            }
        }

        let mut leading_chrome = div().flex().flex_1().min_w_0().items_end();
        if tab_strip_progress > 0.01 {
            leading_chrome = leading_chrome.child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .opacity(tab_strip_progress)
                    .child(tabs_container),
            );
        }

        top_bar = top_bar.child(leading_chrome);

        // Right-side controls — compact row
        let mut tab_controls = div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .mb(px(top_bar_controls_offset))
            .flex_shrink_0();

        tab_controls = tab_controls.child(
            div()
                .id("tab-new")
                .flex()
                .items_center()
                .justify_center()
                .size(px(22.0))
                .rounded(px(5.0))
                .cursor_pointer()
                // `.occlude()` is required on Windows so the parent
                // top_bar's `WindowControlArea::Drag` hit-test doesn't
                // swallow this button (the OS would return HTCAPTION and
                // start a window-drag on click instead of firing the
                // click listener). Same treatment as the Min/Max/Close
                // caption buttons at the top of this file.
                .occlude()
                .hover(|s| s.bg(theme.muted.opacity(0.10)))
                .tooltip(|window, cx| {
                    chrome_tooltip(
                        "New tab",
                        crate::keycaps::first_action_keystroke(&NewTab, window),
                        window,
                        cx,
                    )
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    this.new_tab(&NewTab, window, cx);
                }))
                .child(
                    svg().path("phosphor/plus.svg").size(px(12.0)).text_color(
                        theme
                            .muted_foreground
                            .opacity(0.45 + (0.08 * compact_titlebar_progress)),
                    ),
                ),
        );

        // Input bar toggle
        let input_bar_tooltip = if self.input_bar_visible {
            "Hide input bar"
        } else {
            "Show input bar"
        };
        tab_controls = tab_controls.child(
            div()
                .id("toggle-input-bar")
                .flex()
                .items_center()
                .justify_center()
                .size(px(22.0))
                .rounded(px(5.0))
                .cursor_pointer()
                .occlude()
                .hover(|s| s.bg(theme.muted.opacity(0.10)))
                .tooltip(move |window, cx| {
                    chrome_tooltip(
                        input_bar_tooltip,
                        crate::keycaps::first_action_keystroke(&crate::ToggleInputBar, window),
                        window,
                        cx,
                    )
                })
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
        let agent_panel_tooltip = if self.agent_panel_open {
            "Hide agent panel"
        } else {
            "Show agent panel"
        };
        tab_controls = tab_controls.child(
            div()
                .id("toggle-agent-panel")
                .flex()
                .items_center()
                .justify_center()
                .size(px(22.0))
                .rounded(px(5.0))
                .cursor_pointer()
                .occlude()
                .hover(|s| s.bg(theme.muted.opacity(0.10)))
                .tooltip(move |window, cx| {
                    chrome_tooltip(
                        agent_panel_tooltip,
                        crate::keycaps::first_action_keystroke(&ToggleAgentPanel, window),
                        window,
                        cx,
                    )
                })
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

        top_bar = top_bar.child(tab_controls);

        // Non-macOS caption buttons: Min / (Max|Restore) / Close.
        // macOS gets its traffic-light cluster from the system. We
        // render these *inside* the top bar so they share the same
        // vertical strip and never occlude terminal content.
        #[cfg(not(target_os = "macos"))]
        {
            top_bar = top_bar.child(caption_buttons(window, theme, top_bar_height));
        }

        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.transparent)
            .font_family(theme.mono_font_family.clone())
            .key_context("ConWorkspace")
            // Pane drag-to-resize: capture mouse move/up on root so it works
            // even when cursor is over terminal views (which capture mouse events).
            .on_mouse_move({
                let pending = self.pending_drag_init.clone();
                cx.listener(move |this, event: &MouseMoveEvent, win, cx| {
                    // Agent panel resize drag
                    if let Some((start_x, start_width)) = this.agent_panel_drag {
                        let delta = start_x - f32::from(event.position.x);
                        let max_width = max_agent_panel_width(win.bounds().size.width.as_f32());
                        let new_width = (start_width + delta)
                            .clamp(AGENT_PANEL_MIN_WIDTH, max_width);
                        if (this.agent_panel_width - new_width).abs() > 1.0 {
                            this.agent_panel_width = new_width;
                            if this.active_tab >= this.tabs.len() {
                                cx.notify();
                                return;
                            }
                            cx.notify();
                        }
                        return;
                    }

                    if this.active_tab >= this.tabs.len() {
                        return;
                    }

                    let top_bar_height = this.current_top_bar_height();
                    let input_bar_height = if this.input_bar_visible { 42.0 } else { 0.0 };
                    // Consume a pending drag initiation written by divider on_mouse_down
                    if let Ok(mut guard) = pending.lock() {
                        if let Some((split_id, start_pos)) = guard.take() {
                            this.release_active_terminal_mouse_selection(cx);
                            let pane_tree = &mut this.tabs[this.active_tab].pane_tree;
                            pane_tree.begin_drag(split_id, start_pos);
                        }
                    }

                    let pane_tree = &mut this.tabs[this.active_tab].pane_tree;

                    if !pane_tree.is_dragging() {
                        return;
                    }

                    // Estimate terminal area from window bounds minus fixed chrome
                    // (tab bar ~38px, input bar ~40px, agent panel if open)
                    let win_w = f32::from(win.bounds().size.width);
                    let win_h = f32::from(win.bounds().size.height);
                    let effective_agent_panel_width =
                        this.agent_panel_width.min(max_agent_panel_width(win_w));
                    let (current_pos, total_size) =
                        if let Some(dir) = pane_tree.dragging_direction() {
                            match dir {
                                SplitDirection::Horizontal => {
                                    let panel_w = if this.agent_panel_open {
                                        effective_agent_panel_width + 7.0
                                    } else {
                                        0.0
                                    };
                                    (f32::from(event.position.x), win_w - panel_w)
                                }
                                SplitDirection::Vertical => (
                                    f32::from(event.position.y),
                                    win_h - top_bar_height - input_bar_height,
                                ),
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
                    if this.agent_panel_drag.is_some() {
                        this.agent_panel_drag = None;
                        this.save_session(cx);
                        cx.notify();
                        return;
                    }
                    if this.active_tab >= this.tabs.len() {
                        return;
                    }
                    let pane_tree = &mut this.tabs[this.active_tab].pane_tree;
                    if pane_tree.is_dragging() {
                        pane_tree.end_drag();
                        for terminal in pane_tree.all_terminals() {
                            terminal.notify(cx);
                        }
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
            .on_action(cx.listener(Self::next_tab))
            .on_action(cx.listener(Self::previous_tab))
            .on_action(cx.listener(Self::close_tab))
            .on_action(cx.listener(Self::split_right))
            .on_action(cx.listener(Self::split_down))
            .on_action(cx.listener(Self::focus_input))
            .on_action(cx.listener(Self::cycle_input_mode))
            .on_action(cx.listener(Self::toggle_pane_scope_picker))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // Don't handle workspace shortcuts when a modal overlay is open
                if this.settings_panel.read(cx).is_visible()
                    || this.command_palette.read(cx).is_visible()
                {
                    return;
                }

                let mods = &event.keystroke.modifiers;
                let key = event.keystroke.key.as_str();

                if this.pane_scope_picker_open {
                    if key == "escape" {
                        this.close_pane_scope_picker(cx);
                        return;
                    }

                    if mods.platform && key == "a" {
                        this.set_scope_broadcast(window, cx);
                        return;
                    }

                    if mods.platform && key == "f" {
                        this.set_scope_focused(window, cx);
                        return;
                    }

                    if mods.platform && !mods.shift {
                        if let Some(digit) = key.chars().next().and_then(|c| c.to_digit(10)) {
                            let pane_index = if digit == 0 { 9 } else { (digit - 1) as usize };
                            this.toggle_scope_pane_by_index(pane_index, window, cx);
                            return;
                        }
                    }
                }

                // Cmd+1..9 — jump to tab
                if mods.platform && !mods.shift {
                    if let Some(digit) = key.chars().next().and_then(|c| c.to_digit(10)) {
                        let tab_index = if digit == 0 { 9 } else { (digit - 1) as usize };
                        if tab_index < this.tabs.len() {
                            this.activate_tab(tab_index, window, cx);
                        }
                    }
                }

                // Browser-style fallbacks. The configurable actions also bind
                // Control-Tab / Control-Shift-Tab by default.
                if mods.platform && mods.shift && key == "[" {
                    this.previous_tab(&PreviousTab, window, cx);
                }

                if mods.platform && mods.shift && key == "]" {
                    this.next_tab(&NextTab, window, cx);
                }
            }))
            .child(top_bar)
            .child(main_area);

        if agent_panel_transitioning && agent_panel_progress > 0.01 {
            root = root.child(
                div()
                    .absolute()
                    .top(px(top_bar_height))
                    .bottom(px(43.0 * input_bar_progress))
                    .right(px(animated_panel_width))
                    .w(px(CHROME_TRANSITION_SEAM_COVER))
                    .bg(theme.background.opacity(elevated_ui_surface_opacity)),
            );
        }

        if input_bar_transitioning && input_bar_progress > 0.01 {
            root = root.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(px(43.0 * input_bar_progress))
                    .h(px(CHROME_TRANSITION_SEAM_COVER))
                    .bg(theme.title_bar.opacity(ui_surface_opacity)),
            );
        }

        if input_bar_progress > 0.01 {
            root = root.child(
                div()
                    .overflow_hidden()
                    .h(px(43.0 * input_bar_progress))
                    .bg(theme.title_bar.opacity(ui_surface_opacity))
                    .child(div().h(px(1.0)).bg(theme.title_bar_border))
                    .child(
                        div()
                            .h(px(42.0))
                            .opacity(input_bar_content_progress)
                            .child(self.input_bar.clone()),
                    ),
            );
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
                .font_family(theme.font_family.clone());

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

        if has_path_popup && !has_skill_popup {
            let theme = cx.theme();
            let popup_width = px((window.bounds().size.width.as_f32() * 0.32).clamp(320.0, 440.0));
            let popup_bottom = self.input_bar.read(cx).skill_popup_offset(cx);
            let candidates = self.input_bar.read(cx).path_completion_candidates();
            let sel = self
                .input_bar
                .read(cx)
                .path_completion_selection()
                .min(candidates.len().saturating_sub(1));

            let mut popup = div()
                .absolute()
                .bottom(popup_bottom)
                .left(px(24.0))
                .w(popup_width)
                .max_h(px(280.0))
                .flex()
                .flex_col()
                .rounded(px(10.0))
                .bg(theme.background.opacity(elevated_ui_surface_opacity))
                .border_1()
                .border_color(theme.muted.opacity(0.16))
                .py(px(6.0))
                .overflow_hidden()
                .font_family(theme.mono_font_family.clone());

            for (i, candidate) in candidates.iter().enumerate() {
                let is_sel = i == sel;
                let candidate_ix = i;
                popup = popup.child(
                    div()
                        .id(SharedString::from(format!("path-candidate-{i}")))
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
                                    let _ = bar.accept_path_completion_candidate_at(
                                        candidate_ix,
                                        window,
                                        cx,
                                    );
                                });
                            })
                        })
                        .child(
                            div()
                                .text_size(px(12.0))
                                .font_family(theme.mono_font_family.clone())
                                .text_color(if is_sel {
                                    theme.primary
                                } else {
                                    theme.foreground
                                })
                                .child(candidate.clone()),
                        ),
                );
            }
            root = root.child(popup);
        }

        if self.pane_scope_picker_open
            && self.input_bar_visible
            && self.input_bar.read(cx).mode() != InputMode::Agent
        {
            let panes = self.input_bar.read(cx).pane_infos();
            if panes.len() > 1 {
                let focused_id = self.input_bar.read(cx).focused_pane_id();
                let selected_ids: HashSet<usize> = self
                    .input_bar
                    .read(cx)
                    .scope_selected_ids()
                    .into_iter()
                    .collect();
                let is_broadcast = self.input_bar.read(cx).is_broadcast_scope();
                let is_focused = self.input_bar.read(cx).is_focused_scope();
                let layout = self.active_pane_layout(cx);
                let pane_map: HashMap<usize, PaneInfo> =
                    panes.iter().cloned().map(|pane| (pane.id, pane)).collect();
                let display_indices: HashMap<usize, usize> = panes
                    .iter()
                    .enumerate()
                    .map(|(ix, pane)| (pane.id, ix))
                    .collect();
                let popup_width =
                    px((window.bounds().size.width.as_f32() * 0.38).clamp(360.0, 520.0));
                let popup_bottom = px(58.0 + (43.0 * input_bar_progress.max(0.01)));
                let preview_content = self.render_scope_node(
                    &layout,
                    &pane_map,
                    &display_indices,
                    &selected_ids,
                    focused_id,
                    cx,
                );
                let theme = cx.theme();
                let popup_surface = if theme.is_dark() {
                    theme.background.opacity(elevated_ui_surface_opacity)
                } else {
                    theme.background.opacity(0.98)
                };
                let preview_surface = if theme.is_dark() {
                    theme.title_bar.opacity(ui_surface_opacity * 0.98)
                } else {
                    theme.muted.opacity(0.055)
                };
                let segmented_surface = if theme.is_dark() {
                    theme.title_bar.opacity(ui_surface_opacity * 0.96)
                } else {
                    theme.muted.opacity(0.065)
                };
                let scope_frame_inset = px(4.0);
                let scope_frame_radius = px(10.0);

                let presets = div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.0))
                    .child(
                        div().flex().items_center().gap(px(6.0)).child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.0))
                                .child(
                                    div()
                                        .text_size(px(12.0))
                                        .font_family(theme.mono_font_family.clone())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Pane scope"),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.5))
                                        .line_height(px(13.0))
                                        .font_family(theme.mono_font_family.clone())
                                        .text_color(theme.muted_foreground.opacity(0.58))
                                        .child("Choose where command-mode input is sent"),
                                ),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(5.0))
                            .child(crate::keycaps::keycaps_for_binding("secondary-'", theme))
                            .child(
                                div()
                                    .h(px(19.0))
                                    .px(px(7.0))
                                    .rounded(px(5.0))
                                    .flex()
                                    .items_center()
                                    .bg(theme.muted.opacity(0.12))
                                    .text_size(px(10.5))
                                    .font_family(theme.mono_font_family.clone())
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.foreground.opacity(0.66))
                                    .child("1-9"),
                            ),
                    );

                let preset_segment = |id: &'static str, label: &'static str, active: bool| {
                    div()
                        .flex_1()
                        .child(
                            div()
                                .id(SharedString::from(id))
                                .h(px(24.0))
                                .w_full()
                                .rounded(px(7.0))
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(if active {
                                    theme.primary.opacity(0.14)
                                } else {
                                    theme.transparent
                                })
                                .hover(|s| {
                                    s.bg(if active {
                                        theme.primary.opacity(0.16)
                                    } else {
                                        theme.muted.opacity(0.05)
                                    })
                                })
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .line_height(px(13.0))
                                        .font_family(theme.mono_font_family.clone())
                                        .font_weight(if active {
                                            FontWeight::MEDIUM
                                        } else {
                                            FontWeight::NORMAL
                                        })
                                        .text_color(if active {
                                            theme.primary
                                        } else {
                                            theme.muted_foreground.opacity(0.72)
                                        })
                                        .child(label),
                                ),
                        )
                };

                let presets_row = div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(
                        div()
                            .w_full()
                            .h(px(32.0))
                            .p(scope_frame_inset)
                            .rounded(scope_frame_radius)
                            .bg(segmented_surface)
                            .flex()
                            .items_center()
                            .gap(px(2.0))
                            .child(preset_segment(
                                "scope-all",
                                "All panes",
                                is_broadcast,
                            ).on_mouse_down(MouseButton::Left, cx.listener(
                                |this: &mut ConWorkspace, _: &MouseDownEvent, window, cx| {
                                    this.set_scope_broadcast(window, cx);
                                },
                            )))
                            .child(preset_segment(
                                "scope-focused",
                                "Focused",
                                is_focused,
                            ).on_mouse_down(MouseButton::Left, cx.listener(
                                |this: &mut ConWorkspace, _: &MouseDownEvent, window, cx| {
                                    this.set_scope_focused(window, cx);
                                },
                            ))),
                    )
                    .child(div().flex_1());

                let preview = div()
                    .h(px(224.0))
                    .w_full()
                    .rounded(scope_frame_radius)
                    .p(scope_frame_inset)
                    .bg(preview_surface)
                    .child(preview_content);

                root = root.child(
                    div()
                        .absolute()
                        .left(px(20.0))
                        .bottom(popup_bottom)
                        .w(popup_width)
                        .rounded(px(14.0))
                        .bg(popup_surface)
                        .p(px(12.0))
                        .flex()
                        .flex_col()
                        .gap(px(12.0))
                        .child(presets)
                        .child(presets_row)
                        .child(preview),
                );
            }
        }

        // Inline skill popup — rendered above the agent panel's inline input
        if has_inline_skill_popup {
            let theme = cx.theme();
            let popup_bottom = self.agent_panel.read(cx).inline_skill_popup_offset(cx);
            let effective_agent_panel_width = self
                .agent_panel_width
                .min(max_agent_panel_width(window.bounds().size.width.as_f32()));
            let panel_left = window.bounds().size.width.as_f32() - effective_agent_panel_width;
            let popup_left = px((panel_left + 8.0).max(24.0));
            let popup_width = px((effective_agent_panel_width - 24.0).clamp(180.0, 320.0));
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
                .left(popup_left)
                .w(popup_width)
                .max_h(px(280.0))
                .flex()
                .flex_col()
                .rounded(px(10.0))
                .bg(theme.background.opacity(elevated_ui_surface_opacity))
                .border_1()
                .border_color(theme.muted.opacity(0.16))
                .py(px(6.0))
                .overflow_hidden()
                .font_family(theme.font_family.clone());

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

        root.into_any_element()
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

fn longest_common_prefix<'a>(values: impl IntoIterator<Item = &'a str>) -> String {
    let mut iter = values.into_iter();
    let Some(first) = iter.next() else {
        return String::new();
    };
    let mut prefix = first.to_string();
    for value in iter {
        let shared = prefix
            .chars()
            .zip(value.chars())
            .take_while(|(a, b)| a == b)
            .count();
        prefix = prefix.chars().take(shared).collect();
        if prefix.is_empty() {
            break;
        }
    }
    prefix
}
