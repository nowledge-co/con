use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::Duration;
#[cfg(target_os = "macos")]
use std::time::Instant;

use gpui::{prelude::FluentBuilder as _, *};
#[cfg(target_os = "macos")]
use gpui_component::Theme;
use gpui_component::{
    ActiveTheme,
    input::{InputEvent, InputState},
    tooltip::Tooltip,
};
use serde_json::json;
use tokio::sync::oneshot;

const AGENT_PANEL_DEFAULT_WIDTH: f32 = 400.0;
const AGENT_PANEL_MIN_WIDTH: f32 = 200.0;
const TERMINAL_MIN_CONTENT_WIDTH: f32 = 360.0;
const TOP_BAR_COMPACT_HEIGHT: f32 = 28.0;
const TOP_BAR_TABS_HEIGHT: f32 = 36.0;
const CHROME_TRANSITION_SEAM_COVER: f32 = 4.0;
#[cfg(target_os = "macos")]
const CHROME_MOTION_SEAM_OVERDRAW: f32 = 6.0;
#[cfg(target_os = "macos")]
const CHROME_SNAP_GUARD_MS: u64 = 160;
#[cfg(target_os = "macos")]
const CHROME_RELEASE_COVER_MS: u64 = 48;
const MAX_SHELL_HISTORY_PER_PANE: usize = 80;
const MAX_GLOBAL_SHELL_HISTORY: usize = 240;
const MAX_GLOBAL_INPUT_HISTORY: usize = 240;

#[cfg(target_os = "macos")]
fn terminal_separator_over_backdrop(backdrop: Hsla, theme: &Theme) -> Hsla {
    let overlay_alpha = if theme.is_dark() { 0.14 } else { 0.11 };
    backdrop
        .blend(theme.foreground.opacity(overlay_alpha))
        .alpha(1.0)
}

fn perf_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("CON_GHOSTTY_PROFILE").is_some_and(|v| !v.is_empty() && v != "0")
    })
}

fn chrome_tooltip(
    label: &str,
    stroke: Option<Keystroke>,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    let label = label.to_string();
    Tooltip::element(move |_, cx| {
        let theme = cx.theme();
        let mut content = div().flex().items_center().gap(px(7.0)).child(
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

fn max_sidebar_panel_width(window_width: f32, agent_panel_outer_width: f32) -> f32 {
    (window_width - agent_panel_outer_width - TERMINAL_MIN_CONTENT_WIDTH)
        .clamp(PANEL_MIN_WIDTH, PANEL_MAX_WIDTH)
}

/// Windows / Linux caption buttons (Min / Max+Restore / Close).
///
/// Each button is marked with `.window_control_area(..)` so GPUI's
/// platform layer hit-tests it during `WM_NCHITTEST` on Windows and
/// dispatches the OS-level action automatically. The X11 backend in
/// `gpui_linux` doesn't currently dispatch through that hit-test
/// path (`on_hit_test_window_control` is a no-op there), so on Linux
/// we additionally wire explicit `start_window_move` / `zoom_window`
/// / `minimize_window` / `remove_window` calls on click. The marker
/// is still set for future-proofing once the Linux backend grows
/// `_NET_WM_MOVERESIZE`-style server hit testing.
///
/// Uses Phosphor SVGs instead of Segoe Fluent Icons so the bar
/// renders identically on hosts where Segoe Fluent Icons isn't
/// installed (Win10 without the 2022 optional feature, Linux,
/// tests). Size and hover colors mirror Windows 11's native caption
/// buttons: 36px wide, 45px min height doesn't apply here (we honour
/// the shared `top_bar_height` instead), red hover on Close.
#[cfg(any(target_os = "windows", target_os = "linux"))]
fn caption_buttons(
    window: &Window,
    theme: &gpui_component::theme::ThemeColor,
    height: f32,
    // Linux Close needs a workspace handle so it can call
    // `prepare_window_close` (cancel sessions, flush state, drop
    // pending control responses) before yanking the window — same
    // shutdown path the macOS / Windows X-button hits via
    // `on_window_should_close`. Windows has its own caption-area
    // hit-test that runs through the workspace cleanup; on Linux
    // GPUI's X11 backend doesn't fire that path so we have to
    // route it explicitly.
    #[cfg(target_os = "linux")] workspace: gpui::WeakEntity<ConWorkspace>,
) -> impl IntoElement {
    #[cfg(target_os = "linux")]
    use gpui::MouseButton;
    use gpui::{Hsla, ParentElement, Rgba, Styled, WindowControlArea, div, px, svg};

    let close_red: Hsla = Rgba {
        r: 232.0 / 255.0,
        g: 17.0 / 255.0,
        b: 32.0 / 255.0,
        a: 1.0,
    }
    .into();
    let fg = theme.muted_foreground.opacity(0.9);
    let hover_bg = theme.muted.opacity(0.12);

    let button = |id: &'static str, icon: &'static str, area: WindowControlArea, close: bool| {
        let hover = if close { close_red } else { hover_bg };
        // All three glyphs rest at the same theme-muted foreground so
        // min/max/close read as one visual row. Only on hover does the
        // close glyph switch to white, paired with the red chip bg —
        // matches Windows 11 convention. Parent div declares itself as
        // a `group(id)` so the svg's `.group_hover(id, ...)` fires when
        // the 36px hit-target is hovered, not just the 10px icon ink.
        let hover_fg = if close { gpui::white() } else { fg };
        let el = div()
            .id(id)
            .group(id)
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
            .child(
                svg()
                    .path(icon)
                    .size(px(10.0))
                    .text_color(fg)
                    .group_hover(id, move |s| s.text_color(hover_fg)),
            );

        // Linux: GPUI's X11 hit-test doesn't fire `WindowControlArea`
        // dispatchers, so wire each button to its `Window` action by
        // hand. `on_mouse_down` matches macOS / Windows feel: the
        // action fires on the click-down edge rather than after the
        // up edge, which keeps the cluster snappy. Windows already
        // dispatches via the WindowControlArea hit-test set above —
        // no extra handler needed there.
        #[cfg(target_os = "linux")]
        let workspace_for_close = workspace.clone();
        #[cfg(target_os = "linux")]
        let el = el.on_mouse_down(MouseButton::Left, move |_, window, cx| match area {
            WindowControlArea::Min => window.minimize_window(),
            WindowControlArea::Max => window.zoom_window(),
            WindowControlArea::Close => {
                // Mirror the macOS / Windows close path: run the
                // workspace cleanup (cancel agent sessions, flush
                // session save, drop pending control responses,
                // shut down terminal surfaces) *before* the window
                // goes away. Without this, clicking the Linux CSD
                // close button bypasses agent cancellation and
                // pending control-request responses entirely.
                let _ = workspace_for_close.update(cx, |workspace, cx| {
                    workspace.prepare_window_close(cx);
                });
                window.remove_window();
            }
            _ => {}
        });

        el
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
    RerunFromMessage, SelectSessionModel, SelectSessionProvider, SetAutoApprove,
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
use crate::pane_tree::{
    PaneTree, SplitDirection, SplitPlacement, SurfaceCreateOptions, SurfaceRenameEditor,
};
use crate::settings_panel::{
    self, AppearancePreview, SaveSettings, SettingsPanel, TabsOrientationChanged, ThemePreview,
};
use crate::sidebar::{
    NewSession, PANEL_MAX_WIDTH, PANEL_MIN_WIDTH, SessionEntry, SessionSidebar, SidebarCloseOthers,
    SidebarCloseTab, SidebarDuplicate, SidebarRename, SidebarReorder, SidebarSelect,
};
use crate::terminal_pane::{TerminalPane, subscribe_terminal_pane};
use con_terminal::TerminalTheme;

use crate::ghostty_view::{
    GhosttyCwdChanged, GhosttyFocusChanged, GhosttyProcessExited, GhosttySplitRequested,
    GhosttyTitleChanged, GhosttyView,
};
use crate::{
    AddWorkspaceLayoutTabs, ClearRestoredTerminalHistory, ClearTerminal, ClosePane, CloseSurface,
    CloseTab, CycleInputMode, ExportWorkspaceLayout, FocusInput, NewSurface, NewSurfaceSplitDown,
    NewSurfaceSplitRight, NewTab, NextSurface, NextTab, OpenWorkspaceLayoutWindow, PreviousSurface,
    PreviousTab, Quit, RenameSurface, SelectTab1, SelectTab2, SelectTab3, SelectTab4, SelectTab5,
    SelectTab6, SelectTab7, SelectTab8, SelectTab9, SplitDown, SplitLeft, SplitRight, SplitUp,
    ToggleAgentPanel, TogglePaneScopePicker, TogglePaneZoom, ToggleVerticalTabs,
};
use con_agent::{
    AgentConfig, Conversation, ProviderKind, TerminalExecRequest, TerminalExecResponse,
};
use con_core::config::{
    AppearanceConfig, Config, TabsOrientation, TerminalConfig, sanitize_terminal_font_family,
};
use con_core::control::{
    AgentAskResult, ControlCommand, ControlError, ControlRequestEnvelope, ControlResult,
    SystemIdentifyResult, TabInfo,
};
use con_core::harness::{AgentHarness, AgentSession, HarnessEvent, InputKind};
use con_core::session::{
    AgentModelOverrideState, AgentRoutingState, GlobalHistoryState, PaneLayoutState,
    PaneSplitDirection, Session, TabState,
};
use con_core::workspace_layout::WorkspaceLayout;
use con_core::{
    SuggestionContext, SuggestionEngine, TabIconKind, TabSummary, TabSummaryEngine,
    TabSummaryRequest,
};

struct Tab {
    pane_tree: PaneTree,
    title: String,
    /// User-supplied label that overrides every auto-derived name in
    /// the panel/strip. Set via inline rename (double-click in
    /// vertical panel) or context menu. `None` means "use smart
    /// auto-derived name".
    user_label: Option<String>,
    /// AI-suggested label, when the suggestion model is enabled and
    /// has produced one. Sits between `user_label` and the regex
    /// heuristic in the naming priority — never overrides an
    /// explicit user choice, but does override the heuristic when
    /// available.
    ai_label: Option<String>,
    /// AI-suggested icon, paired with `ai_label`. When `None`, the
    /// row falls back to the heuristic icon.
    ai_icon: Option<TabIconKind>,
    /// Stable identifier for this tab across the lifetime of the
    /// window — used as the cache key in the `TabSummaryEngine` so
    /// reorders, closes, and re-opens don't collide. Allocated from
    /// `next_tab_summary_id` at tab construction time.
    summary_id: u64,
    needs_attention: bool,
    session: AgentSession,
    agent_routing: AgentRoutingState,
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

struct SettingsWindowView {
    panel: Entity<SettingsPanel>,
}

impl Render for SettingsWindowView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.panel.clone())
    }
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
    config: Config,
    sidebar: Entity<SessionSidebar>,
    tabs: Vec<Tab>,
    active_tab: usize,
    /// True when this workspace is the singleton quick terminal,
    /// which must never be fully closed — closing the last tab
    /// should reinitialize a fresh tab and hide the window instead.
    is_quick_terminal: bool,
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
    tabs_orientation: TabsOrientation,
    agent_panel: Entity<AgentPanel>,
    input_bar: Entity<InputBar>,
    settings_panel: Entity<SettingsPanel>,
    settings_window: Option<AnyWindowHandle>,
    settings_window_panel: Option<Entity<SettingsPanel>>,
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
    /// Vertical tabs panel drag state: start X position and start width when drag began.
    sidebar_drag: Option<(f32, f32)>,
    /// Current terminal color theme
    terminal_theme: TerminalTheme,
    /// Shared Ghostty app instance for all panes in this window.
    ghostty_app: std::sync::Arc<con_ghostty::GhosttyApp>,
    /// Last wake generation observed from Ghostty's embedded runtime.
    last_ghostty_wake_generation: u64,
    #[cfg(target_os = "macos")]
    chrome_transition_underlay_until: Option<Instant>,
    #[cfg(target_os = "macos")]
    agent_panel_snap_guard_until: Option<Instant>,
    #[cfg(target_os = "macos")]
    input_bar_snap_guard_until: Option<Instant>,
    #[cfg(target_os = "macos")]
    top_chrome_snap_guard_until: Option<Instant>,
    #[cfg(target_os = "macos")]
    sidebar_snap_guard_until: Option<Instant>,
    #[cfg(target_os = "macos")]
    sidebar_snap_guard_width: f32,
    #[cfg(target_os = "macos")]
    agent_panel_release_cover_until: Option<Instant>,
    #[cfg(target_os = "macos")]
    input_bar_release_cover_until: Option<Instant>,
    #[cfg(target_os = "macos")]
    top_chrome_release_cover_until: Option<Instant>,
    #[cfg(target_os = "macos")]
    sidebar_release_cover_until: Option<Instant>,
    #[cfg(target_os = "macos")]
    sidebar_release_cover_width: f32,
    /// Pending create-pane requests that need a window context to process.
    pending_create_pane_requests: Vec<PendingCreatePane>,
    /// Pending window-aware control requests such as tab lifecycle mutations.
    pending_window_control_requests: Vec<PendingWindowControlRequest>,
    /// Pending surface-control requests that need a window context to allocate a terminal view.
    pending_surface_control_requests: Vec<PendingSurfaceControlRequest>,
    /// Inline editor for the pane-local surface rail.
    surface_rename: Option<SurfaceRenameEditor>,
    /// Control-plane requests from the external `con-cli` socket bridge.
    control_request_rx: crossbeam_channel::Receiver<ControlRequestEnvelope>,
    /// Keeps the Unix socket alive for this workspace instance.
    control_socket: Option<con_core::ControlSocketHandle>,
    /// Pending external agent requests keyed by 0-based tab index.
    pending_control_agent_requests: HashMap<usize, PendingControlAgentRequest>,
    shell_suggestion_rx: crossbeam_channel::Receiver<ShellSuggestionResult>,
    shell_suggestion_tx: crossbeam_channel::Sender<ShellSuggestionResult>,
    /// Background AI engine that produces a label + icon for each
    /// vertical-tabs row. Shares the harness's tokio runtime and the
    /// user's `agent.suggestion_model` settings.
    tab_summary_engine: TabSummaryEngine,
    tab_summary_rx: crossbeam_channel::Receiver<(u64, TabSummary)>,
    tab_summary_tx: crossbeam_channel::Sender<(u64, TabSummary)>,
    /// Bumped whenever summary-model settings change so late async
    /// responses from the old configuration are ignored.
    tab_summary_generation: u64,
    last_sidebar_pinned: bool,
    /// Monotonic counter for [`Tab::summary_id`] — stable across the
    /// window's lifetime so the summary engine's per-tab cache
    /// survives reorders and tab close/reopen.
    next_tab_summary_id: u64,
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

#[derive(Clone)]
struct ResolvedSurfaceTarget {
    terminal: TerminalPane,
    pane_index: usize,
    pane_id: usize,
    surface_index: usize,
    surface_id: usize,
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

enum PendingSurfaceControlRequest {
    Create {
        tab_idx: usize,
        pane: con_core::PaneTarget,
        title: Option<String>,
        command: Option<String>,
        owner: Option<String>,
        close_pane_when_last: bool,
        response_tx: oneshot::Sender<ControlResult>,
    },
    Split {
        tab_idx: usize,
        source: con_core::SurfaceTarget,
        location: con_agent::tools::PaneCreateLocation,
        title: Option<String>,
        command: Option<String>,
        owner: Option<String>,
        close_pane_when_last: bool,
        response_tx: oneshot::Sender<ControlResult>,
    },
    Close {
        tab_idx: usize,
        target: con_core::SurfaceTarget,
        close_empty_owned_pane: bool,
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
    restored_screen_text: Option<&[String]>,
    font_size: f32,
    window: &mut Window,
    cx: &mut Context<ConWorkspace>,
) -> TerminalPane {
    let app = app.clone();
    let cwd = cwd.filter(|cwd| !cwd.is_empty()).map(str::to_string);
    let restored_screen_text = restored_screen_text
        .map(|lines| lines.to_vec())
        .filter(|lines| !lines.is_empty());
    let view = cx.new(|cx| {
        crate::ghostty_view::GhosttyView::new(app, cwd, restored_screen_text, font_size, cx)
    });
    let pane = TerminalPane::new(view);
    subscribe_terminal_pane(&pane, window, cx);
    pane
}

fn find_git_worktree_root(start: &std::path::Path) -> Option<std::path::PathBuf> {
    start
        .ancestors()
        .find(|candidate| {
            let marker = candidate.join(".git");
            marker.is_dir() || marker.is_file()
        })
        .map(std::path::Path::to_path_buf)
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

    pub(crate) fn effective_terminal_opacity(value: f32) -> f32 {
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

    #[cfg(target_os = "macos")]
    fn terminal_adjacent_chrome_duration(_open: bool, _open_ms: u64, _close_ms: u64) -> Duration {
        // Embedded Ghostty panes are native AppKit views under GPUI. Animating
        // layout next to them can expose a one-frame clear backing seam that no
        // GPUI border can reliably hide while preserving terminal glass.
        Duration::ZERO
    }

    #[cfg(not(target_os = "macos"))]
    fn terminal_adjacent_chrome_duration(open: bool, open_ms: u64, close_ms: u64) -> Duration {
        Duration::from_millis(if open { open_ms } else { close_ms })
    }

    #[cfg(target_os = "macos")]
    fn arm_chrome_transition_underlay(&mut self, duration: Duration) {
        let until = Instant::now() + duration;
        self.chrome_transition_underlay_until = Some(
            self.chrome_transition_underlay_until
                .map_or(until, |prev| prev.max(until)),
        );
    }

    #[cfg(target_os = "macos")]
    fn extend_guard(until: &mut Option<Instant>, duration: Duration) {
        let next = Instant::now() + duration;
        *until = Some(until.map_or(next, |prev| prev.max(next)));
    }

    #[cfg(target_os = "macos")]
    fn arm_agent_panel_snap_guard(&mut self, cx: &mut App) {
        Self::extend_guard(
            &mut self.agent_panel_snap_guard_until,
            Duration::from_millis(CHROME_SNAP_GUARD_MS),
        );
        self.mark_active_tab_terminal_native_layout_pending(cx);
    }

    #[cfg(target_os = "macos")]
    fn arm_input_bar_snap_guard(&mut self, cx: &mut App) {
        Self::extend_guard(
            &mut self.input_bar_snap_guard_until,
            Duration::from_millis(CHROME_SNAP_GUARD_MS),
        );
        self.mark_active_tab_terminal_native_layout_pending(cx);
    }

    #[cfg(target_os = "macos")]
    fn arm_top_chrome_snap_guard(&mut self, cx: &mut App) {
        Self::extend_guard(
            &mut self.top_chrome_snap_guard_until,
            Duration::from_millis(CHROME_SNAP_GUARD_MS),
        );
        self.mark_active_tab_terminal_native_layout_pending(cx);
    }

    #[cfg(target_os = "macos")]
    fn arm_sidebar_snap_guard(&mut self, width: f32, cx: &mut App) {
        Self::extend_guard(
            &mut self.sidebar_snap_guard_until,
            Duration::from_millis(CHROME_SNAP_GUARD_MS),
        );
        self.sidebar_snap_guard_width = self.sidebar_snap_guard_width.max(width.max(0.0));
        self.mark_active_tab_terminal_native_layout_pending(cx);
    }

    #[cfg(target_os = "macos")]
    fn snap_guard_active(until: &mut Option<Instant>, window: &mut Window) -> bool {
        Self::snap_guard_state(until, window).0
    }

    #[cfg(target_os = "macos")]
    fn snap_guard_state(until: &mut Option<Instant>, window: &mut Window) -> (bool, bool) {
        let Some(deadline) = *until else {
            return (false, false);
        };

        if Instant::now() >= deadline {
            *until = None;
            (false, true)
        } else {
            window.request_animation_frame();
            (true, false)
        }
    }

    #[cfg(target_os = "macos")]
    fn sync_chrome_transition_underlay(&self, visible: bool, cx: &App) {
        if visible {
            if self.has_active_tab() {
                for terminal in self.tabs[self.active_tab].pane_tree.all_terminals() {
                    terminal.set_native_transition_underlay_visible(true, cx);
                }
            }
            return;
        }

        for tab in &self.tabs {
            for terminal in tab.pane_tree.all_terminals() {
                terminal.set_native_transition_underlay_visible(false, cx);
            }
        }
    }

    pub fn from_session(
        config: Config,
        session: Session,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let initial_vertical_pinned = session.vertical_tabs_pinned;
        let agent_panel_open = session.agent_panel_open;
        let agent_panel_width = session
            .agent_panel_width
            .unwrap_or(AGENT_PANEL_DEFAULT_WIDTH);
        let initial_agent_outer_width = if agent_panel_open {
            agent_panel_width.min(max_agent_panel_width(window.bounds().size.width.as_f32())) + 1.0
        } else {
            0.0
        };
        let initial_sidebar_max_width = max_sidebar_panel_width(
            window.bounds().size.width.as_f32(),
            initial_agent_outer_width,
        );
        let initial_vertical_width = session
            .vertical_tabs_width
            .map(|width| SessionSidebar::clamped_panel_width(width, initial_sidebar_max_width));
        let sidebar = cx.new(|cx| {
            let mut s = SessionSidebar::new(cx);
            if let Some(width) = initial_vertical_width {
                s.set_panel_width(width, cx);
            }
            s.set_pinned(initial_vertical_pinned, cx);
            s
        });
        let terminal_font_family = sanitize_terminal_font_family(&config.terminal.font_family);
        let ui_font_family = config.appearance.ui_font_family.clone();
        let ui_font_size = config.appearance.ui_font_size;
        let font_size = config.terminal.font_size;
        let terminal_cursor_style = config.terminal.cursor_style.clone();
        let terminal_opacity = Self::effective_terminal_opacity(config.appearance.terminal_opacity);
        let terminal_blur = Self::effective_terminal_blur(config.appearance.terminal_blur);
        let ui_opacity = Self::clamp_ui_opacity(config.appearance.ui_opacity);
        let effective_ui_opacity = Self::effective_ui_opacity(ui_opacity);
        sidebar.update(cx, |s, cx| s.set_ui_opacity(effective_ui_opacity, cx));
        let background_image = config.appearance.background_image.clone();
        let background_image_opacity =
            Self::clamp_background_image_opacity(config.appearance.background_image_opacity);
        let background_image_position = config.appearance.background_image_position.clone();
        let background_image_fit = config.appearance.background_image_fit.clone();
        let background_image_repeat = config.appearance.background_image_repeat;
        let tabs_orientation = config.appearance.tabs_orientation;
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
        let tab_summary_engine = harness.tab_summary_engine();
        let session_save_tx = spawn_session_save_worker();
        let (control_request_tx, control_request_rx) = crossbeam_channel::unbounded();
        let (shell_suggestion_tx, shell_suggestion_rx) = crossbeam_channel::unbounded();
        let (tab_summary_tx, tab_summary_rx) = crossbeam_channel::unbounded();
        let control_socket = if crate::control_socket_started() {
            None
        } else {
            match con_core::spawn_control_socket_server(
                harness.runtime_handle(),
                control_request_tx,
            ) {
                Ok(handle) => {
                    crate::mark_control_socket_started();
                    Some(handle)
                }
                Err(err) => {
                    log::error!("Failed to start con control socket: {}", err);
                    None
                }
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

        let restore_terminal_text = config.appearance.restore_terminal_text;
        let make_terminal = |cwd: Option<&str>,
                             restored_screen_text: Option<&[String]>,
                             force_restored_screen_text: bool,
                             window: &mut Window,
                             cx: &mut Context<Self>|
         -> TerminalPane {
            let restored_screen_text = if restore_terminal_text || force_restored_screen_text {
                restored_screen_text
            } else {
                None
            };
            make_ghostty_terminal(
                &ghostty_app,
                cwd,
                restored_screen_text,
                font_size,
                window,
                cx,
            )
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
                        |restore_cwd: Option<&str>,
                         restored_screen_text: Option<&[String]>,
                         force_restored_screen_text: bool| {
                            make_terminal(
                                restore_cwd,
                                restored_screen_text,
                                force_restored_screen_text,
                                window,
                                cx,
                            )
                        };
                    PaneTree::from_state(layout, tab_state.focused_pane_id, &mut restore_terminal)
                } else {
                    let cwd = tab_state.cwd.as_deref();
                    PaneTree::new(make_terminal(cwd, None, false, window, cx))
                };
                Tab {
                    pane_tree,
                    title: if tab_state.title.is_empty() {
                        format!("Terminal {}", i + 1)
                    } else {
                        tab_state.title.clone()
                    },
                    user_label: tab_state.user_label.clone(),
                    ai_label: None,
                    ai_icon: None,
                    summary_id: i as u64,
                    needs_attention: false,
                    session: agent_session,
                    agent_routing: if tab_state.agent_routing.is_empty() {
                        Self::default_agent_routing(&config.agent)
                    } else {
                        tab_state.agent_routing.clone()
                    },
                    panel_state,
                    runtime_trackers: RefCell::new(HashMap::new()),
                    runtime_cache: RefCell::new(HashMap::new()),
                    shell_history: Self::restore_shell_history(tab_state),
                }
            })
            .collect();
        if tabs.is_empty() {
            let terminal = make_terminal(None, None, false, window, cx);
            tabs.push(Tab {
                pane_tree: PaneTree::new(terminal),
                title: "Terminal".to_string(),
                user_label: None,
                ai_label: None,
                ai_icon: None,
                summary_id: 0,
                needs_attention: false,
                session: AgentSession::new(),
                agent_routing: Self::default_agent_routing(&config.agent),
                panel_state: PanelState::new(),
                runtime_trackers: RefCell::new(HashMap::new()),
                runtime_cache: RefCell::new(HashMap::new()),
                shell_history: HashMap::new(),
            });
        }
        let active_tab = session.active_tab.min(tabs.len() - 1);
        // Seed the vertical-tabs side panel with the restored tab list
        // so it has something to render before the first
        // `sync_sidebar` call (which only fires when the live terminal
        // title or the tab set changes). Without this seed the panel
        // would draw an empty rail until the user opened or activated
        // a tab.
        {
            let entries: Vec<SessionEntry> = tabs
                .iter()
                .enumerate()
                .map(|(i, tab)| {
                    let presentation = smart_tab_presentation(
                        tab.user_label.as_deref(),
                        tab.ai_label.as_deref(),
                        tab.ai_icon.map(|k| k.svg_path()),
                        None,
                        Some(tab.title.as_str()),
                        None,
                        i,
                    );
                    let pane_count = tab.pane_tree.pane_terminals().len();
                    SessionEntry {
                        id: tab.summary_id,
                        name: presentation.name,
                        subtitle: presentation.subtitle,
                        is_ssh: presentation.is_ssh,
                        needs_attention: false,
                        icon: presentation.icon,
                        has_user_label: tab.user_label.is_some(),
                        pane_count,
                    }
                })
                .collect();
            sidebar.update(cx, |s, cx| {
                s.sync_sessions(entries, active_tab, cx);
            });
        }
        let persisted_history = GlobalHistoryState::load().unwrap_or_else(|err| {
            log::warn!("Failed to load command history: {}", err);
            GlobalHistoryState::default()
        });
        let global_shell_history = Self::restore_global_shell_history(&session, &tabs);
        let global_shell_history =
            Self::merge_shell_histories(global_shell_history, &persisted_history);
        let global_input_history =
            Self::restore_global_input_history(&session, &persisted_history, &global_shell_history);
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
        sidebar.update(cx, |s, cx| s.set_ui_opacity(effective_ui_opacity, cx));
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
        cx.subscribe_in(&settings_panel, window, Self::on_tabs_orientation_changed)
            .detach();
        cx.subscribe_in(&settings_panel, window, Self::on_theme_preview)
            .detach();
        cx.subscribe_in(&settings_panel, window, Self::on_appearance_preview)
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
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_close_tab)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_rename)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_duplicate)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_reorder)
            .detach();
        cx.subscribe_in(&sidebar, window, Self::on_sidebar_close_others)
            .detach();
        cx.observe(&sidebar, |this, sidebar, cx| {
            // The sidebar also notifies for transient hover and drag
            // affordances. Re-render for all sidebar changes, but only
            // persist when the actual pin state changes.
            let pinned = sidebar.read(cx).is_pinned();
            if this.last_sidebar_pinned != pinned {
                this.last_sidebar_pinned = pinned;
                this.save_session(cx);
            }
            cx.notify();
        })
        .detach();
        let workspace_handle = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            // Two shutdown paths, two behaviours:
            //
            // macOS — run prepare + return true so NSApp destroys the
            // window. Background tasks riding the main runloop finish
            // naturally; NSApp keeps the process alive without windows
            // per platform convention, so no explicit quit is needed.
            //
            // Windows — returning true tears the HWND down inside the
            // same WM_CLOSE iteration that fired this callback. That
            // races with pending async window tasks (e.g.
            // `handle_activate_msg::async_block$0`) which then run on
            // a dead HWND and log "Invalid window handle" / "window
            // not found". Defer the cleanup so in-flight tasks drain
            // first, then remove the window and quit (closing the
            // last window does NOT auto-terminate the process on
            // Windows). This mirrors `close_window_from_last_tab`,
            // which the user confirmed shuts down cleanly from the
            // pane-exit path.
            #[cfg(target_os = "windows")]
            {
                // `cx.defer_in` delays by a single event-loop tick, which
                // isn't enough: GPUI's `handle_activate_msg` spawns an
                // async task on the same executor when WM_ACTIVATE fires
                // during close, and those tasks outlive one tick — they
                // then run `window.update()` on a freshly-destroyed HWND
                // and log `window not found` with a backtrace. Spawn a
                // small timer instead so every in-flight window task
                // has time to drain before we tear the HWND down.
                //
                // The 120ms timer is a probability reduction, not a
                // guarantee: Windows can still deliver `WM_ACTIVATE` /
                // `WM_PAINT` to a closing HWND and surface the same log
                // error via GPUI's `async_context::update_window`. That
                // residual noise is benign — `prepare_window_close`
                // runs, sessions save, surfaces shut down, and the
                // process exits cleanly. See
                // `postmortem/2026-04-21-windows-x-close-log-noise.md`
                // for the full analysis and the upstream GPUI fix that
                // would eliminate the noise.
                //
                // `cx` inside this callback is `&mut App`, which has
                // `spawn` (not `spawn_in`), so reach the window via its
                // handle inside the spawned task.
                let handle = workspace_handle.clone();
                let window_handle = window.window_handle();
                cx.spawn(async move |cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(120))
                        .await;
                    let _ = window_handle.update(cx, |_, window, cx| {
                        let _ = handle.update(cx, |workspace, cx| {
                            workspace.prepare_window_close(cx);
                        });
                        let should_quit = cx.windows().len() <= 1;
                        window.remove_window();
                        if should_quit {
                            cx.quit();
                        }
                    });
                })
                .detach();
                return false;
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = window;
                let _ = workspace_handle.update(cx, |workspace, cx| {
                    workspace.prepare_window_close(cx);
                });
                true
            }
        });

        // Poll all tabs' agent sessions.
        cx.spawn(async move |this, cx| {
            // Backstop interval for the AI-summary trigger below.
            // See the comment on `last_summary_poll` use site.
            let summary_poll_interval = std::time::Duration::from_secs(3);
            let mut last_summary_poll = std::time::Instant::now();
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

                    while let Ok((generation, summary)) = workspace.tab_summary_rx.try_recv() {
                        got_event = true;
                        workspace.apply_tab_summary(generation, summary, cx);
                    }

                    if workspace.pump_ghostty_views(cx) {
                        got_event = true;
                        // Output flowed — ask the AI summarizer to
                        // re-check. The engine's per-tab 5 s budget
                        // and context-hash dedupe keep this from
                        // firing more than once per real change.
                        workspace.request_tab_summaries(cx);
                        cx.notify();
                    } else if last_summary_poll.elapsed() >= summary_poll_interval {
                        // Backstop for the pump-driven trigger
                        // above. `pump_ghostty_views` only fires
                        // while output is actively streaming, so a
                        // tab whose context drifted while it sat
                        // idle (user navigated away and back)
                        // would never re-summarize. The engine's
                        // per-tab cache + 5 s success budget keep
                        // repeated calls cheap.
                        workspace.request_tab_summaries(cx);
                        last_summary_poll = std::time::Instant::now();
                    }
                })
                .ok();

                if !got_event {
                    // Idle tick. The previous 16ms cap meant a PTY
                    // chunk arriving 1ms after the loop entered the
                    // sleep had to wait the full 15ms before any
                    // GPUI repaint was even scheduled — visible as
                    // a stall between "user hits Enter on htop" and
                    // "htop's alt-screen actually paints" on Linux,
                    // because Linux drives the renderer through this
                    // loop instead of through libghostty's own
                    // NSView pump (macOS) or D3D11 swapchain
                    // (Windows). 8ms keeps the work bounded —
                    // `pump_ghostty_views` short-circuits on
                    // unchanged `wake_generation` — while halving
                    // the worst-case PTY-to-frame latency. Refresh
                    // is still capped at the GPUI vsync rate
                    // (typically 60 Hz) so this doesn't actually
                    // double the paint work.
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(8))
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
                for terminal in tab.pane_tree.all_surface_terminals() {
                    terminal.set_native_view_visible(false, cx);
                }
            }
        }

        let has_multiple_tabs = tabs.len() > 1;
        let last_ghostty_wake_generation = ghostty_app.wake_generation();
        let next_tab_summary_id_init = tabs.len() as u64;

        // Ask the AI summarizer for initial labels — many tabs will
        // be SSH (instant short-circuit) and the rest will arrive
        // when the shell starts producing output anyway, but kicking
        // it off here lets non-shell signals (cwd from session
        // restore) feed the model immediately.
        if config.agent.suggestion_model.enabled
            && matches!(
                config.appearance.tabs_orientation,
                TabsOrientation::Vertical
            )
        {
            let tx = tab_summary_tx.clone();
            for (i, tab) in tabs.iter_mut().enumerate() {
                let terminal = tab.pane_tree.focused_terminal();
                let cwd = terminal.current_dir(cx).or_else(|| {
                    session
                        .tabs
                        .get(i)
                        .and_then(|tab_state| tab_state.cwd.clone())
                });
                let title = Some(tab.title.clone()).filter(|t| !t.is_empty());
                let pane_id = tab
                    .pane_tree
                    .pane_id_for_terminal(terminal)
                    .unwrap_or(usize::MAX);
                let observation = terminal.observation_frame(12, cx);
                let runtime = {
                    let mut trackers = tab.runtime_trackers.borrow_mut();
                    trackers.entry(pane_id).or_default().observe(observation)
                };
                tab.runtime_cache
                    .borrow_mut()
                    .insert(pane_id, runtime.clone());
                let req = TabSummaryRequest {
                    tab_id: tab.summary_id,
                    cwd,
                    title,
                    ssh_host: runtime.remote_host,
                    recent_commands: vec![],
                    recent_output: vec![],
                };
                let tx = tx.clone();
                tab_summary_engine.request(req, move |summary| {
                    let _ = tx.send((0, summary));
                });
            }
        }

        Self {
            config: config.clone(),
            sidebar,
            tabs,
            active_tab,
            is_quick_terminal: false,
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
            tabs_orientation,
            agent_panel,
            input_bar,
            settings_panel,
            settings_window: None,
            settings_window_panel: None,
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
            sidebar_drag: None,
            terminal_theme,
            ghostty_app,
            last_ghostty_wake_generation,
            #[cfg(target_os = "macos")]
            chrome_transition_underlay_until: None,
            #[cfg(target_os = "macos")]
            agent_panel_snap_guard_until: None,
            #[cfg(target_os = "macos")]
            input_bar_snap_guard_until: None,
            #[cfg(target_os = "macos")]
            top_chrome_snap_guard_until: None,
            #[cfg(target_os = "macos")]
            sidebar_snap_guard_until: None,
            #[cfg(target_os = "macos")]
            sidebar_snap_guard_width: 0.0,
            #[cfg(target_os = "macos")]
            agent_panel_release_cover_until: None,
            #[cfg(target_os = "macos")]
            input_bar_release_cover_until: None,
            #[cfg(target_os = "macos")]
            top_chrome_release_cover_until: None,
            #[cfg(target_os = "macos")]
            sidebar_release_cover_until: None,
            #[cfg(target_os = "macos")]
            sidebar_release_cover_width: 0.0,
            pending_create_pane_requests: Vec::new(),
            pending_window_control_requests: Vec::new(),
            pending_surface_control_requests: Vec::new(),
            surface_rename: None,
            control_request_rx,
            control_socket,
            pending_control_agent_requests: HashMap::new(),
            shell_suggestion_rx,
            shell_suggestion_tx,
            tab_summary_engine,
            tab_summary_rx,
            tab_summary_tx,
            tab_summary_generation: 0,
            last_sidebar_pinned: initial_vertical_pinned,
            next_tab_summary_id: next_tab_summary_id_init,
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
        make_ghostty_terminal(&self.ghostty_app, cwd, None, self.font_size, window, cx)
    }

    fn horizontal_tabs_visible(&self) -> bool {
        matches!(self.tabs_orientation, TabsOrientation::Horizontal) && self.tabs.len() > 1
    }

    fn vertical_tabs_active(&self) -> bool {
        matches!(self.tabs_orientation, TabsOrientation::Vertical)
    }

    fn sync_tab_strip_motion(&mut self) -> bool {
        let target = if self.horizontal_tabs_visible() {
            1.0
        } else {
            0.0
        };
        let changed = (self.tab_strip_motion.current() - target).abs() > 0.001;
        self.tab_strip_motion.set_target(
            target,
            Self::terminal_adjacent_chrome_duration(target > 0.5, 180, 180),
        );
        changed
    }

    fn current_top_bar_height(&self) -> f32 {
        if self.tab_strip_motion.is_animating() || self.horizontal_tabs_visible() {
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
        let surface_terminals = pane_tree.surface_terminals();
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
        let all_ids = surface_terminals
            .iter()
            .map(|(pane_id, _, _)| *pane_id)
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

        for (pane_id, is_active_surface, terminal) in surface_terminals {
            terminal.set_focus_state(is_active_surface && target_ids.contains(&pane_id), cx);
        }
    }

    fn sync_tab_native_view_visibility(&self, tab_index: usize, visible: bool, cx: &App) {
        let Some(tab) = self.tabs.get(tab_index) else {
            return;
        };
        let zoomed_pane_id = tab.pane_tree.zoomed_pane_id();

        for surface in tab.pane_tree.surface_infos(None) {
            let pane_visible =
                visible && zoomed_pane_id.is_none_or(|zoomed| zoomed == surface.pane_id);
            surface
                .terminal
                .set_native_view_visible(pane_visible && surface.is_active, cx);
        }
    }

    fn sync_active_tab_native_view_visibility(&self, cx: &App) {
        if self.has_active_tab() {
            self.sync_tab_native_view_visibility(self.active_tab, true, cx);
        }
    }

    fn hide_active_tab_native_views_not_in_layout(&self, cx: &App) {
        if !self.has_active_tab() {
            return;
        }
        let tab = &self.tabs[self.active_tab];
        let Some(zoomed_pane_id) = tab.pane_tree.zoomed_pane_id() else {
            return;
        };

        for surface in tab.pane_tree.surface_infos(None) {
            if surface.pane_id != zoomed_pane_id || !surface.is_active {
                surface.terminal.set_native_view_visible(false, cx);
            }
        }
    }

    fn notify_tab_terminal_views(&self, tab_index: usize, cx: &mut App) {
        let Some(tab) = self.tabs.get(tab_index) else {
            return;
        };
        for terminal in tab.pane_tree.all_terminals() {
            terminal.notify(cx);
        }
    }

    fn notify_active_tab_terminal_views(&self, cx: &mut App) {
        if self.has_active_tab() {
            self.notify_tab_terminal_views(self.active_tab, cx);
        }
    }

    #[cfg(target_os = "macos")]
    fn mark_tab_terminal_native_layout_pending(&self, tab_index: usize, cx: &mut App) {
        let Some(tab) = self.tabs.get(tab_index) else {
            return;
        };
        for terminal in tab.pane_tree.all_terminals() {
            terminal.mark_native_layout_pending(cx);
        }
    }

    #[cfg(target_os = "macos")]
    fn mark_active_tab_terminal_native_layout_pending(&self, cx: &mut App) {
        if self.has_active_tab() {
            self.mark_tab_terminal_native_layout_pending(self.active_tab, cx);
        }
    }

    fn sync_active_tab_native_view_visibility_after_layout(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Native Ghostty NSViews live outside GPUI's element tree. Hide panes
        // that are definitely leaving the layout immediately, but delay
        // revealing newly-visible panes until GPUI has committed one layout
        // frame so they do not flash at stale split coordinates.
        self.hide_active_tab_native_views_not_in_layout(cx);
        cx.on_next_frame(window, |_workspace, window, cx| {
            cx.notify();
            cx.on_next_frame(window, |workspace, _window, cx| {
                if workspace.has_active_tab()
                    && !workspace.ghostty_hidden
                    && !workspace.is_modal_open(cx)
                {
                    workspace.sync_active_tab_native_view_visibility(cx);
                }
            });
        });
    }

    fn sync_active_tab_native_view_visibility_after_zoom_layout(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Zoom/unzoom should not hide anything before GPUI has produced the
        // new pane frames. Keeping the old visual state for one frame is less
        // visible than flashing the matte/fallback background.
        cx.on_next_frame(window, |workspace, _window, cx| {
            if workspace.has_active_tab()
                && !workspace.ghostty_hidden
                && !workspace.is_modal_open(cx)
            {
                workspace.sync_active_tab_native_view_visibility(cx);
            }
        });
    }

    fn sync_active_tab_native_view_visibility_now_or_after_layout(
        &self,
        was_zoomed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if was_zoomed
            || self
                .tabs
                .get(self.active_tab)
                .and_then(|tab| tab.pane_tree.zoomed_pane_id())
                .is_some()
        {
            self.sync_active_tab_native_view_visibility_after_zoom_layout(window, cx);
            return;
        }
        self.sync_active_tab_native_view_visibility_after_layout(window, cx);
    }

    fn pump_ghostty_views(&mut self, cx: &mut Context<Self>) -> bool {
        let started = perf_trace_enabled().then(std::time::Instant::now);
        let mut changed = false;
        let mut terminal_count = 0usize;
        let mut drain_count = 0usize;

        for tab in &self.tabs {
            for terminal in tab.pane_tree.all_surface_terminals() {
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
        for (tab_index, tab) in self.tabs.iter().enumerate() {
            let sync_native_scroll = tab_index == self.active_tab;
            for terminal in tab.pane_tree.all_surface_terminals() {
                drain_count += 1;
                changed |= terminal.drain_surface_state_with_native_scroll(sync_native_scroll, cx);
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
                            let terminal_entity_id = terminal.entity_id();
                            let terminal_still_owned = _workspace.tabs.iter().any(|tab| {
                                tab.pane_tree
                                    .all_surface_terminals()
                                    .iter()
                                    .any(|candidate| candidate.entity_id() == terminal_entity_id)
                            });
                            if !terminal_still_owned {
                                terminal.set_focus_state(false, cx);
                                return true;
                            }

                            terminal.ensure_surface(window, cx);
                            terminal.notify(cx);
                            _workspace.sync_active_tab_native_view_visibility(cx);
                            if should_focus {
                                let still_active_terminal = _workspace
                                    .tabs
                                    .get(_workspace.active_tab)
                                    .is_some_and(|tab| {
                                        tab.pane_tree.all_terminals().iter().any(|candidate| {
                                            candidate.entity_id() == terminal_entity_id
                                        })
                                    });
                                if still_active_terminal {
                                    _workspace.sync_active_terminal_focus_states(cx);
                                    terminal.focus(window, cx);
                                } else {
                                    terminal.set_focus_state(false, cx);
                                }
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
                        workspace.flush_pending_surface_control_requests(window, cx);
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

    fn schedule_active_terminal_focus(&self, cx: &mut Context<Self>) {
        let window_handle = self.window_handle;
        let workspace_handle = self.workspace_handle.clone();
        cx.defer(move |cx| {
            let result = window_handle.update(cx, |_root, window, cx| {
                if let Some(workspace) = workspace_handle.upgrade() {
                    let _ = workspace.update(cx, |workspace, cx| {
                        if !workspace.has_active_tab() {
                            return;
                        }
                        let terminal = workspace.tabs[workspace.active_tab]
                            .pane_tree
                            .focused_terminal()
                            .clone();
                        terminal.ensure_surface(window, cx);
                        workspace.sync_active_tab_native_view_visibility(cx);
                        terminal.focus(window, cx);
                        workspace.sync_active_terminal_focus_states(cx);
                        cx.notify();
                    });
                }
            });
            if let Err(err) = result {
                log::warn!(
                    "[control] failed to focus active terminal in a window-aware context: {err}"
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
            let was_zoomed = self.tabs[req.tab_idx].pane_tree.zoomed_pane_id().is_some();
            self.tabs[req.tab_idx]
                .pane_tree
                .split(direction, terminal.clone());
            let should_focus = req.tab_idx == self.active_tab;
            if should_focus {
                #[cfg(target_os = "macos")]
                self.mark_tab_terminal_native_layout_pending(req.tab_idx, cx);
                self.notify_tab_terminal_views(req.tab_idx, cx);
                terminal.focus(window, cx);
                self.sync_active_terminal_focus_states(cx);
                self.sync_active_tab_native_view_visibility_now_or_after_layout(
                    was_zoomed, window, cx,
                );
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
        window.refresh();
    }

    fn flush_pending_surface_control_requests(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pending = std::mem::take(&mut self.pending_surface_control_requests);
        if pending.is_empty() {
            return;
        }

        for request in pending {
            match request {
                PendingSurfaceControlRequest::Create {
                    tab_idx,
                    pane,
                    title,
                    command,
                    owner,
                    close_pane_when_last,
                    response_tx,
                } => {
                    if tab_idx >= self.tabs.len() {
                        Self::send_control_result(
                            response_tx,
                            Err(ControlError::invalid_params(format!(
                                "Tab {} is no longer available.",
                                tab_idx + 1
                            ))),
                        );
                        continue;
                    }
                    let resolved = match self
                        .resolve_pane_target_for_tab(tab_idx, Self::pane_selector_from_target(pane))
                    {
                        Ok(resolved) => resolved,
                        Err(err) => {
                            Self::send_control_result(
                                response_tx,
                                Err(ControlError::invalid_params(err)),
                            );
                            continue;
                        }
                    };
                    let cwd = resolved.pane.current_dir(cx);
                    let terminal = self.create_terminal(cwd.as_deref(), window, cx);
                    let options = SurfaceCreateOptions {
                        title,
                        owner,
                        close_pane_when_last,
                    };
                    let Some(surface_id) = self.tabs[tab_idx].pane_tree.create_surface_in_pane(
                        resolved.pane_id,
                        terminal.clone(),
                        options,
                    ) else {
                        Self::send_control_result(
                            response_tx,
                            Err(ControlError::invalid_params(format!(
                                "Pane id {} is no longer available in tab {}.",
                                resolved.pane_id,
                                tab_idx + 1
                            ))),
                        );
                        continue;
                    };
                    if tab_idx == self.active_tab {
                        #[cfg(target_os = "macos")]
                        self.mark_tab_terminal_native_layout_pending(tab_idx, cx);
                        self.notify_tab_terminal_views(tab_idx, cx);
                        self.sync_active_tab_native_view_visibility_now_or_after_layout(
                            false, window, cx,
                        );
                    }
                    self.finish_created_surface(
                        tab_idx,
                        resolved.pane_id,
                        surface_id,
                        terminal,
                        command,
                        false,
                        response_tx,
                        window,
                        cx,
                    );
                }
                PendingSurfaceControlRequest::Split {
                    tab_idx,
                    source,
                    location,
                    title,
                    command,
                    owner,
                    close_pane_when_last,
                    response_tx,
                } => {
                    if tab_idx >= self.tabs.len() {
                        Self::send_control_result(
                            response_tx,
                            Err(ControlError::invalid_params(format!(
                                "Tab {} is no longer available.",
                                tab_idx + 1
                            ))),
                        );
                        continue;
                    }
                    let resolved = match self.resolve_surface_target_for_tab(tab_idx, source) {
                        Ok(resolved) => resolved,
                        Err(err) => {
                            Self::send_control_result(response_tx, Err(err));
                            continue;
                        }
                    };
                    let cwd = resolved.terminal.current_dir(cx);
                    let terminal = self.create_terminal(cwd.as_deref(), window, cx);
                    let direction = match location {
                        con_agent::tools::PaneCreateLocation::Right => SplitDirection::Horizontal,
                        con_agent::tools::PaneCreateLocation::Down => SplitDirection::Vertical,
                    };
                    let was_zoomed = self.tabs[tab_idx].pane_tree.zoomed_pane_id().is_some();
                    let options = SurfaceCreateOptions {
                        title,
                        owner: Some(owner.unwrap_or_else(|| "con-cli".to_string())),
                        close_pane_when_last,
                    };
                    let Some((pane_id, surface_id)) = self.tabs[tab_idx]
                        .pane_tree
                        .split_pane_with_surface_options(
                            resolved.pane_id,
                            direction,
                            SplitPlacement::After,
                            terminal.clone(),
                            options,
                        )
                    else {
                        Self::send_control_result(
                            response_tx,
                            Err(ControlError::invalid_params(format!(
                                "{} is no longer available in tab {}.",
                                source.describe(),
                                tab_idx + 1
                            ))),
                        );
                        continue;
                    };
                    if tab_idx == self.active_tab {
                        #[cfg(target_os = "macos")]
                        self.mark_tab_terminal_native_layout_pending(tab_idx, cx);
                        self.notify_tab_terminal_views(tab_idx, cx);
                        self.sync_active_tab_native_view_visibility_now_or_after_layout(
                            was_zoomed, window, cx,
                        );
                    }
                    self.finish_created_surface(
                        tab_idx,
                        pane_id,
                        surface_id,
                        terminal,
                        command,
                        true,
                        response_tx,
                        window,
                        cx,
                    );
                }
                PendingSurfaceControlRequest::Close {
                    tab_idx,
                    target,
                    close_empty_owned_pane,
                    response_tx,
                } => {
                    if tab_idx >= self.tabs.len() {
                        Self::send_control_result(
                            response_tx,
                            Err(ControlError::invalid_params(format!(
                                "Tab {} is no longer available.",
                                tab_idx + 1
                            ))),
                        );
                        continue;
                    }
                    let resolved = match self.resolve_surface_target_for_tab(tab_idx, target) {
                        Ok(resolved) => resolved,
                        Err(err) => {
                            Self::send_control_result(response_tx, Err(err));
                            continue;
                        }
                    };
                    let surfaces = self.tabs[tab_idx]
                        .pane_tree
                        .surface_infos(Some(resolved.pane_id));
                    let current = surfaces
                        .iter()
                        .find(|surface| surface.surface_id == resolved.surface_id)
                        .cloned();
                    let closing_was_focused = current
                        .as_ref()
                        .is_some_and(|surface| surface.is_active && surface.is_focused_pane);
                    let Some(close_outcome) = self.tabs[tab_idx]
                        .pane_tree
                        .close_surface(resolved.surface_id, close_empty_owned_pane)
                    else {
                        Self::send_control_result(
                            response_tx,
                            Err(ControlError::invalid_params(
                                "Refusing to close the last surface in a pane unless it is an owned ephemeral pane and close_empty_owned_pane=true.",
                            )),
                        );
                        continue;
                    };
                    let closing = close_outcome.terminal.clone();
                    closing.set_focus_state(false, cx);
                    closing.set_native_view_visible(false, cx);
                    closing.shutdown_surface(cx);
                    if tab_idx == self.active_tab {
                        if close_outcome.closed_pane {
                            #[cfg(target_os = "macos")]
                            self.mark_active_tab_terminal_native_layout_pending(cx);
                            for terminal in self.tabs[tab_idx]
                                .pane_tree
                                .all_terminals()
                                .into_iter()
                                .cloned()
                                .collect::<Vec<_>>()
                            {
                                terminal.ensure_surface(window, cx);
                                terminal.notify(cx);
                            }
                        }
                        self.sync_active_tab_native_view_visibility(cx);
                        if closing_was_focused || close_outcome.closed_pane {
                            let replacement =
                                self.tabs[tab_idx].pane_tree.focused_terminal().clone();
                            replacement.ensure_surface(window, cx);
                            replacement.focus(window, cx);
                        }
                        self.sync_active_terminal_focus_states(cx);
                    }
                    if close_outcome.closed_pane {
                        self.reconcile_runtime_trackers_for_tab(tab_idx);
                    }
                    self.save_session(cx);
                    Self::send_control_result(
                        response_tx,
                        Ok(json!({
                            "status": "closed",
                            "closed_pane": close_outcome.closed_pane,
                            "tab_index": tab_idx + 1,
                            "pane_id": close_outcome.pane_id,
                            "surface_id": resolved.surface_id,
                        })),
                    );
                }
            }
        }

        cx.notify();
        window.refresh();
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_created_surface(
        &mut self,
        tab_idx: usize,
        pane_id: usize,
        surface_id: usize,
        terminal: TerminalPane,
        command: Option<String>,
        created_pane: bool,
        response_tx: oneshot::Sender<ControlResult>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab_is_active = tab_idx == self.active_tab;
        if tab_is_active {
            terminal.focus(window, cx);
            self.sync_active_terminal_focus_states(cx);
            self.sync_active_tab_native_view_visibility(cx);
        } else {
            terminal.set_focus_state(false, cx);
            terminal.set_native_view_visible(false, cx);
        }
        Self::schedule_terminal_bootstrap_reassert(
            &terminal,
            tab_is_active,
            self.window_handle,
            self.workspace_handle.clone(),
            cx,
        );
        if let Some(cmd) = &command {
            terminal.write(format!("{cmd}\n").as_bytes(), cx);
        }
        self.record_runtime_event_for_terminal(
            tab_idx,
            &terminal,
            con_agent::context::PaneRuntimeEvent::PaneCreated {
                startup_command: command.clone(),
            },
        );
        let result =
            self.surface_created_result(tab_idx, pane_id, surface_id, created_pane, &terminal, cx);
        Self::send_control_result(response_tx, Ok(result));
        self.save_session(cx);
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

    fn resolve_surface_target_for_tab(
        &self,
        tab_idx: usize,
        target: con_core::SurfaceTarget,
    ) -> Result<ResolvedSurfaceTarget, ControlError> {
        let pane_tree = &self.tabs[tab_idx].pane_tree;
        let surfaces = pane_tree.surface_infos(None);
        if let Some(surface_id) = target.surface_id {
            let surface = surfaces
                .into_iter()
                .find(|surface| surface.surface_id == surface_id)
                .ok_or_else(|| {
                    ControlError::invalid_params(format!(
                        "Surface id {} is no longer available in tab {}.",
                        surface_id,
                        tab_idx + 1
                    ))
                })?;
            if let Some(pane_id) = target.pane_id
                && pane_id != surface.pane_id
            {
                return Err(ControlError::invalid_params(format!(
                    "Surface id {} belongs to pane id {}, not pane id {}.",
                    surface_id, surface.pane_id, pane_id
                )));
            }
            if let Some(pane_index) = target.pane_index
                && pane_index != surface.pane_index
            {
                return Err(ControlError::invalid_params(format!(
                    "Surface id {} belongs to pane {}, not pane {}.",
                    surface_id, surface.pane_index, pane_index
                )));
            }
            return Ok(ResolvedSurfaceTarget {
                terminal: surface.terminal,
                pane_index: surface.pane_index,
                pane_id: surface.pane_id,
                surface_index: surface.surface_index,
                surface_id,
            });
        }

        let resolved_pane = self
            .resolve_pane_target_for_tab(
                tab_idx,
                Self::pane_selector_from_target(target.pane_target()),
            )
            .map_err(ControlError::invalid_params)?;
        let surface_id = pane_tree
            .active_surface_id_for_pane(resolved_pane.pane_id)
            .ok_or_else(|| {
                ControlError::invalid_params(format!(
                    "Pane id {} has no active surface in tab {}.",
                    resolved_pane.pane_id,
                    tab_idx + 1
                ))
            })?;
        let surface = pane_tree
            .surface_infos(Some(resolved_pane.pane_id))
            .into_iter()
            .find(|surface| surface.surface_id == surface_id)
            .ok_or_else(|| {
                ControlError::invalid_params(format!(
                    "Surface id {} is no longer available in tab {}.",
                    surface_id,
                    tab_idx + 1
                ))
            })?;
        Ok(ResolvedSurfaceTarget {
            terminal: surface.terminal,
            pane_index: surface.pane_index,
            pane_id: surface.pane_id,
            surface_index: surface.surface_index,
            surface_id,
        })
    }

    fn surface_created_result(
        &self,
        tab_idx: usize,
        pane_id: usize,
        surface_id: usize,
        created_pane: bool,
        terminal: &TerminalPane,
        cx: &App,
    ) -> serde_json::Value {
        let pane_tree = &self.tabs[tab_idx].pane_tree;
        let pane_index = pane_tree
            .pane_terminals()
            .into_iter()
            .enumerate()
            .find_map(|(index, (candidate, _))| (candidate == pane_id).then_some(index + 1))
            .unwrap_or(1);
        let surface_index = pane_tree
            .surface_infos(Some(pane_id))
            .into_iter()
            .find_map(|surface| (surface.surface_id == surface_id).then_some(surface.surface_index))
            .unwrap_or(1);
        json!({
            "tab_index": tab_idx + 1,
            "created_pane": created_pane,
            "pane_index": pane_index,
            "pane_id": pane_id,
            "pane_ref": format!("pane:{pane_index}"),
            "surface_index": surface_index,
            "surface_id": surface_id,
            "surface_ref": format!("surface:{surface_index}"),
            "surface_ready": terminal.surface_ready(cx),
            "is_alive": terminal.is_alive(cx),
            "has_shell_integration": terminal.has_shell_integration(cx),
        })
    }

    fn surface_wait_ready_result(
        &self,
        tab_idx: usize,
        resolved: &ResolvedSurfaceTarget,
        status: &str,
        cx: &App,
    ) -> serde_json::Value {
        json!({
            "status": status,
            "tab_index": tab_idx + 1,
            "pane_index": resolved.pane_index,
            "pane_id": resolved.pane_id,
            "surface_index": resolved.surface_index,
            "surface_id": resolved.surface_id,
            "surface_ready": resolved.terminal.surface_ready(cx),
            "is_alive": resolved.terminal.is_alive(cx),
            "has_shell_integration": resolved.terminal.has_shell_integration(cx),
            "is_busy": resolved.terminal.is_busy(cx),
        })
    }

    fn surface_info_value(
        &self,
        tab_idx: usize,
        surface: crate::pane_tree::PaneSurfaceInfo,
        cx: &App,
    ) -> serde_json::Value {
        let title = surface
            .title
            .clone()
            .or_else(|| surface.terminal.title(cx))
            .unwrap_or_else(|| format!("Surface {}", surface.surface_index));
        let (cols, rows) = surface.terminal.grid_size(cx);
        json!({
            "tab_index": tab_idx + 1,
            "pane_index": surface.pane_index,
            "pane_id": surface.pane_id,
            "pane_ref": format!("pane:{}", surface.pane_index),
            "surface_index": surface.surface_index,
            "surface_id": surface.surface_id,
            "surface_ref": format!("surface:{}", surface.surface_index),
            "title": title,
            "cwd": surface.terminal.current_dir(cx),
            "is_active": surface.is_active,
            "is_focused_pane": surface.is_focused_pane,
            "surface_ready": surface.terminal.surface_ready(cx),
            "is_alive": surface.terminal.is_alive(cx),
            "has_shell_integration": surface.terminal.has_shell_integration(cx),
            "is_busy": surface.terminal.is_busy(cx),
            "rows": rows,
            "cols": cols,
            "owner": surface.owner,
            "close_pane_when_last": surface.close_pane_when_last,
        })
    }

    fn surface_key_bytes(key: &str) -> Result<Vec<u8>, ControlError> {
        let key = key.trim();
        match key.to_ascii_lowercase().as_str() {
            "escape" | "esc" => Ok(vec![0x1b]),
            "enter" | "return" => Ok(b"\n".to_vec()),
            "tab" => Ok(b"\t".to_vec()),
            "backspace" => Ok(vec![0x7f]),
            "ctrl-c" | "control-c" | "c-c" => Ok(vec![0x03]),
            "ctrl-d" | "control-d" | "c-d" => Ok(vec![0x04]),
            _ if key.chars().count() == 1 => Ok(key.as_bytes().to_vec()),
            _ => Err(ControlError::invalid_params(format!(
                "Unsupported surface key `{key}`. Supported keys: escape, enter, tab, backspace, ctrl-c, ctrl-d."
            ))),
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

    fn tab_index_for_summary_id(&self, tab_id: u64) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.summary_id == tab_id)
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
            ControlCommand::TreeGet { tab_index } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => {
                        let tabs = self
                        .tabs
                        .iter()
                        .enumerate()
                        .filter(|(idx, _)| tab_index.is_none() || *idx == tab_idx)
                        .map(|(idx, tab)| {
                            let panes = tab
                                .pane_tree
                                .pane_terminals()
                                .into_iter()
                                .enumerate()
                                .map(|(pane_index, (pane_id, terminal))| {
                                    let surfaces = tab
                                        .pane_tree
                                        .surface_infos(Some(pane_id))
                                        .into_iter()
                                        .map(|surface| self.surface_info_value(idx, surface, cx))
                                        .collect::<Vec<_>>();
                                    json!({
                                        "pane_index": pane_index + 1,
                                        "pane_id": pane_id,
                                        "pane_ref": format!("pane:{}", pane_index + 1),
                                        "title": terminal.title(cx).unwrap_or_else(|| format!("Pane {}", pane_index + 1)),
                                        "is_focused": pane_id == tab.pane_tree.focused_pane_id(),
                                        "active_surface_id": tab.pane_tree.active_surface_id_for_pane(pane_id),
                                        "surfaces": surfaces,
                                    })
                                })
                                .collect::<Vec<_>>();
                            json!({
                                "tab_index": idx + 1,
                                "is_active": idx == self.active_tab,
                                "title": tab.title,
                                "focused_pane_id": tab.pane_tree.focused_pane_id(),
                                "panes": panes,
                            })
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
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::SurfacesList { tab_index, pane } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => {
                        let target_pane_id = if pane.pane_index.is_some() || pane.pane_id.is_some()
                        {
                            match self.resolve_pane_target_for_tab(
                                tab_idx,
                                Self::pane_selector_from_target(pane),
                            ) {
                                Ok(resolved) => Some(resolved.pane_id),
                                Err(err) => {
                                    Self::send_control_result(
                                        response_tx,
                                        Err(ControlError::invalid_params(err)),
                                    );
                                    return;
                                }
                            }
                        } else {
                            None
                        };
                        let surfaces = self.tabs[tab_idx]
                            .pane_tree
                            .surface_infos(target_pane_id)
                            .into_iter()
                            .map(|surface| self.surface_info_value(tab_idx, surface, cx))
                            .collect::<Vec<_>>();
                        Self::send_control_result(
                            response_tx,
                            Ok(json!({
                                "tab_index": tab_idx + 1,
                                "surfaces": surfaces,
                            })),
                        );
                    }
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::SurfacesCreate {
                tab_index,
                pane,
                title,
                command,
                owner,
                close_pane_when_last,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => {
                    self.pending_surface_control_requests.push(
                        PendingSurfaceControlRequest::Create {
                            tab_idx,
                            pane,
                            title,
                            command,
                            owner,
                            close_pane_when_last,
                            response_tx,
                        },
                    );
                    self.schedule_pending_create_pane_flush(cx);
                }
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::SurfacesSplit {
                tab_index,
                source,
                location,
                title,
                command,
                owner,
                close_pane_when_last,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => {
                    self.pending_surface_control_requests.push(
                        PendingSurfaceControlRequest::Split {
                            tab_idx,
                            source,
                            location,
                            title,
                            command,
                            owner,
                            close_pane_when_last,
                            response_tx,
                        },
                    );
                    self.schedule_pending_create_pane_flush(cx);
                }
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::SurfacesFocus { tab_index, target } => {
                match self.resolve_control_tab_index(tab_index) {
                    Ok(tab_idx) => match self.resolve_surface_target_for_tab(tab_idx, target) {
                        Ok(resolved) => {
                            let surface_id = resolved.surface_id;
                            let changed = self.tabs[tab_idx].pane_tree.focus_surface(surface_id);
                            let resolved = match self.resolve_surface_target_for_tab(
                                tab_idx,
                                con_core::SurfaceTarget::new(None, None, Some(surface_id)),
                            ) {
                                Ok(resolved) => resolved,
                                Err(err) => {
                                    Self::send_control_result(response_tx, Err(err));
                                    return;
                                }
                            };
                            if changed {
                                if tab_idx == self.active_tab {
                                    self.sync_active_tab_native_view_visibility(cx);
                                    self.sync_active_terminal_focus_states(cx);
                                    self.schedule_active_terminal_focus(cx);
                                }
                                self.sync_sidebar(cx);
                                self.save_session(cx);
                                cx.notify();
                            }
                            Self::send_control_result(
                                response_tx,
                                Ok(json!({
                                    "status": if changed { "focused" } else { "unchanged" },
                                    "tab_index": tab_idx + 1,
                                    "pane_index": resolved.pane_index,
                                    "pane_id": resolved.pane_id,
                                    "surface_index": resolved.surface_index,
                                    "surface_id": resolved.surface_id,
                                })),
                            );
                        }
                        Err(err) => Self::send_control_result(response_tx, Err(err)),
                    },
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                }
            }
            ControlCommand::SurfacesRename {
                tab_index,
                target,
                title,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => match self.resolve_surface_target_for_tab(tab_idx, target) {
                    Ok(resolved) => {
                        self.tabs[tab_idx]
                            .pane_tree
                            .rename_surface(resolved.surface_id, Some(title.clone()));
                        self.sync_sidebar(cx);
                        self.save_session(cx);
                        Self::send_control_result(
                            response_tx,
                            Ok(json!({
                                "status": "renamed",
                                "tab_index": tab_idx + 1,
                                "pane_id": resolved.pane_id,
                                "surface_id": resolved.surface_id,
                                "title": title,
                            })),
                        );
                        cx.notify();
                    }
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                },
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::SurfacesClose {
                tab_index,
                target,
                close_empty_owned_pane,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => {
                    self.pending_surface_control_requests.push(
                        PendingSurfaceControlRequest::Close {
                            tab_idx,
                            target,
                            close_empty_owned_pane,
                            response_tx,
                        },
                    );
                    self.schedule_pending_create_pane_flush(cx);
                }
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::SurfacesRead {
                tab_index,
                target,
                lines,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => match self.resolve_surface_target_for_tab(tab_idx, target) {
                    Ok(resolved) => Self::send_control_result(
                        response_tx,
                        Ok(json!({
                            "tab_index": tab_idx + 1,
                            "pane_id": resolved.pane_id,
                            "surface_id": resolved.surface_id,
                            "content": resolved.terminal.recent_lines(lines, cx).join("\n"),
                        })),
                    ),
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                },
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::SurfacesSendText {
                tab_index,
                target,
                text,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => match self.resolve_surface_target_for_tab(tab_idx, target) {
                    Ok(resolved) => {
                        resolved.terminal.write(text.as_bytes(), cx);
                        self.record_runtime_event_for_terminal(
                            tab_idx,
                            &resolved.terminal,
                            con_agent::context::PaneRuntimeEvent::RawInput {
                                keys: text,
                                input_generation: resolved.terminal.input_generation(cx),
                            },
                        );
                        Self::send_control_result(
                            response_tx,
                            Ok(json!({
                                "status": "sent",
                                "tab_index": tab_idx + 1,
                                "pane_id": resolved.pane_id,
                                "surface_id": resolved.surface_id,
                            })),
                        );
                    }
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                },
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::SurfacesSendKey {
                tab_index,
                target,
                key,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => match self.resolve_surface_target_for_tab(tab_idx, target) {
                    Ok(resolved) => match Self::surface_key_bytes(&key) {
                        Ok(bytes) => {
                            resolved.terminal.write(&bytes, cx);
                            self.record_runtime_event_for_terminal(
                                tab_idx,
                                &resolved.terminal,
                                con_agent::context::PaneRuntimeEvent::RawInput {
                                    keys: key,
                                    input_generation: resolved.terminal.input_generation(cx),
                                },
                            );
                            Self::send_control_result(
                                response_tx,
                                Ok(json!({
                                    "status": "sent",
                                    "tab_index": tab_idx + 1,
                                    "pane_id": resolved.pane_id,
                                    "surface_id": resolved.surface_id,
                                })),
                            );
                        }
                        Err(err) => Self::send_control_result(response_tx, Err(err)),
                    },
                    Err(err) => Self::send_control_result(response_tx, Err(err)),
                },
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
            ControlCommand::SurfacesWaitReady {
                tab_index,
                target,
                timeout_secs,
            } => match self.resolve_control_tab_index(tab_index) {
                Ok(tab_idx) => {
                    let requested_tab_index = tab_idx + 1;
                    let tab_id = self.tabs[tab_idx].summary_id;
                    let surface_id = match self.resolve_surface_target_for_tab(tab_idx, target) {
                        Ok(resolved) => resolved.surface_id,
                        Err(err) => {
                            Self::send_control_result(response_tx, Err(err));
                            return;
                        }
                    };
                    let timeout = Duration::from_secs(timeout_secs.unwrap_or(10).clamp(1, 300));
                    let started = std::time::Instant::now();
                    cx.spawn(async move |this, cx| {
                        loop {
                            let result = this.update(cx, |workspace, cx| {
                                let Some(current_tab_idx) =
                                    workspace.tab_index_for_summary_id(tab_id)
                                else {
                                    return Some(Err(ControlError::invalid_params(format!(
                                        "Tab {} is no longer available.",
                                        requested_tab_index
                                    ))));
                                };
                                let resolved = match workspace.resolve_surface_target_for_tab(
                                    current_tab_idx,
                                    con_core::SurfaceTarget::new(None, None, Some(surface_id)),
                                ) {
                                    Ok(resolved) => resolved,
                                    Err(err) => return Some(Err(err)),
                                };
                                let is_ready = resolved.terminal.surface_ready(cx)
                                    && resolved.terminal.is_alive(cx)
                                    && resolved.terminal.has_shell_integration(cx);
                                let timed_out = started.elapsed() >= timeout;
                                if is_ready || timed_out {
                                    let status = if is_ready { "ready" } else { "timeout" };
                                    Some(Ok(workspace.surface_wait_ready_result(
                                        current_tab_idx,
                                        &resolved,
                                        status,
                                        cx,
                                    )))
                                } else {
                                    None
                                }
                            });

                            match result {
                                Ok(Some(result)) => {
                                    Self::send_control_result(response_tx, result);
                                    return;
                                }
                                Ok(None) => {}
                                Err(err) => {
                                    Self::send_control_result(
                                        response_tx,
                                        Err(ControlError::internal(format!(
                                            "Failed to wait for surface readiness: {err}"
                                        ))),
                                    );
                                    return;
                                }
                            }

                            cx.background_executor()
                                .timer(Duration::from_millis(50))
                                .await;
                        }
                    })
                    .detach();
                }
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
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
                    let agent_config = self.tab_agent_config(tab_idx);
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
                    self.harness
                        .send_message(session, agent_config, prompt, context);
                }
                Err(err) => Self::send_control_result(response_tx, Err(err)),
            },
        }
    }

    fn spawn_control_agent_request_timeout(
        &self,
        _tab_idx: usize,
        request_id: u64,
        timeout_secs: u64,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(timeout_secs))
                .await;

            let _ = this.update(cx, |workspace, _| {
                let current_tab_idx = workspace
                    .pending_control_agent_requests
                    .iter()
                    .find_map(|(idx, pending)| (pending.request_id == request_id).then_some(*idx));
                if let Some(current_tab_idx) = current_tab_idx {
                    let pending = workspace
                        .pending_control_agent_requests
                        .remove(&current_tab_idx)
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
        self.snapshot_session_with_options(cx, self.config.appearance.restore_terminal_text)
    }

    fn snapshot_session_with_options(&self, cx: &App, capture_screen_text: bool) -> Session {
        let tabs: Vec<con_core::session::TabState> = self
            .tabs
            .iter()
            .map(|tab| {
                let terminal = tab.pane_tree.focused_terminal();
                let cwd = terminal.current_dir(cx);
                let title = terminal.title(cx).unwrap_or_else(|| tab.title.clone());
                let pane_layout = tab.pane_tree.to_state(cx, capture_screen_text);
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
                    agent_routing: tab.agent_routing.clone(),
                    user_label: tab.user_label.clone(),
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
            vertical_tabs_pinned: self.sidebar.read(cx).is_pinned(),
            vertical_tabs_width: Some(self.sidebar.read(cx).panel_width()),
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
        if let Err(err) = self.session_save_tx.send(SessionSaveRequest::Flush(
            session.clone(),
            history.clone(),
            done_tx,
        )) {
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
                log::warn!(
                    "Failed to save session directly after flush timeout: {}",
                    save_err
                );
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

    fn default_agent_routing(config: &AgentConfig) -> AgentRoutingState {
        let mut model_overrides = Vec::new();
        for provider in [
            ProviderKind::Anthropic,
            ProviderKind::OpenAI,
            ProviderKind::ChatGPT,
            ProviderKind::GitHubCopilot,
            ProviderKind::OpenAICompatible,
            ProviderKind::MiniMax,
            ProviderKind::MiniMaxAnthropic,
            ProviderKind::Moonshot,
            ProviderKind::MoonshotAnthropic,
            ProviderKind::ZAI,
            ProviderKind::ZAIAnthropic,
            ProviderKind::DeepSeek,
            ProviderKind::Groq,
            ProviderKind::Cohere,
            ProviderKind::Gemini,
            ProviderKind::Ollama,
            ProviderKind::OpenRouter,
            ProviderKind::Perplexity,
            ProviderKind::Mistral,
            ProviderKind::Together,
            ProviderKind::XAI,
        ] {
            if let Some(model) = config
                .providers
                .get(&provider)
                .and_then(|entry| entry.model.clone())
            {
                model_overrides.push(AgentModelOverrideState { provider, model });
            }
        }

        AgentRoutingState {
            provider: Some(config.provider.clone()),
            model_overrides,
        }
    }

    fn apply_agent_routing(base: &AgentConfig, routing: &AgentRoutingState) -> AgentConfig {
        let mut config = base.clone();
        if let Some(provider) = routing.provider.as_ref() {
            config.provider = provider.clone();
        }

        for override_state in &routing.model_overrides {
            if override_state.model.trim().is_empty() {
                continue;
            }
            let mut provider_config = config.providers.get_or_default(&override_state.provider);
            provider_config.model = Some(override_state.model.clone());
            config
                .providers
                .set(&override_state.provider, provider_config);
        }

        config
    }

    fn tab_agent_config(&self, tab_idx: usize) -> AgentConfig {
        Self::apply_agent_routing(self.harness.config(), &self.tabs[tab_idx].agent_routing)
    }

    fn active_tab_agent_config(&self) -> AgentConfig {
        self.tab_agent_config(self.active_tab)
    }

    fn provider_models_for_config(&self, config: &AgentConfig) -> Vec<String> {
        self.model_registry.models_for_base_url(
            &config.provider,
            config
                .providers
                .get(&config.provider)
                .and_then(|pc| pc.base_url.as_deref()),
        )
    }

    fn set_tab_provider_override(&mut self, tab_idx: usize, provider: ProviderKind) {
        self.tabs[tab_idx].agent_routing.provider = Some(provider);
    }

    fn set_tab_model_override(&mut self, tab_idx: usize, provider: ProviderKind, model: String) {
        let routing = &mut self.tabs[tab_idx].agent_routing;
        if let Some(existing) = routing
            .model_overrides
            .iter_mut()
            .find(|entry| entry.provider == provider)
        {
            existing.model = model;
            return;
        }

        routing
            .model_overrides
            .push(AgentModelOverrideState { provider, model });
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
        let provider = self.tab_agent_config(self.active_tab).provider.clone();
        self.set_tab_model_override(self.active_tab, provider.clone(), event.model.clone());
        let config = self.tab_agent_config(self.active_tab);
        let available_models = self.provider_models_for_config(&config);

        self.agent_panel.update(cx, |panel, cx| {
            panel.set_session_provider_options(
                AgentPanel::configured_session_providers(&config),
                window,
                cx,
            );
            panel.set_model_name(event.model.clone());
            panel.set_session_model_options(available_models, window, cx);
        });

        self.save_session(cx);
        cx.notify();
    }

    fn on_select_session_provider(
        &mut self,
        _panel: &Entity<AgentPanel>,
        event: &SelectSessionProvider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_tab_provider_override(self.active_tab, event.provider.clone());
        let config = self.tab_agent_config(self.active_tab);

        let provider = config.provider.clone();
        let model_name = AgentHarness::active_model_name_for(&config);
        let available_models = self.provider_models_for_config(&config);

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

        self.save_session(cx);
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
        let agent_config = self.active_tab_agent_config();
        if event.content.trim().starts_with('/') {
            match self.harness.classify_input(
                &event.content,
                self.effective_remote_host_for_tab(self.active_tab, self.active_terminal(), cx)
                    .is_some(),
            ) {
                InputKind::SkillInvoke(name, args) => {
                    if let Some(desc) = self.harness.invoke_skill(
                        session,
                        agent_config.clone(),
                        &name,
                        args.as_deref(),
                        context,
                    ) {
                        self.agent_panel.update(cx, |panel, cx| {
                            panel.add_step(&desc, cx);
                        });
                    }
                }
                _ => self.harness.send_message(
                    session,
                    agent_config.clone(),
                    event.content.clone(),
                    context,
                ),
            }
        } else {
            self.harness
                .send_message(session, agent_config, event.content.clone(), context);
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

    fn on_sidebar_close_tab(
        &mut self,
        _sidebar: &Entity<SessionSidebar>,
        event: &SidebarCloseTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.summary_id == event.session_id)
        else {
            return;
        };
        self.close_tab_by_index(index, window, cx);
    }

    fn on_sidebar_rename(
        &mut self,
        _sidebar: &Entity<SessionSidebar>,
        event: &SidebarRename,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Empty / whitespace-only labels reset to smart auto-naming.
        let new_label = event
            .label
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.summary_id == event.session_id)
        else {
            return;
        };
        if self.tabs[index].user_label == new_label {
            return;
        }
        self.tabs[index].user_label = new_label;
        self.sync_sidebar(cx);
        self.save_session(cx);
        cx.notify();
    }

    fn on_sidebar_duplicate(
        &mut self,
        _sidebar: &Entity<SessionSidebar>,
        event: &SidebarDuplicate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.summary_id == event.session_id)
        else {
            return;
        };
        // For now, "duplicate" = open a new tab whose CWD matches the
        // source tab's focused terminal cwd. The conversation, panel
        // state, and history don't carry over — that's intentional;
        // duplicate is for "open another shell here", not "fork
        // session state".
        let cwd = self.tabs[index]
            .pane_tree
            .focused_terminal()
            .current_dir(cx);
        let terminal = self.create_terminal(cwd.as_deref(), window, cx);
        let tab_number = self.tabs.len() + 1;
        let summary_id = self.next_tab_summary_id;
        self.next_tab_summary_id += 1;
        self.tabs.push(Tab {
            pane_tree: PaneTree::new(terminal),
            title: format!("Terminal {}", tab_number),
            user_label: self.tabs[index].user_label.clone(),
            ai_label: None,
            ai_icon: None,
            summary_id,
            needs_attention: false,
            session: AgentSession::new(),
            agent_routing: self.tabs[index].agent_routing.clone(),
            panel_state: PanelState::new(),
            runtime_trackers: RefCell::new(HashMap::new()),
            runtime_cache: RefCell::new(HashMap::new()),
            shell_history: HashMap::new(),
        });
        let new_index = self.tabs.len() - 1;
        if self.sync_tab_strip_motion() {
            #[cfg(target_os = "macos")]
            self.arm_top_chrome_snap_guard(cx);
        }
        self.activate_tab(new_index, window, cx);
    }

    fn on_sidebar_reorder(
        &mut self,
        _sidebar: &Entity<SessionSidebar>,
        event: &SidebarReorder,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self
            .tabs
            .iter()
            .position(|tab| tab.summary_id == event.session_id)
        else {
            return;
        };
        // Sidebar emits `to` as a *slot* in `0..=tabs.len()`:
        //   slot K with K < tabs.len() == "insert before row K"
        //   slot tabs.len()             == "after the last row"
        // After `Vec::remove(from)` shifts every subsequent index
        // down by one, the resulting insert index is:
        //   from < to → to - 1 (the slot moved down with the rest)
        //   from > to → to     (the slot was above the source)
        //   from == to or from + 1 == to → no-op (drop on the same
        //     row's top half, or the slot just below — same place).
        let to = match event.move_delta {
            Some(delta) if delta < 0 => from.saturating_sub(1),
            Some(delta) if delta > 0 => (from + 2).min(self.tabs.len()),
            _ => event.to,
        };
        if from >= self.tabs.len() || to > self.tabs.len() {
            return;
        }
        if from == to || from + 1 == to {
            return;
        }
        let old_order: Vec<u64> = self.tabs.iter().map(|tab| tab.summary_id).collect();
        let active_id = self.tabs[self.active_tab].summary_id;
        let insert_at = if from < to { to - 1 } else { to };
        let tab = self.tabs.remove(from);
        // Vec::insert clamps via assert; insert_at is guaranteed
        // ≤ tabs.len() (post-remove) by construction above.
        self.tabs.insert(insert_at, tab);

        let new_positions: HashMap<u64, usize> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(idx, tab)| (tab.summary_id, idx))
            .collect();
        let mut remapped_pending = HashMap::new();
        for (old_idx, pending) in std::mem::take(&mut self.pending_control_agent_requests) {
            if let Some(summary_id) = old_order.get(old_idx)
                && let Some(&new_idx) = new_positions.get(summary_id)
            {
                remapped_pending.insert(new_idx, pending);
            }
        }
        self.pending_control_agent_requests = remapped_pending;

        for pending in &mut self.pending_create_pane_requests {
            if let Some(summary_id) = old_order.get(pending.tab_idx)
                && let Some(&new_idx) = new_positions.get(summary_id)
            {
                pending.tab_idx = new_idx;
            }
        }

        for pending in &mut self.pending_window_control_requests {
            if let PendingWindowControlRequest::TabsClose { tab_idx, .. } = pending
                && let Some(summary_id) = old_order.get(*tab_idx)
                && let Some(&new_idx) = new_positions.get(summary_id)
            {
                *tab_idx = new_idx;
            }
        }

        for pending in &mut self.pending_surface_control_requests {
            let tab_idx = match pending {
                PendingSurfaceControlRequest::Create { tab_idx, .. }
                | PendingSurfaceControlRequest::Split { tab_idx, .. }
                | PendingSurfaceControlRequest::Close { tab_idx, .. } => tab_idx,
            };
            if let Some(summary_id) = old_order.get(*tab_idx)
                && let Some(&new_idx) = new_positions.get(summary_id)
            {
                *tab_idx = new_idx;
            }
        }

        // Re-locate the active tab by stable summary_id rather than
        // index arithmetic (which had its own off-by-one in the
        // previous version).
        if let Some(new_active) = self.tabs.iter().position(|t| t.summary_id == active_id) {
            self.active_tab = new_active;
        }

        self.sync_sidebar(cx);
        self.save_session(cx);
        cx.notify();
    }

    fn on_sidebar_close_others(
        &mut self,
        _sidebar: &Entity<SessionSidebar>,
        event: &SidebarCloseOthers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(keep) = self
            .tabs
            .iter()
            .position(|tab| tab.summary_id == event.session_id)
        else {
            return;
        };
        // Iterate from the end so indices stay stable as we close
        // tabs to the right of `keep`. After that, close everything
        // left of `keep` from the highest index downwards.
        let mut i = self.tabs.len();
        while i > keep + 1 {
            i -= 1;
            self.close_tab_by_index(i, window, cx);
        }
        let mut j = keep;
        while j > 0 {
            j -= 1;
            self.close_tab_by_index(j, window, cx);
        }
    }

    fn sync_sidebar(&self, cx: &mut Context<Self>) {
        let sessions: Vec<SessionEntry> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let terminal = tab.pane_tree.focused_terminal();
                let hostname = self.effective_remote_host_for_tab(i, terminal, cx);
                let title = terminal.title(cx);
                let current_dir = terminal.current_dir(cx);
                let presentation = smart_tab_presentation(
                    tab.user_label.as_deref(),
                    tab.ai_label.as_deref(),
                    tab.ai_icon.map(|k| k.svg_path()),
                    hostname.as_deref(),
                    title.as_deref(),
                    current_dir.as_deref(),
                    i,
                );
                let pane_count = tab.pane_tree.pane_terminals().len();
                SessionEntry {
                    id: tab.summary_id,
                    name: presentation.name,
                    subtitle: presentation.subtitle,
                    is_ssh: presentation.is_ssh,
                    needs_attention: tab.needs_attention,
                    icon: presentation.icon,
                    has_user_label: tab.user_label.is_some(),
                    pane_count,
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
            "new-window" => {
                cx.dispatch_action(&crate::NewWindow);
            }
            "toggle-agent" => {
                self.toggle_agent_panel(&ToggleAgentPanel, window, cx);
            }
            "settings" => {
                self.toggle_settings(&settings_panel::ToggleSettings, window, cx);
            }
            "new-tab" => {
                self.new_tab(&NewTab, window, cx);
            }
            "export-workspace-layout" => {
                self.export_workspace_layout(&ExportWorkspaceLayout, window, cx);
            }
            "add-workspace-layout-tabs" => {
                self.add_workspace_layout_tabs(&AddWorkspaceLayoutTabs, window, cx);
            }
            "open-workspace-layout-window" => {
                self.open_workspace_layout_window(&OpenWorkspaceLayoutWindow, window, cx);
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
            "split-left" => {
                self.split_pane(
                    SplitDirection::Horizontal,
                    SplitPlacement::Before,
                    window,
                    cx,
                );
            }
            "split-up" => {
                self.split_pane(SplitDirection::Vertical, SplitPlacement::Before, window, cx);
            }
            "toggle-pane-zoom" => {
                self.toggle_pane_zoom(&TogglePaneZoom, window, cx);
            }
            "new-surface" => {
                self.create_surface_in_focused_pane(window, cx);
            }
            "new-surface-split-right" => {
                self.create_surface_split_from_focused_pane(
                    SplitDirection::Horizontal,
                    SplitPlacement::After,
                    window,
                    cx,
                );
            }
            "new-surface-split-down" => {
                self.create_surface_split_from_focused_pane(
                    SplitDirection::Vertical,
                    SplitPlacement::After,
                    window,
                    cx,
                );
            }
            "next-surface" => {
                self.cycle_surface_in_focused_pane(1, window, cx);
            }
            "previous-surface" => {
                self.cycle_surface_in_focused_pane(-1, window, cx);
            }
            "rename-surface" => {
                self.rename_current_surface(&RenameSurface, window, cx);
            }
            "close-surface" => {
                self.close_current_surface_in_focused_pane(window, cx);
            }
            "clear-terminal" => {
                if self.has_active_tab() {
                    self.active_terminal().clear_scrollback(cx);
                }
            }
            "clear-restored-terminal-history" => {
                self.clear_restored_terminal_history(window, cx);
            }
            "focus-terminal" => {
                if self.has_active_tab() {
                    self.active_terminal().focus(window, cx);
                }
            }
            "toggle-input-bar" => {
                self.toggle_input_bar(&crate::ToggleInputBar, window, cx);
            }
            "toggle-vertical-tabs" => {
                self.toggle_vertical_tabs(&ToggleVerticalTabs, window, cx);
            }
            "cycle-input-mode" => {
                self.input_bar.update(cx, |bar, cx| {
                    bar.cycle_mode(window, cx);
                });
                self.sync_active_terminal_focus_states(cx);
            }
            "check-for-updates" => {
                cx.dispatch_action(&crate::CheckForUpdates);
            }
            "quit" => {
                self.quit(&Quit, window, cx);
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
        self.apply_settings_from_panel(settings, window, cx, true);
    }

    fn apply_settings_from_panel(
        &mut self,
        settings: &Entity<SettingsPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
        restore_focus: bool,
    ) {
        let full_config = settings.read(cx).config().clone();
        let restore_terminal_text_was_enabled = self.config.appearance.restore_terminal_text;
        self.config = full_config.clone();
        let new_agent_config = full_config.agent.clone();
        let auto_approve = new_agent_config.auto_approve_tools;
        self.harness.update_config(new_agent_config);
        self.shell_suggestion_engine = self.harness.suggestion_engine(180);
        self.shell_suggestion_engine.clear_cache();
        // Same suggestion model drives the tab summarizer; rebuild
        // it so the new credentials / model override take effect.
        self.tab_summary_engine = self
            .harness
            .tab_summary_engine()
            .with_state_from(&self.tab_summary_engine);
        self.tab_summary_generation = self.tab_summary_generation.wrapping_add(1);
        self.tab_summary_engine.clear_success_cache();
        for tab in &mut self.tabs {
            tab.ai_label = None;
            tab.ai_icon = None;
        }
        self.sync_sidebar(cx);
        if self.harness.config().suggestion_model.enabled {
            // Re-ask for fresh summaries with the new model.
            self.request_tab_summaries(cx);
        } else {
            for tab in &mut self.tabs {
                tab.ai_label = None;
                tab.ai_icon = None;
            }
            self.sync_sidebar(cx);
        }
        let active_agent_config = self.active_tab_agent_config();
        let active_agent_models = self.provider_models_for_config(&active_agent_config);

        // Sync auto-approve to agent panel UI
        self.agent_panel.update(cx, |panel, cx| {
            panel.set_auto_approve(auto_approve);
            panel.set_session_provider_options(
                AgentPanel::configured_session_providers(&active_agent_config),
                window,
                cx,
            );
            panel.set_provider_name(active_agent_config.provider.clone(), window, cx);
            panel.set_model_name(AgentHarness::active_model_name_for(&active_agent_config));
            panel.set_session_model_options(active_agent_models, window, cx);
        });

        // Apply updated skills paths (forces rescan on next cwd check)
        let skills_config = full_config.skills.clone();
        self.harness.update_skills_config(skills_config);
        if self.has_active_tab() {
            if let Some(cwd) = self.active_terminal().current_dir(cx) {
                self.harness.scan_skills(&cwd);
            }
        }

        let term_config = full_config.terminal.clone();
        let appearance_config = full_config.appearance.clone();
        self.apply_terminal_and_ui_appearance(&term_config, &appearance_config, window, cx);
        if restore_terminal_text_was_enabled && !appearance_config.restore_terminal_text {
            self.save_session(cx);
        }

        // Re-apply keybindings at runtime so changes take effect immediately
        let kb = full_config.keybindings.clone();
        crate::bind_app_keybindings(cx, &kb);
        #[cfg(target_os = "macos")]
        crate::global_hotkey::update_from_keybindings(&kb);
        #[cfg(target_os = "macos")]
        crate::quick_terminal::set_always_on_top(kb.quick_terminal_always_on_top);

        if restore_focus {
            self.focus_terminal(window, cx);
        }
    }

    fn on_appearance_preview(
        &mut self,
        settings: &Entity<SettingsPanel>,
        _event: &AppearancePreview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_appearance_preview_from_panel(settings, window, cx);
    }

    fn apply_appearance_preview_from_panel(
        &mut self,
        settings: &Entity<SettingsPanel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let term_config = settings.read(cx).terminal_config().clone();
        let appearance_config = settings.read(cx).appearance_config().clone();
        self.apply_terminal_and_ui_appearance(&term_config, &appearance_config, window, cx);
        cx.notify();
    }

    fn on_tabs_orientation_changed(
        &mut self,
        settings: &Entity<SettingsPanel>,
        _event: &TabsOrientationChanged,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_tabs_orientation_from_panel(settings, cx);
    }

    fn apply_tabs_orientation_from_panel(
        &mut self,
        settings: &Entity<SettingsPanel>,
        cx: &mut Context<Self>,
    ) {
        let orientation = settings.read(cx).appearance_config().tabs_orientation;
        self.apply_tabs_orientation(orientation, false, cx);
    }

    fn apply_tabs_orientation(
        &mut self,
        orientation: TabsOrientation,
        persist_config: bool,
        cx: &mut Context<Self>,
    ) {
        if self.tabs_orientation == orientation {
            return;
        }
        #[cfg(target_os = "macos")]
        let previous_sidebar_width = if self.vertical_tabs_active() {
            self.sidebar
                .read(cx)
                .occupied_width_with_max(PANEL_MAX_WIDTH)
        } else {
            0.0
        };
        #[cfg(target_os = "macos")]
        if self.terminal_opacity >= 0.999 {
            self.arm_chrome_transition_underlay(Duration::from_millis(260));
        }
        self.config.appearance.tabs_orientation = orientation;
        self.tabs_orientation = orientation;
        if persist_config {
            self.sync_settings_panels_tabs_orientation(orientation, cx);
        }
        if self.sync_tab_strip_motion() {
            #[cfg(target_os = "macos")]
            self.arm_top_chrome_snap_guard(cx);
        }
        #[cfg(target_os = "macos")]
        self.arm_sidebar_snap_guard(
            if self.vertical_tabs_active() {
                self.sidebar
                    .read(cx)
                    .occupied_width_with_max(PANEL_MAX_WIDTH)
            } else {
                previous_sidebar_width
            },
            cx,
        );
        if persist_config {
            if let Err(err) = self.persist_tabs_orientation(orientation) {
                log::warn!("workspace: persist tabs_orientation failed: {err}");
            }
        }
        self.save_session(cx);
        cx.notify();
    }

    fn sync_settings_panels_tabs_orientation(
        &mut self,
        orientation: TabsOrientation,
        cx: &mut Context<Self>,
    ) {
        self.settings_panel.update(cx, |panel, cx| {
            panel.set_tabs_orientation(orientation);
            cx.notify();
        });

        let mut settings_window_open = false;
        if let Some(handle) = self.settings_window {
            settings_window_open = handle.update(cx, |_, _, _| {}).is_ok();
        }
        if settings_window_open {
            if let Some(panel) = self.settings_window_panel.clone() {
                panel.update(cx, |panel, cx| {
                    panel.set_tabs_orientation(orientation);
                    cx.notify();
                });
            }
        } else {
            self.settings_window = None;
            self.settings_window_panel = None;
        }
    }

    fn persist_tabs_orientation(&self, orientation: TabsOrientation) -> anyhow::Result<()> {
        let mut config = Config::load().unwrap_or_else(|err| {
            log::warn!(
                "workspace: reload config before tabs_orientation save failed: {err}; using workspace config"
            );
            self.config.clone()
        });
        config.appearance.tabs_orientation = orientation;
        config.save()
    }

    fn on_theme_preview(
        &mut self,
        _settings: &Entity<SettingsPanel>,
        event: &ThemePreview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_theme_preview(&event.0, window, cx);
    }

    fn apply_theme_preview(
        &mut self,
        theme_name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(new_theme) = TerminalTheme::by_name(theme_name) {
            if new_theme.name != self.terminal_theme.name {
                self.apply_terminal_theme(new_theme, window, cx);
                cx.notify();
            }
        }
    }

    fn apply_terminal_and_ui_appearance(
        &mut self,
        term_config: &TerminalConfig,
        appearance_config: &AppearanceConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next_terminal_font_family = sanitize_terminal_font_family(&term_config.font_family);
        let next_ui_font_family = appearance_config.ui_font_family.clone();
        let next_ui_font_size = appearance_config.ui_font_size;
        let next_font_size = term_config.font_size;
        let next_terminal_cursor_style = term_config.cursor_style.clone();
        let next_terminal_opacity =
            Self::effective_terminal_opacity(appearance_config.terminal_opacity);
        let next_terminal_blur = Self::effective_terminal_blur(appearance_config.terminal_blur);
        let next_background_image = appearance_config.background_image.clone();
        let next_background_image_opacity =
            Self::clamp_background_image_opacity(appearance_config.background_image_opacity);
        let next_background_image_position = appearance_config.background_image_position.clone();
        let next_background_image_fit = appearance_config.background_image_fit.clone();
        let next_background_image_repeat = appearance_config.background_image_repeat;

        let font_changed = self.terminal_font_family != next_terminal_font_family
            || (self.font_size - next_font_size).abs() > f32::EPSILON;
        let terminal_appearance_changed = font_changed
            || self.terminal_cursor_style != next_terminal_cursor_style
            || (self.terminal_opacity - next_terminal_opacity).abs() > f32::EPSILON
            || self.terminal_blur != next_terminal_blur
            || self.background_image != next_background_image
            || (self.background_image_opacity - next_background_image_opacity).abs() > f32::EPSILON
            || self.background_image_position != next_background_image_position
            || self.background_image_fit != next_background_image_fit
            || self.background_image_repeat != next_background_image_repeat;
        let ui_theme_changed = font_changed
            || self.ui_font_family != next_ui_font_family
            || (self.ui_font_size - next_ui_font_size).abs() > f32::EPSILON;

        self.terminal_font_family = next_terminal_font_family;
        self.ui_font_family = next_ui_font_family;
        self.ui_font_size = next_ui_font_size;
        self.font_size = next_font_size;
        self.terminal_cursor_style = next_terminal_cursor_style;
        self.terminal_opacity = next_terminal_opacity;
        self.terminal_blur = next_terminal_blur;
        self.ui_opacity = Self::clamp_ui_opacity(appearance_config.ui_opacity);
        self.background_image = next_background_image;
        self.background_image_opacity = next_background_image_opacity;
        self.background_image_position = next_background_image_position;
        self.background_image_fit = next_background_image_fit;
        self.background_image_repeat = next_background_image_repeat;

        if self.tabs_orientation != appearance_config.tabs_orientation {
            #[cfg(target_os = "macos")]
            if self.terminal_opacity >= 0.999 {
                self.arm_chrome_transition_underlay(Duration::from_millis(260));
            }
            #[cfg(target_os = "macos")]
            let previous_sidebar_width = if self.vertical_tabs_active() {
                self.sidebar
                    .read(cx)
                    .occupied_width_with_max(PANEL_MAX_WIDTH)
            } else {
                0.0
            };
            self.tabs_orientation = appearance_config.tabs_orientation;
            // Tab strip motion drives the top-bar height. In vertical
            // mode the strip is always hidden, so collapse the motion now
            // and avoid an unrelated transition.
            if self.sync_tab_strip_motion() {
                #[cfg(target_os = "macos")]
                self.arm_top_chrome_snap_guard(cx);
            }
            #[cfg(target_os = "macos")]
            self.arm_sidebar_snap_guard(
                if self.vertical_tabs_active() {
                    self.sidebar
                        .read(cx)
                        .occupied_width_with_max(PANEL_MAX_WIDTH)
                } else {
                    previous_sidebar_width
                },
                cx,
            );
            self.save_session(cx);
            cx.notify();
        }

        let effective_ui_opacity = Self::effective_ui_opacity(self.ui_opacity);
        self.agent_panel
            .update(cx, |panel, _cx| panel.set_ui_opacity(effective_ui_opacity));
        self.input_bar
            .update(cx, |bar, _cx| bar.set_ui_opacity(effective_ui_opacity));
        self.sidebar
            .update(cx, |s, cx| s.set_ui_opacity(effective_ui_opacity, cx));
        self.command_palette.update(cx, |palette, _cx| {
            palette.set_ui_opacity(effective_ui_opacity)
        });

        if let Some(new_theme) = TerminalTheme::by_name(&term_config.theme) {
            let theme_changed = new_theme.name != self.terminal_theme.name;
            if theme_changed {
                self.terminal_theme = new_theme.clone();
            }
            if theme_changed || terminal_appearance_changed {
                self.sync_terminal_surface_appearance(&new_theme, window, cx);
            }
            if theme_changed || ui_theme_changed {
                self.sync_gpui_theme_appearance(&new_theme, window, cx);
            }
        } else {
            log::warn!(
                "Skipping terminal theme sync; theme {:?} was not found",
                term_config.theme
            );
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
        self.sync_terminal_surface_appearance(&theme, window, cx);
        self.sync_gpui_theme_appearance(&theme, window, cx);
    }

    fn sync_terminal_surface_appearance(
        &self,
        theme: &TerminalTheme,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let colors = theme_to_ghostty_colors(theme);
        // Update all terminal panes (legacy gets full theme, ghostty gets color scheme)
        for tab in &self.tabs {
            for terminal in tab.pane_tree.all_surface_terminals() {
                terminal.set_theme(
                    theme,
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
            for terminal in tab.pane_tree.all_surface_terminals() {
                terminal.sync_window_background_blur(cx);
            }
        }
        #[cfg(target_os = "windows")]
        crate::set_windows_backdrop_blur(_window, self.terminal_blur);
        #[cfg(target_os = "macos")]
        crate::set_macos_window_glass_backdrop(_window, self.terminal_blur, self.terminal_opacity);
        #[cfg(target_os = "linux")]
        crate::set_linux_window_blur(_window, self.terminal_blur);
    }

    fn sync_gpui_theme_appearance(
        &self,
        theme: &TerminalTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Sync GPUI UI theme colors with terminal theme
        crate::theme::sync_gpui_theme(
            theme,
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
                        let agent_config = self.active_tab_agent_config();
                        if let Some(desc) = self.harness.invoke_skill(
                            session,
                            agent_config,
                            &name,
                            args.as_deref(),
                            context,
                        ) {
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
            self.after_shell_command_recorded(cx);
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
        let duration = Self::terminal_adjacent_chrome_duration(self.agent_panel_open, 290, 220);
        #[cfg(target_os = "macos")]
        if !duration.is_zero() {
            self.arm_chrome_transition_underlay(duration + Duration::from_millis(80));
        }
        #[cfg(target_os = "macos")]
        if duration.is_zero() {
            self.arm_agent_panel_snap_guard(cx);
        }
        self.agent_panel_motion
            .set_target(if self.agent_panel_open { 1.0 } else { 0.0 }, duration);
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
        let duration = Self::terminal_adjacent_chrome_duration(self.input_bar_visible, 210, 160);
        #[cfg(target_os = "macos")]
        if !duration.is_zero() {
            self.arm_chrome_transition_underlay(duration + Duration::from_millis(80));
        }
        #[cfg(target_os = "macos")]
        if duration.is_zero() {
            self.arm_input_bar_snap_guard(cx);
        }
        self.input_bar_motion
            .set_target(if self.input_bar_visible { 1.0 } else { 0.0 }, duration);
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

    fn toggle_vertical_tabs(
        &mut self,
        _: &ToggleVerticalTabs,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = if self.vertical_tabs_active() {
            TabsOrientation::Horizontal
        } else {
            TabsOrientation::Vertical
        };
        self.apply_tabs_orientation(next, true, cx);
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
            let duration = Self::terminal_adjacent_chrome_duration(true, 180, 180);
            #[cfg(target_os = "macos")]
            if duration.is_zero() {
                self.arm_input_bar_snap_guard(cx);
            }
            self.input_bar_motion.set_target(1.0, duration);
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
        self.tabs[self.active_tab].pane_tree.to_state(cx, false)
    }

    fn clear_restored_terminal_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let mut next_config = self.config.clone();
        next_config.appearance.restore_terminal_text = false;
        if let Err(err) = next_config.save() {
            Self::show_layout_profile_error(
                window,
                cx,
                "Could not clear restored terminal history",
                err,
            );
            return;
        }
        self.config = next_config;

        self.settings_panel.update(cx, |panel, cx| {
            panel.set_persisted_restore_terminal_text(false, cx);
        });
        if let Some(panel) = self.settings_window_panel.clone() {
            panel.update(cx, |panel, cx| {
                panel.set_persisted_restore_terminal_text(false, cx);
            });
        }

        self.flush_session_save(cx);
        Self::show_layout_profile_info(
            window,
            cx,
            "Restored terminal history cleared",
            "Terminal text restore is now off. Re-enable it in Settings > General > Continuity."
                .to_string(),
        );
    }

    fn layout_profile_export_root(&self, cx: &App) -> std::path::PathBuf {
        self.active_terminal()
            .current_dir(cx)
            .map(std::path::PathBuf::from)
            .and_then(|path| {
                let path = std::fs::canonicalize(&path).unwrap_or(path);
                find_git_worktree_root(&path).or(Some(path))
            })
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| std::path::PathBuf::from("."))
    }

    fn show_layout_profile_error(
        window: &mut Window,
        cx: &mut Context<Self>,
        message: &str,
        err: impl std::fmt::Display,
    ) {
        let detail = err.to_string();
        let _ = window.prompt(PromptLevel::Critical, message, Some(&detail), &["OK"], cx);
    }

    fn show_layout_profile_info(
        window: &mut Window,
        cx: &mut Context<Self>,
        message: &str,
        detail: String,
    ) {
        let _ = window.prompt(PromptLevel::Info, message, Some(&detail), &["OK"], cx);
    }

    fn save_current_layout_profile_to(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = crate::workspace_layout_root_for_file(&path);
        let session = self.snapshot_session_with_options(cx, false);
        let layout = WorkspaceLayout::from_session(&session, &root);

        if let Err(err) = layout.save(&path) {
            Self::show_layout_profile_error(window, cx, "Could not save layout profile", err);
            return;
        }

        let detail = format!(
            "{}\n\nThe file contains layout intent only: tabs, panes, surfaces, cwd, and agent defaults. It does not include terminal text, command history, conversations, credentials, or commands to run.",
            path.display()
        );
        Self::show_layout_profile_info(window, cx, "Layout profile saved", detail);
        cx.reveal_path(&path);
    }

    fn export_workspace_layout(
        &mut self,
        _: &ExportWorkspaceLayout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let root = self.layout_profile_export_root(cx);
        let export_dir = root.join(".con");
        if let Err(err) = std::fs::create_dir_all(&export_dir) {
            Self::show_layout_profile_error(window, cx, "Could not prepare .con directory", err);
            return;
        }

        let save_path = cx.prompt_for_new_path(&export_dir, Some("workspace.toml"));
        cx.spawn_in(window, async move |this, window| {
            let path = save_path.await.ok()?.ok()??;
            window
                .update(|window, cx| {
                    let _ = this.update(cx, |workspace, cx| {
                        workspace.save_current_layout_profile_to(path, window, cx);
                    });
                })
                .ok()?;
            Some(())
        })
        .detach();
    }

    fn tab_from_state(
        &mut self,
        tab_state: &TabState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Tab {
        let ghostty_app = self.ghostty_app.clone();
        let font_size = self.font_size;
        let restore_terminal_text = self.config.appearance.restore_terminal_text;
        let pane_tree = if let Some(layout) = &tab_state.layout {
            let mut restore_terminal =
                |restore_cwd: Option<&str>,
                 restored_screen_text: Option<&[String]>,
                 force_restored_screen_text: bool| {
                    let restored_screen_text =
                        if restore_terminal_text || force_restored_screen_text {
                            restored_screen_text
                        } else {
                            None
                        };
                    make_ghostty_terminal(
                        &ghostty_app,
                        restore_cwd,
                        restored_screen_text,
                        font_size,
                        window,
                        cx,
                    )
                };
            PaneTree::from_state(layout, tab_state.focused_pane_id, &mut restore_terminal)
        } else {
            PaneTree::new(self.create_terminal(tab_state.cwd.as_deref(), window, cx))
        };
        let summary_id = self.next_tab_summary_id;
        self.next_tab_summary_id += 1;

        Tab {
            pane_tree,
            title: if tab_state.title.trim().is_empty() {
                format!("Terminal {}", summary_id + 1)
            } else {
                tab_state.title.clone()
            },
            user_label: tab_state.user_label.clone(),
            ai_label: None,
            ai_icon: None,
            summary_id,
            needs_attention: false,
            session: AgentSession::new(),
            agent_routing: if tab_state.agent_routing.is_empty() {
                Self::default_agent_routing(self.harness.config())
            } else {
                tab_state.agent_routing.clone()
            },
            panel_state: PanelState::new(),
            runtime_trackers: RefCell::new(HashMap::new()),
            runtime_cache: RefCell::new(HashMap::new()),
            shell_history: Self::restore_shell_history(tab_state),
        }
    }

    fn append_workspace_layout_session(
        &mut self,
        session: Session,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if session.tabs.is_empty() {
            return;
        }

        let old_active = self.active_tab;
        let first_new = self.tabs.len();
        let imported_active = session.active_tab.min(session.tabs.len().saturating_sub(1));

        for tab_state in &session.tabs {
            let tab = self.tab_from_state(tab_state, window, cx);
            self.tabs.push(tab);
        }

        if self.sync_tab_strip_motion() {
            #[cfg(target_os = "macos")]
            self.arm_top_chrome_snap_guard(cx);
        }

        self.active_tab = first_new + imported_active;
        let incoming = std::mem::replace(
            &mut self.tabs[self.active_tab].panel_state,
            PanelState::new(),
        );
        let outgoing = self
            .agent_panel
            .update(cx, |panel, cx| panel.swap_state(incoming, cx));
        if old_active < self.tabs.len() {
            self.tabs[old_active].panel_state = outgoing;
        }

        for (tab_idx, tab) in self.tabs.iter().enumerate() {
            if tab_idx == self.active_tab {
                continue;
            }
            for terminal in tab.pane_tree.all_surface_terminals() {
                terminal.set_focus_state(false, cx);
                terminal.set_native_view_visible(false, cx);
            }
        }

        for terminal in self.tabs[self.active_tab].pane_tree.all_terminals() {
            terminal.ensure_surface(window, cx);
        }
        self.sync_active_tab_native_view_visibility(cx);
        self.tabs[self.active_tab]
            .pane_tree
            .focused_terminal()
            .focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        self.sync_sidebar(cx);
        self.request_tab_summaries(cx);
        self.save_session(cx);
        cx.notify();
    }

    fn add_workspace_layout_tabs_from_path(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match crate::session_from_workspace_layout_path(&path) {
            Ok(session) => {
                let count = session.tabs.len();
                self.append_workspace_layout_session(session, window, cx);
                Self::show_layout_profile_info(
                    window,
                    cx,
                    "Layout profile added",
                    format!(
                        "Added {count} tab{} from {}.",
                        if count == 1 { "" } else { "s" },
                        path.display()
                    ),
                );
            }
            Err(err) => {
                Self::show_layout_profile_error(window, cx, "Could not open layout profile", err);
            }
        }
    }

    fn prompt_for_workspace_layout_path(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        open_in_new_window: bool,
    ) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some("Choose a project folder or Con workspace layout".into()),
        });

        cx.spawn_in(window, async move |this, window| {
            let path = paths.await.ok()?.ok()??.into_iter().next()?;
            window
                .update(|window, cx| {
                    let _ = this.update(cx, |workspace, cx| {
                        if open_in_new_window {
                            match crate::session_from_workspace_layout_path(&path) {
                                Ok(session) => {
                                    let config = Config::load().unwrap_or_default();
                                    crate::open_con_window(config, session, false, cx);
                                }
                                Err(err) => Self::show_layout_profile_error(
                                    window,
                                    cx,
                                    "Could not open layout profile",
                                    err,
                                ),
                            }
                        } else {
                            workspace.add_workspace_layout_tabs_from_path(path, window, cx);
                        }
                    });
                })
                .ok()?;
            Some(())
        })
        .detach();
    }

    fn add_workspace_layout_tabs(
        &mut self,
        _: &AddWorkspaceLayoutTabs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prompt_for_workspace_layout_path(window, cx, false);
    }

    fn open_workspace_layout_window(
        &mut self,
        _: &OpenWorkspaceLayoutWindow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prompt_for_workspace_layout_path(window, cx, true);
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
            theme
                .title_bar
                .opacity(if is_focused { 0.84 } else { 0.72 })
        } else {
            theme
                .background
                .opacity(if is_focused { 0.96 } else { 0.90 })
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
            for t in tab.pane_tree.all_surface_terminals() {
                t.set_focus_state(false, cx);
                t.set_native_view_visible(false, cx);
            }
        }
        self.tabs.clear();
        cx.quit();
    }

    fn focus_input(&mut self, _: &FocusInput, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_modal_open(cx) {
            return;
        }

        if self.is_input_surface_focused(window, cx) {
            self.focus_first_terminal(window, cx);
            return;
        }

        self.focus_preferred_input_surface(window, cx);
    }

    fn is_input_surface_focused(&self, window: &Window, cx: &App) -> bool {
        let input_bar_focused =
            self.input_bar_visible && self.input_bar.focus_handle(cx).is_focused(window);
        let agent_inline_focused = self.agent_panel_open
            && self
                .agent_panel
                .read(cx)
                .inline_input_is_focused(window, cx);

        input_bar_focused || agent_inline_focused
    }

    fn focus_preferred_input_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.input_bar_visible {
            if self.agent_panel_open {
                let focused_inline = self
                    .agent_panel
                    .update(cx, |panel, cx| panel.focus_inline_input(window, cx));
                if !focused_inline {
                    self.focus_agent_inline_input_next_frame(window, cx);
                }
                return;
            }
        }
        self.focus_input_bar_surface(window, cx);
    }

    fn focus_input_bar_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.input_bar_visible {
            self.input_bar_visible = true;
            self.save_session(cx);
        }
        let duration = Self::terminal_adjacent_chrome_duration(true, 180, 180);
        #[cfg(target_os = "macos")]
        if duration.is_zero() {
            self.arm_input_bar_snap_guard(cx);
        }
        self.input_bar_motion.set_target(1.0, duration);
        self.input_bar.focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn focus_first_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_active_tab() {
            return;
        }

        self.pane_scope_picker_open = false;
        let (pane_id, terminal) = {
            let pane_tree = &self.tabs[self.active_tab].pane_tree;
            let (pane_id, terminal) = pane_tree.visible_focus_terminal();
            (pane_id, terminal.clone())
        };
        self.tabs[self.active_tab].pane_tree.focus(pane_id);
        terminal.focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        cx.notify();
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
        if self.settings_panel.read(cx).is_overlay_visible() {
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

    fn open_settings_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.settings_window {
            if handle
                .update(cx, |_, settings_window, _| {
                    settings_window.activate_window();
                })
                .is_ok()
            {
                return;
            }
            self.settings_window = None;
            self.settings_window_panel = None;
        }

        let config = self.config.clone();
        let registry = self.model_registry.clone();
        let runtime = self.harness.runtime_handle();
        let workspace = cx.weak_entity();
        let main_window = window.window_handle();
        let opened_panel: Rc<RefCell<Option<Entity<SettingsPanel>>>> = Rc::new(RefCell::new(None));
        let opened_panel_for_window = opened_panel.clone();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(920.0), px(680.0)), cx)),
            titlebar: Some(TitlebarOptions {
                title: Some("Settings".into()),
                appears_transparent: false,
                ..Default::default()
            }),
            window_background: WindowBackgroundAppearance::Opaque,
            ..Default::default()
        };

        match cx.open_window(options, move |settings_window, cx| {
            let panel = cx.new(|cx| {
                let mut panel =
                    SettingsPanel::new(&config, registry.clone(), runtime, settings_window, cx);
                panel.open_standalone(settings_window, cx);
                panel
            });
            *opened_panel_for_window.borrow_mut() = Some(panel.clone());

            let workspace_for_save = workspace.clone();
            let main_window_for_save = main_window;
            cx.subscribe(&panel, move |settings, _: &SaveSettings, cx| {
                let _ = main_window_for_save.update(cx, |_, window, cx| {
                    let _ = workspace_for_save.update(cx, |workspace, cx| {
                        workspace.apply_settings_from_panel(&settings, window, cx, false);
                    });
                });
            })
            .detach();

            let workspace_for_tabs = workspace.clone();
            let main_window_for_tabs = main_window;
            cx.subscribe(&panel, move |settings, _: &TabsOrientationChanged, cx| {
                let _ = main_window_for_tabs.update(cx, |_, _window, cx| {
                    let _ = workspace_for_tabs.update(cx, |workspace, cx| {
                        workspace.apply_tabs_orientation_from_panel(&settings, cx);
                    });
                });
            })
            .detach();

            let workspace_for_theme = workspace.clone();
            let main_window_for_theme = main_window;
            cx.subscribe(&panel, move |_settings, event: &ThemePreview, cx| {
                let theme_name = event.0.clone();
                let _ = main_window_for_theme.update(cx, |_, window, cx| {
                    let _ = workspace_for_theme.update(cx, |workspace, cx| {
                        workspace.apply_theme_preview(&theme_name, window, cx);
                    });
                });
            })
            .detach();

            let workspace_for_appearance = workspace.clone();
            let main_window_for_appearance = main_window;
            cx.subscribe(&panel, move |settings, _: &AppearancePreview, cx| {
                let _ = main_window_for_appearance.update(cx, |_, window, cx| {
                    let _ = workspace_for_appearance.update(cx, |workspace, cx| {
                        workspace.apply_appearance_preview_from_panel(&settings, window, cx);
                    });
                });
            })
            .detach();

            let panel_for_close = panel.clone();
            let workspace_for_close = workspace.clone();
            let main_window_for_close = main_window;
            settings_window.on_window_should_close(cx, move |_window, cx| {
                let _ = panel_for_close.update(cx, |panel, cx| {
                    panel.revert_standalone_preview(cx);
                });
                let _ = main_window_for_close.update(cx, |_, _window, cx| {
                    let _ = workspace_for_close.update(cx, |workspace, _cx| {
                        workspace.settings_window = None;
                        workspace.settings_window_panel = None;
                    });
                });
                true
            });

            let view = cx.new(|_| SettingsWindowView {
                panel: panel.clone(),
            });
            cx.new(|cx| {
                gpui_component::Root::new(view, settings_window, cx).bg(cx.theme().background)
            })
        }) {
            Ok(handle) => {
                self.settings_window = Some(handle.into());
                self.settings_window_panel = opened_panel.borrow().clone();
            }
            Err(err) => {
                log::error!("Failed to open settings window: {err}");
            }
        }
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
        self.open_settings_window(window, cx);
        cx.notify();
    }

    fn is_modal_open(&self, cx: &App) -> bool {
        self.settings_panel.read(cx).is_overlay_visible()
            || self.command_palette.read(cx).is_visible()
    }

    fn new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        let terminal = self.create_terminal(None, window, cx);
        let tab_number = self.tabs.len() + 1;
        let summary_id = self.next_tab_summary_id;
        self.next_tab_summary_id += 1;

        self.tabs.push(Tab {
            pane_tree: PaneTree::new(terminal),
            title: format!("Terminal {}", tab_number),
            user_label: None,
            ai_label: None,
            ai_icon: None,
            summary_id,
            needs_attention: false,
            session: AgentSession::new(),
            agent_routing: Self::default_agent_routing(self.harness.config()),
            panel_state: PanelState::new(),
            runtime_trackers: RefCell::new(HashMap::new()),
            runtime_cache: RefCell::new(HashMap::new()),
            shell_history: HashMap::new(),
        });

        if self.sync_tab_strip_motion() {
            #[cfg(target_os = "macos")]
            self.arm_top_chrome_snap_guard(cx);
            if Self::should_defer_top_chrome_refresh_when_tab_strip_appears() {
                cx.on_next_frame(window, |_, _, cx| {
                    cx.notify();
                });
            }
        }

        let new_index = self.tabs.len() - 1;
        self.activate_tab(new_index, window, cx);
    }

    fn focus_agent_inline_input_next_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.on_next_frame(window, |workspace, window, cx| {
            if !workspace.agent_panel_open || workspace.input_bar_visible {
                return;
            }
            let focused = workspace
                .agent_panel
                .update(cx, |panel, cx| panel.focus_inline_input(window, cx));
            if !focused {
                workspace.focus_input_bar_surface(window, cx);
            }
        });
    }

    pub fn mark_as_quick_terminal(&mut self) {
        self.is_quick_terminal = true;
    }

    /// Reinitialize the quick terminal with a fresh tab and hide the window.
    /// Called when the last tab is closed via Cmd+W or when the shell in the
    /// last pane exits (Ctrl+D). The quick terminal must never be fully
    /// removed while the app is running.
    fn reinitialize_quick_terminal_and_hide(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = self.active_tab;
        let closing_terminals: Vec<TerminalPane> = self.tabs[index]
            .pane_tree
            .all_surface_terminals()
            .into_iter()
            .cloned()
            .collect();
        let _summary_id = self.tabs[index].summary_id;
        self.tab_summary_engine.forget(_summary_id);
        self.tabs.remove(index);

        let terminal = self.create_terminal(None, window, cx);
        let summary_id = self.next_tab_summary_id;
        self.next_tab_summary_id += 1;
        self.tabs.push(Tab {
            pane_tree: PaneTree::new(terminal),
            title: "Terminal 1".to_string(),
            user_label: None,
            ai_label: None,
            ai_icon: None,
            summary_id,
            needs_attention: false,
            session: AgentSession::new(),
            agent_routing: Self::default_agent_routing(self.harness.config()),
            panel_state: PanelState::new(),
            runtime_trackers: RefCell::new(HashMap::new()),
            runtime_cache: RefCell::new(HashMap::new()),
            shell_history: HashMap::new(),
        });
        self.active_tab = 0;
        self.save_session(cx);
        cx.notify();

        cx.on_next_frame(window, move |_workspace, _window, cx| {
            for terminal in &closing_terminals {
                terminal.shutdown_surface(cx);
            }
        });

        #[cfg(target_os = "macos")]
        crate::quick_terminal::hide();
    }

    fn close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        // If the active tab has multiple panes, close the focused pane first.
        // Only close the entire tab when it's down to a single pane.
        if self.tabs[self.active_tab].pane_tree.pane_count() > 1 {
            let (closing_terminals, surviving_terminals, new_focus) = {
                let tab = &mut self.tabs[self.active_tab];
                let pane_id = tab.pane_tree.focused_pane_id();
                let closing_terminals = tab
                    .pane_tree
                    .surface_infos(Some(pane_id))
                    .into_iter()
                    .map(|surface| surface.terminal)
                    .collect::<Vec<_>>();
                tab.pane_tree.close_focused();
                let surviving_terminals: Vec<TerminalPane> =
                    tab.pane_tree.all_terminals().into_iter().cloned().collect();
                let new_focus = tab.pane_tree.focused_terminal().clone();
                (closing_terminals, surviving_terminals, new_focus)
            };
            #[cfg(target_os = "macos")]
            self.mark_active_tab_terminal_native_layout_pending(cx);

            for terminal in &surviving_terminals {
                terminal.ensure_surface(window, cx);
                terminal.notify(cx);
            }
            self.sync_active_tab_native_view_visibility(cx);

            new_focus.focus(window, cx);
            self.sync_active_terminal_focus_states(cx);
            cx.on_next_frame(window, move |_workspace, _window, cx| {
                for terminal in &closing_terminals {
                    terminal.shutdown_surface(cx);
                }
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
            if self.is_quick_terminal {
                self.reinitialize_quick_terminal_and_hide(window, cx);
                return;
            }

            self.close_window_from_last_tab(window, cx);
            return;
        }
        let closing_terminals: Vec<TerminalPane> = self.tabs[index]
            .pane_tree
            .all_surface_terminals()
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
        self.reindex_pending_surface_control_requests_after_tab_close(index);
        let removed = self.tabs.remove(index);
        // Drop the closed tab's cached AI summary so a future tab
        // assigned the same summary_id (which won't happen, since
        // ids are monotonic) doesn't inherit stale state.
        self.tab_summary_engine.forget(removed.summary_id);
        if self.sync_tab_strip_motion() {
            #[cfg(target_os = "macos")]
            self.arm_top_chrome_snap_guard(cx);
        }
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
        for terminal in self.tabs[self.active_tab].pane_tree.all_terminals() {
            terminal.ensure_surface(window, cx);
        }
        self.sync_active_tab_native_view_visibility(cx);
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
            let should_quit = cfg!(not(target_os = "macos")) && cx.windows().len() <= 1;
            window.remove_window();
            if should_quit {
                cx.quit();
            }
        });
    }

    fn prepare_window_close(&mut self, cx: &mut Context<Self>) {
        if self.window_close_prepared {
            return;
        }
        self.window_close_prepared = true;

        self.cancel_all_sessions();
        self.flush_session_save(cx);

        if let Some(settings_window) = self.settings_window.take() {
            self.settings_window_panel = None;
            let _ = settings_window.update(cx, |_, window, _| {
                window.remove_window();
            });
        }

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

        for request in std::mem::take(&mut self.pending_surface_control_requests) {
            let response_tx = match request {
                PendingSurfaceControlRequest::Create { response_tx, .. }
                | PendingSurfaceControlRequest::Split { response_tx, .. }
                | PendingSurfaceControlRequest::Close { response_tx, .. } => response_tx,
            };
            Self::send_control_result(
                response_tx,
                Err(ControlError::internal(
                    "window closed while a surface control request was pending".to_string(),
                )),
            );
        }

        for tab in &self.tabs {
            let conv = tab.session.conversation();
            let _ = conv.lock().save();

            for terminal in tab.pane_tree.all_surface_terminals() {
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

    fn reindex_pending_surface_control_requests_after_tab_close(&mut self, closed_tab_idx: usize) {
        let mut shifted = Vec::new();
        for mut request in std::mem::take(&mut self.pending_surface_control_requests) {
            let tab_idx = match &mut request {
                PendingSurfaceControlRequest::Create { tab_idx, .. }
                | PendingSurfaceControlRequest::Split { tab_idx, .. }
                | PendingSurfaceControlRequest::Close { tab_idx, .. } => tab_idx,
            };

            if *tab_idx == closed_tab_idx {
                let (method, response_tx) = match request {
                    PendingSurfaceControlRequest::Create { response_tx, .. } => {
                        ("surfaces.create", response_tx)
                    }
                    PendingSurfaceControlRequest::Split { response_tx, .. } => {
                        ("surfaces.split", response_tx)
                    }
                    PendingSurfaceControlRequest::Close { response_tx, .. } => {
                        ("surfaces.close", response_tx)
                    }
                };
                Self::send_control_result(
                    response_tx,
                    Err(ControlError::internal(format!(
                        "Tab {} was closed while {method} was pending",
                        closed_tab_idx + 1
                    ))),
                );
                continue;
            }

            if *tab_idx > closed_tab_idx {
                *tab_idx -= 1;
            }
            shifted.push(request);
        }
        self.pending_surface_control_requests = shifted;
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

    /// Public-ish hook so the workspace can re-ask the AI summarizer
    /// after recording a new shell command. Separate from the
    /// `&mut self` mutation above so the borrow checker is happy.
    fn after_shell_command_recorded(&self, cx: &App) {
        self.request_tab_summaries(cx);
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
        let is_remote = pane.as_ref().is_some_and(|terminal| {
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

        if matches!(mode, InputMode::Agent)
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

        let input_changed = text != result.prefix;

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

        if input_changed {
            let full_suggestion = if result.completion.starts_with(&result.prefix) {
                result.completion.clone()
            } else {
                format!("{}{}", result.prefix, result.completion)
            };

            if !full_suggestion.starts_with(&text) || full_suggestion == text {
                log::debug!(
                    target: "con::suggestions",
                    "drop ai suggestion prefix={:?}: text changed to incompatible prefix {:?}",
                    result.prefix,
                    text
                );
                return;
            }

            log::debug!(
                target: "con::suggestions",
                "apply ai suggestion prefix={:?} current={:?} completion={:?}",
                result.prefix,
                text,
                full_suggestion
            );
            self.input_bar.update(cx, |bar, _cx| {
                bar.set_ai_inline_suggestion(&text, &full_suggestion);
            });
            cx.notify();
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

    /// Result delivered by [`TabSummaryEngine`] — locate the tab by
    /// `summary_id` (NOT index, since reorders / closes shift the
    /// indexes), update its `ai_label` / `ai_icon`, and republish to
    /// the sidebar.
    fn apply_tab_summary(&mut self, generation: u64, summary: TabSummary, cx: &mut Context<Self>) {
        if generation != self.tab_summary_generation
            || !self.vertical_tabs_active()
            || !self.harness.config().suggestion_model.enabled
        {
            return;
        }
        let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|t| t.summary_id == summary.tab_id)
        else {
            // Tab was closed while the request was in flight.
            return;
        };
        let label = summary.label.trim().to_string();
        let icon = summary.icon;
        let label_changed = tab.ai_label.as_deref() != Some(label.as_str());
        let icon_changed = tab.ai_icon != Some(icon);
        if !label_changed && !icon_changed {
            return;
        }
        tab.ai_label = Some(label);
        tab.ai_icon = Some(icon);
        log::debug!(
            target: "con::tab_summary",
            "tab_summary applied tab_id={} label={:?} icon={:?}",
            summary.tab_id,
            tab.ai_label,
            tab.ai_icon,
        );
        self.sync_sidebar(cx);
        cx.notify();
    }

    #[cfg(test)]
    fn new_tab_sync_policy_for_tests() -> NewTabSyncPolicy {
        NewTabSyncPolicy {
            activates_new_tab: true,
            syncs_sidebar: true,
            notifies_ui: true,
            syncs_native_visibility: true,
            reuses_shared_tab_activation_flow: true,
        }
    }

    fn should_defer_top_chrome_refresh_when_tab_strip_appears() -> bool {
        true
    }

    #[cfg(test)]
    fn should_defer_top_chrome_refresh_when_tab_strip_appears_for_tests() -> bool {
        Self::should_defer_top_chrome_refresh_when_tab_strip_appears()
    }

    fn request_tab_summaries(&self, cx: &App) {
        if !self.vertical_tabs_active() || !self.harness.config().suggestion_model.enabled {
            return;
        }
        let tx = self.tab_summary_tx.clone();
        let generation = self.tab_summary_generation;
        for (i, tab) in self.tabs.iter().enumerate() {
            let terminal = tab.pane_tree.focused_terminal();
            // The terminal's recent scrollback is the only signal we
            // get for commands the user typed *directly* into the
            // pane (Con doesn't intercept those into shell_history).
            // Pull a small tail and pass it to the model — same lines
            // the user can see right now.
            let recent_output = terminal.recent_lines(24, cx);
            let req = TabSummaryRequest {
                tab_id: tab.summary_id,
                cwd: terminal.current_dir(cx),
                title: terminal.title(cx),
                ssh_host: self.effective_remote_host_for_tab(i, terminal, cx),
                recent_commands: {
                    let mut histories: Vec<_> = tab.shell_history.iter().collect();
                    histories.sort_by_key(|(pane_id, _)| *pane_id);
                    histories
                        .into_iter()
                        .flat_map(|(_, q)| q.iter().rev())
                        .map(|entry| entry.command.clone())
                        .take(8)
                        .collect()
                },
                recent_output,
            };
            let tx = tx.clone();
            self.tab_summary_engine.request(req, move |summary| {
                let _ = tx.send((generation, summary));
            });
        }
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
        self.after_shell_command_recorded(cx);

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
        let agent_config = self.active_tab_agent_config();
        self.harness
            .send_message(session, agent_config, content.to_string(), context);
        self.save_session(cx);
    }

    fn create_surface_in_focused_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_active_tab() {
            return;
        }

        let tab_idx = self.active_tab;
        let pane_id = self.tabs[tab_idx].pane_tree.focused_pane_id();
        let cwd = self.tabs[tab_idx]
            .pane_tree
            .focused_terminal()
            .current_dir(cx);
        let terminal = self.create_terminal(cwd.as_deref(), window, cx);
        let next_surface_index = self.tabs[tab_idx]
            .pane_tree
            .surface_infos(Some(pane_id))
            .len()
            .saturating_add(1);
        let options = SurfaceCreateOptions::plain(Some(format!("Surface {next_surface_index}")));
        let Some(_surface_id) =
            self.tabs[tab_idx]
                .pane_tree
                .create_surface_in_pane(pane_id, terminal.clone(), options)
        else {
            return;
        };

        #[cfg(target_os = "macos")]
        self.mark_tab_terminal_native_layout_pending(tab_idx, cx);
        self.notify_tab_terminal_views(tab_idx, cx);
        terminal.ensure_surface(window, cx);
        terminal.focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        self.sync_active_tab_native_view_visibility_now_or_after_layout(false, window, cx);
        Self::schedule_terminal_bootstrap_reassert(
            &terminal,
            true,
            self.window_handle,
            self.workspace_handle.clone(),
            cx,
        );
        self.record_runtime_event_for_terminal(
            tab_idx,
            &terminal,
            con_agent::context::PaneRuntimeEvent::PaneCreated {
                startup_command: None,
            },
        );
        self.sync_sidebar(cx);
        self.save_session(cx);
        cx.notify();
    }

    fn create_surface_split_from_focused_pane(
        &mut self,
        direction: SplitDirection,
        placement: SplitPlacement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }

        let tab_idx = self.active_tab;
        let source_pane_id = self.tabs[tab_idx].pane_tree.focused_pane_id();
        let cwd = self.tabs[tab_idx]
            .pane_tree
            .focused_terminal()
            .current_dir(cx);
        let terminal = self.create_terminal(cwd.as_deref(), window, cx);
        let was_zoomed = self.tabs[tab_idx].pane_tree.zoomed_pane_id().is_some();
        let options = SurfaceCreateOptions {
            title: Some("Surface 1".to_string()),
            owner: Some("command-palette".to_string()),
            close_pane_when_last: true,
        };
        let Some((_pane_id, _surface_id)) = self.tabs[tab_idx]
            .pane_tree
            .split_pane_with_surface_options(
                source_pane_id,
                direction,
                placement,
                terminal.clone(),
                options,
            )
        else {
            return;
        };

        #[cfg(target_os = "macos")]
        self.mark_tab_terminal_native_layout_pending(tab_idx, cx);
        self.notify_tab_terminal_views(tab_idx, cx);
        terminal.ensure_surface(window, cx);
        terminal.focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        self.sync_active_tab_native_view_visibility_now_or_after_layout(was_zoomed, window, cx);
        Self::schedule_terminal_bootstrap_reassert(
            &terminal,
            true,
            self.window_handle,
            self.workspace_handle.clone(),
            cx,
        );
        self.record_runtime_event_for_terminal(
            tab_idx,
            &terminal,
            con_agent::context::PaneRuntimeEvent::PaneCreated {
                startup_command: None,
            },
        );
        self.sync_sidebar(cx);
        self.save_session(cx);
        cx.notify();
    }

    fn cycle_surface_in_focused_pane(
        &mut self,
        offset: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }

        let pane_tree = &self.tabs[self.active_tab].pane_tree;
        let pane_id = pane_tree.focused_pane_id();
        let surfaces = pane_tree.surface_infos(Some(pane_id));
        if surfaces.len() <= 1 {
            return;
        }

        let active_index = surfaces
            .iter()
            .position(|surface| surface.is_active)
            .unwrap_or(0);
        let len = surfaces.len() as isize;
        let next_index = (active_index as isize + offset).rem_euclid(len) as usize;
        let next_surface_id = surfaces[next_index].surface_id;
        self.focus_surface_in_active_tab(next_surface_id, window, cx);
    }

    fn focused_active_surface_for_rename(&self, cx: &App) -> Option<(usize, usize, String)> {
        let tab_idx = self.active_tab;
        let tab = self.tabs.get(tab_idx)?;
        let pane_id = tab.pane_tree.focused_pane_id();
        let surface = tab
            .pane_tree
            .surface_infos(Some(pane_id))
            .into_iter()
            .find(|surface| surface.is_active)?;
        let title = surface
            .title
            .clone()
            .or_else(|| surface.terminal.title(cx))
            .unwrap_or_else(|| format!("Surface {}", surface.surface_index + 1));

        Some((tab_idx, surface.surface_id, title))
    }

    fn rename_surface_title(
        &mut self,
        tab_idx: usize,
        surface_id: usize,
        value: String,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab) = self.tabs.get_mut(tab_idx) else {
            return false;
        };
        let value = value.trim();
        let title = if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        };

        if !tab.pane_tree.rename_surface(surface_id, title) {
            return false;
        }

        self.sync_sidebar(cx);
        self.save_session(cx);
        cx.notify();
        true
    }

    fn begin_surface_rename(
        &mut self,
        surface_id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }

        let tab_idx = self.active_tab;
        let Some(surface) = self.tabs[tab_idx]
            .pane_tree
            .surface_infos(None)
            .into_iter()
            .find(|surface| surface.surface_id == surface_id)
        else {
            return;
        };
        let initial = surface
            .title
            .clone()
            .unwrap_or_else(|| format!("Surface {}", surface.surface_index + 1));

        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(&initial, window, cx);
            state.set_placeholder("Surface name", window, cx);
            state
        });

        cx.subscribe_in(&input, window, {
            move |this, input_entity, event: &InputEvent, _window, cx| {
                if !matches!(event, InputEvent::PressEnter { .. }) {
                    return;
                }
                let value = input_entity.read(cx).value().to_string();
                this.rename_surface_title(tab_idx, surface_id, value, cx);
                this.surface_rename = None;
                cx.notify();
            }
        })
        .detach();

        self.surface_rename = Some(SurfaceRenameEditor {
            surface_id,
            input: input.clone(),
        });
        input.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    fn close_surface_by_id_in_active_tab(
        &mut self,
        surface_id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }

        let tab_idx = self.active_tab;
        if self
            .surface_rename
            .as_ref()
            .is_some_and(|editor| editor.surface_id == surface_id)
        {
            self.surface_rename = None;
        }
        let Some(close_outcome) = self.tabs[tab_idx].pane_tree.close_surface(surface_id, true)
        else {
            return;
        };
        let closing = close_outcome.terminal.clone();
        closing.set_focus_state(false, cx);
        closing.set_native_view_visible(false, cx);
        closing.shutdown_surface(cx);

        #[cfg(target_os = "macos")]
        self.mark_tab_terminal_native_layout_pending(tab_idx, cx);
        self.notify_tab_terminal_views(tab_idx, cx);
        self.sync_active_tab_native_view_visibility(cx);
        let replacement = self.tabs[tab_idx].pane_tree.focused_terminal().clone();
        replacement.ensure_surface(window, cx);
        replacement.focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        self.sync_sidebar(cx);
        self.save_session(cx);
        cx.notify();
    }

    fn close_current_surface_in_focused_pane(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }

        let tab_idx = self.active_tab;
        let pane_id = self.tabs[tab_idx].pane_tree.focused_pane_id();
        let Some(surface_id) = self.tabs[tab_idx]
            .pane_tree
            .surface_infos(Some(pane_id))
            .into_iter()
            .find(|surface| surface.is_active)
            .map(|surface| surface.surface_id)
        else {
            return;
        };

        self.close_surface_by_id_in_active_tab(surface_id, window, cx);
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
        let was_zoomed = self.tabs[self.active_tab]
            .pane_tree
            .zoomed_pane_id()
            .is_some();
        self.tabs[self.active_tab].pane_tree.split_with_placement(
            direction,
            placement,
            terminal.clone(),
        );
        #[cfg(target_os = "macos")]
        self.mark_active_tab_terminal_native_layout_pending(cx);
        self.record_runtime_event_for_terminal(
            self.active_tab,
            &terminal,
            con_agent::context::PaneRuntimeEvent::PaneCreated {
                startup_command: None,
            },
        );
        self.notify_active_tab_terminal_views(cx);
        terminal.focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        self.sync_active_tab_native_view_visibility_now_or_after_layout(was_zoomed, window, cx);
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
            self.split_pane(SplitDirection::Vertical, SplitPlacement::After, window, cx);
        }
    }

    fn split_left(&mut self, _: &SplitLeft, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_active_tab() {
            self.split_pane(
                SplitDirection::Horizontal,
                SplitPlacement::Before,
                window,
                cx,
            );
        }
    }

    fn split_up(&mut self, _: &SplitUp, window: &mut Window, cx: &mut Context<Self>) {
        if self.has_active_tab() {
            self.split_pane(SplitDirection::Vertical, SplitPlacement::Before, window, cx);
        }
    }

    fn clear_terminal(&mut self, _: &ClearTerminal, _window: &mut Window, cx: &mut Context<Self>) {
        if self.has_active_tab() {
            self.active_terminal().clear_scrollback(cx);
        }
    }

    fn clear_restored_terminal_history_action(
        &mut self,
        _: &ClearRestoredTerminalHistory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_restored_terminal_history(window, cx);
    }

    fn new_surface(&mut self, _: &NewSurface, window: &mut Window, cx: &mut Context<Self>) {
        self.create_surface_in_focused_pane(window, cx);
    }

    fn new_surface_split_right(
        &mut self,
        _: &NewSurfaceSplitRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.create_surface_split_from_focused_pane(
            SplitDirection::Horizontal,
            SplitPlacement::After,
            window,
            cx,
        );
    }

    fn new_surface_split_down(
        &mut self,
        _: &NewSurfaceSplitDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.create_surface_split_from_focused_pane(
            SplitDirection::Vertical,
            SplitPlacement::After,
            window,
            cx,
        );
    }

    fn next_surface(&mut self, _: &NextSurface, window: &mut Window, cx: &mut Context<Self>) {
        self.cycle_surface_in_focused_pane(1, window, cx);
    }

    fn previous_surface(
        &mut self,
        _: &PreviousSurface,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cycle_surface_in_focused_pane(-1, window, cx);
    }

    fn rename_current_surface(
        &mut self,
        _: &RenameSurface,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_tab_idx, surface_id, _title)) = self.focused_active_surface_for_rename(cx)
        else {
            return;
        };
        self.begin_surface_rename(surface_id, window, cx);
    }

    fn close_surface(&mut self, _: &CloseSurface, window: &mut Window, cx: &mut Context<Self>) {
        self.close_current_surface_in_focused_pane(window, cx);
    }

    fn close_pane(&mut self, _: &ClosePane, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_active_tab() {
            return;
        }

        if self.tabs[self.active_tab].pane_tree.pane_count() > 1 {
            let pane_id = self.tabs[self.active_tab].pane_tree.focused_pane_id();
            let _ = self.close_pane_in_tab(self.active_tab, pane_id, window, cx);
            return;
        }

        if self.tabs.len() > 1 {
            self.close_tab_by_index(self.active_tab, window, cx);
            return;
        }

        self.close_window_from_last_tab(window, cx);
    }

    fn toggle_pane_zoom(
        &mut self,
        _: &TogglePaneZoom,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }

        let was_zoomed = self.tabs[self.active_tab]
            .pane_tree
            .zoomed_pane_id()
            .is_some();
        if !self.tabs[self.active_tab].pane_tree.toggle_zoom_focused() {
            return;
        }

        #[cfg(target_os = "macos")]
        self.mark_active_tab_terminal_native_layout_pending(cx);
        self.notify_active_tab_terminal_views(cx);
        self.active_terminal().focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        self.sync_active_tab_native_view_visibility_now_or_after_layout(was_zoomed, window, cx);
        cx.notify();
    }

    fn close_pane_in_tab(
        &mut self,
        tab_idx: usize,
        pane_id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if tab_idx >= self.tabs.len() || self.tabs[tab_idx].pane_tree.pane_count() <= 1 {
            return false;
        }

        let mut focus_after_close = None;
        let mut sync_visibility_after_close = false;
        {
            let pane_tree = &mut self.tabs[tab_idx].pane_tree;
            let closing_terminals = pane_tree
                .surface_infos(Some(pane_id))
                .into_iter()
                .map(|surface| surface.terminal)
                .collect::<Vec<_>>();

            if closing_terminals.is_empty() || !pane_tree.close_pane(pane_id) {
                return false;
            }

            let surviving_terminals: Vec<TerminalPane> =
                pane_tree.all_terminals().into_iter().cloned().collect();
            let tab_is_visible = tab_idx == self.active_tab;
            if tab_is_visible {
                for terminal in &surviving_terminals {
                    terminal.ensure_surface(window, cx);
                    terminal.notify(cx);
                }
                sync_visibility_after_close = true;
            }

            cx.on_next_frame(window, move |_workspace, _window, cx| {
                for terminal in &closing_terminals {
                    terminal.shutdown_surface(cx);
                }
                if tab_is_visible {
                    for terminal in &surviving_terminals {
                        terminal.notify(cx);
                    }
                }
            });

            if tab_idx == self.active_tab {
                focus_after_close = Some(pane_tree.focused_terminal().clone());
            }
        }
        if tab_idx == self.active_tab {
            #[cfg(target_os = "macos")]
            self.mark_active_tab_terminal_native_layout_pending(cx);
        }

        if let Some(focused) = focus_after_close {
            if sync_visibility_after_close {
                self.sync_active_tab_native_view_visibility(cx);
            }
            focused.focus(window, cx);
            self.sync_active_terminal_focus_states(cx);
        }

        self.save_session(cx);
        cx.notify();
        true
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
            terminal.ensure_surface(window, cx);
        }
        self.sync_tab_native_view_visibility(index, true, cx);
        for terminal in self.tabs[old_active].pane_tree.all_surface_terminals() {
            terminal.set_focus_state(false, cx);
        }
        let old_terminals: Vec<TerminalPane> = self.tabs[old_active]
            .pane_tree
            .all_surface_terminals()
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

        self.sync_sidebar(cx);
        // Activating a tab is a strong signal the user cares about
        // it — refresh AI label/icon if context shifted since it was
        // last summarized.
        self.request_tab_summaries(cx);
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

    fn select_tab_index(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() || index == self.active_tab {
            return;
        }
        self.activate_tab(index, window, cx);
    }

    fn select_tab_1(&mut self, _: &SelectTab1, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_index(0, window, cx);
    }

    fn select_tab_2(&mut self, _: &SelectTab2, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_index(1, window, cx);
    }

    fn select_tab_3(&mut self, _: &SelectTab3, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_index(2, window, cx);
    }

    fn select_tab_4(&mut self, _: &SelectTab4, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_index(3, window, cx);
    }

    fn select_tab_5(&mut self, _: &SelectTab5, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_index(4, window, cx);
    }

    fn select_tab_6(&mut self, _: &SelectTab6, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_index(5, window, cx);
    }

    fn select_tab_7(&mut self, _: &SelectTab7, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_index(6, window, cx);
    }

    fn select_tab_8(&mut self, _: &SelectTab8, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_index(7, window, cx);
    }

    fn select_tab_9(&mut self, _: &SelectTab9, window: &mut Window, cx: &mut Context<Self>) {
        self.select_tab_index(8, window, cx);
    }

    /// Focus the active terminal (used after modal close, etc.)
    fn focus_terminal(&self, window: &mut Window, cx: &mut App) {
        self.active_terminal().focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
    }

    fn focus_surface_in_active_tab(
        &mut self,
        surface_id: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.has_active_tab() {
            return;
        }
        let changed = self.tabs[self.active_tab]
            .pane_tree
            .focus_surface(surface_id);
        if !changed {
            return;
        }
        let terminal = self.tabs[self.active_tab]
            .pane_tree
            .focused_terminal()
            .clone();
        terminal.ensure_surface(window, cx);
        self.sync_active_tab_native_view_visibility(cx);
        self.sync_sidebar(cx);
        terminal.focus(window, cx);
        self.sync_active_terminal_focus_states(cx);
        self.save_session(cx);
        cx.notify();
    }

    fn restore_terminal_focus_after_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.has_active_tab() {
            return;
        }
        self.focus_terminal(window, cx);
        cx.on_next_frame(window, |workspace, window, cx| {
            if workspace.has_active_tab() && !workspace.settings_panel.read(cx).is_overlay_visible()
            {
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
            self.sync_active_tab_native_view_visibility(cx);
        } else {
            // Hide all tabs' views for modal z-order
            for tab in &self.tabs {
                for terminal in tab.pane_tree.all_surface_terminals() {
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
        // Find which tab contains the dead terminal surface (may not be the active tab).
        let entity_id = entity.entity_id();
        let tab_idx = self
            .tabs
            .iter()
            .position(|tab| tab.pane_tree.pane_id_for_entity(entity_id).is_some());
        let Some(tab_idx) = tab_idx else { return };

        let surface = self.tabs[tab_idx]
            .pane_tree
            .surface_infos(None)
            .into_iter()
            .find(|surface| surface.terminal.entity_id() == entity_id);
        let Some(surface) = surface else { return };
        self.record_runtime_event_for_terminal(
            tab_idx,
            &surface.terminal,
            con_agent::context::PaneRuntimeEvent::ProcessExited,
        );

        let pane_surface_count = self.tabs[tab_idx]
            .pane_tree
            .surface_infos(Some(surface.pane_id))
            .len();
        if pane_surface_count > 1 {
            let should_focus_replacement =
                tab_idx == self.active_tab && surface.is_active && surface.is_focused_pane;
            if let Some(outcome) = self.tabs[tab_idx]
                .pane_tree
                .close_surface(surface.surface_id, false)
            {
                let terminal = outcome.terminal;
                terminal.set_native_view_visible(false, cx);
                terminal.shutdown_surface(cx);
            }
            if tab_idx == self.active_tab {
                self.sync_active_tab_native_view_visibility(cx);
                if should_focus_replacement {
                    let replacement = self.tabs[tab_idx].pane_tree.focused_terminal().clone();
                    replacement.ensure_surface(window, cx);
                    replacement.focus(window, cx);
                }
                self.sync_active_terminal_focus_states(cx);
            }
            self.save_session(cx);
            cx.notify();
            return;
        }

        if self.tabs[tab_idx].pane_tree.pane_count() > 1 {
            let _ = self.close_pane_in_tab(tab_idx, surface.pane_id, window, cx);
        } else if self.tabs.len() > 1 {
            // Last pane in this tab — close the tab.
            self.close_tab_by_index(tab_idx, window, cx);
        } else {
            // Last pane in this window — close this workspace only.
            // App-level quit would tear down sibling windows too.
            if self.is_quick_terminal {
                self.reinitialize_quick_terminal_and_hide(window, cx);
                return;
            }
            self.close_window_from_last_tab(window, cx);
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
        // Title changed — sync sidebar and tab bar.
        self.sync_sidebar(cx);
        // The OSC title change is the most reliable signal that a
        // tab's purpose just shifted (`vim` → `bash`, `bash` → `htop`).
        // Re-ask the AI for an updated label/icon. The engine
        // dedupes on cache key so this is cheap if context didn't
        // actually change.
        self.request_tab_summaries(cx);
        cx.notify();
    }

    pub(crate) fn on_terminal_cwd_changed(
        &mut self,
        _entity: &Entity<GhosttyView>,
        event: &GhosttyCwdChanged,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _reported_cwd = event.0.as_deref();
        // Shell integration reports cwd independently from title/output.
        // Persist immediately so restart continuity survives a later crash or
        // force-quit instead of depending on an unrelated tab/layout save.
        self.sync_sidebar(cx);
        self.save_session(cx);
        self.request_tab_summaries(cx);
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
            .surface_infos(Some(origin_pane_id))
            .into_iter()
            .find_map(|surface| {
                (surface.terminal.entity_id() == entity_id).then_some(surface.terminal)
            });
        let cwd = origin_terminal
            .as_ref()
            .and_then(|terminal| terminal.current_dir(cx));

        let terminal = self.create_terminal(cwd.as_deref(), window, cx);
        let was_zoomed = self.tabs[tab_idx].pane_tree.zoomed_pane_id().is_some();
        self.tabs[tab_idx].pane_tree.split_pane_with_placement(
            origin_pane_id,
            direction,
            placement,
            terminal.clone(),
        );
        let tab_was_active = tab_idx == self.active_tab;
        #[cfg(target_os = "macos")]
        self.mark_tab_terminal_native_layout_pending(tab_idx, cx);
        self.notify_tab_terminal_views(tab_idx, cx);
        if tab_was_active {
            terminal.focus(window, cx);
            self.sync_active_terminal_focus_states(cx);
            self.sync_active_tab_native_view_visibility_now_or_after_layout(was_zoomed, window, cx);
        } else {
            terminal.set_focus_state(false, cx);
            self.sync_tab_native_view_visibility(tab_idx, false, cx);
        }
        Self::schedule_terminal_bootstrap_reassert(
            &terminal,
            tab_was_active,
            self.window_handle,
            self.workspace_handle.clone(),
            cx,
        );
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
        // Scan skills when cwd changes (project-local + platform global skills path).
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
        let active_agent_config = self.active_tab_agent_config();
        let model_name = AgentHarness::active_model_name_for(&active_agent_config);
        let provider = active_agent_config.provider.clone();
        let available_models = self.provider_models_for_config(&active_agent_config);
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
                AgentPanel::configured_session_providers(&active_agent_config),
                window,
                cx,
            );
            panel.set_provider_name(provider, window, cx);
            panel.set_model_name(model_name);
            panel.set_session_model_options(available_models, window, cx);
            panel.set_show_inline_input(show_inline);
            panel.set_skills(panel_skills, cx);
            panel.set_recent_inputs(self.recent_input_history(80));
        });

        let agent_panel_progress = self.agent_panel_motion.value(window);
        let input_bar_progress = self.input_bar_motion.value(window);
        let tab_strip_progress = self.tab_strip_motion.value(window);
        let agent_panel_transitioning = self.agent_panel_motion.is_animating();
        let input_bar_transitioning = self.input_bar_motion.is_animating();
        let tab_strip_transitioning = self.tab_strip_motion.is_animating();
        #[cfg(target_os = "macos")]
        let (agent_panel_snap_guard_active, agent_panel_snap_guard_expired) =
            Self::snap_guard_state(&mut self.agent_panel_snap_guard_until, window);
        #[cfg(target_os = "macos")]
        let (input_bar_snap_guard_active, input_bar_snap_guard_expired) =
            Self::snap_guard_state(&mut self.input_bar_snap_guard_until, window);
        #[cfg(target_os = "macos")]
        let (top_chrome_snap_guard_active, top_chrome_snap_guard_expired) =
            Self::snap_guard_state(&mut self.top_chrome_snap_guard_until, window);
        #[cfg(target_os = "macos")]
        let (sidebar_snap_guard_active, sidebar_snap_guard_expired) =
            Self::snap_guard_state(&mut self.sidebar_snap_guard_until, window);
        #[cfg(target_os = "macos")]
        {
            let release_cover = Duration::from_millis(CHROME_RELEASE_COVER_MS);
            if agent_panel_snap_guard_expired && !self.agent_panel_open {
                Self::extend_guard(&mut self.agent_panel_release_cover_until, release_cover);
            }
            if input_bar_snap_guard_expired && !self.input_bar_visible {
                Self::extend_guard(&mut self.input_bar_release_cover_until, release_cover);
            }
            if top_chrome_snap_guard_expired && !self.horizontal_tabs_visible() {
                Self::extend_guard(&mut self.top_chrome_release_cover_until, release_cover);
            }
            if sidebar_snap_guard_expired && !self.vertical_tabs_active() {
                self.sidebar_release_cover_width = self
                    .sidebar_release_cover_width
                    .max(self.sidebar_snap_guard_width);
                Self::extend_guard(&mut self.sidebar_release_cover_until, release_cover);
            }
        }
        #[cfg(target_os = "macos")]
        let agent_panel_release_cover_active =
            Self::snap_guard_active(&mut self.agent_panel_release_cover_until, window);
        #[cfg(target_os = "macos")]
        let input_bar_release_cover_active =
            Self::snap_guard_active(&mut self.input_bar_release_cover_until, window);
        #[cfg(target_os = "macos")]
        let top_chrome_release_cover_active =
            Self::snap_guard_active(&mut self.top_chrome_release_cover_until, window);
        #[cfg(target_os = "macos")]
        let sidebar_release_cover_active =
            Self::snap_guard_active(&mut self.sidebar_release_cover_until, window);
        #[cfg(target_os = "macos")]
        if !sidebar_snap_guard_active && !sidebar_release_cover_active {
            self.sidebar_snap_guard_width = 0.0;
            self.sidebar_release_cover_width = 0.0;
        }
        #[cfg(target_os = "macos")]
        {
            let allow_native_transition_underlay = self.terminal_opacity >= 0.999;
            let guard_active = if allow_native_transition_underlay {
                if let Some(until) = self.chrome_transition_underlay_until {
                    if Instant::now() < until {
                        window.request_animation_frame();
                        true
                    } else {
                        self.chrome_transition_underlay_until = None;
                        false
                    }
                } else {
                    false
                }
            } else {
                if self.chrome_transition_underlay_until.is_some() {
                    self.chrome_transition_underlay_until = None;
                }
                false
            };
            let pane_dragging =
                self.has_active_tab() && self.tabs[self.active_tab].pane_tree.is_dragging();
            let underlay_active = allow_native_transition_underlay
                && (agent_panel_transitioning
                    || input_bar_transitioning
                    || tab_strip_transitioning
                    || self.agent_panel_drag.is_some()
                    || self.sidebar_drag.is_some()
                    || pane_dragging
                    || guard_active);
            self.sync_chrome_transition_underlay(underlay_active, cx);
        }

        let window_width = window.bounds().size.width.as_f32();
        let effective_agent_panel_width = self
            .agent_panel_width
            .min(max_agent_panel_width(window_width));
        #[cfg(not(target_os = "macos"))]
        let animated_panel_width = effective_agent_panel_width * agent_panel_progress;
        #[cfg(target_os = "macos")]
        let agent_panel_reserved_for_layout =
            self.agent_panel_open || agent_panel_progress > 0.01 || agent_panel_snap_guard_active;
        #[cfg(target_os = "macos")]
        let agent_panel_outer_width = if agent_panel_reserved_for_layout {
            effective_agent_panel_width + 1.0
        } else {
            0.0
        };
        #[cfg(not(target_os = "macos"))]
        let agent_panel_outer_width = if agent_panel_progress > 0.01 {
            animated_panel_width + 1.0
        } else {
            0.0
        };
        let max_vertical_tabs_width =
            max_sidebar_panel_width(window_width, agent_panel_outer_width);
        let vertical_tabs_width = if self.vertical_tabs_active() {
            self.sidebar.update(cx, |sidebar, _cx| {
                sidebar.set_effective_panel_max_width(max_vertical_tabs_width);
                sidebar.occupied_width_with_max(max_vertical_tabs_width)
            })
        } else {
            #[cfg(target_os = "macos")]
            {
                if sidebar_snap_guard_active {
                    self.sidebar_snap_guard_width
                } else {
                    0.0
                }
            }
            #[cfg(not(target_os = "macos"))]
            0.0
        };
        let vertical_tabs_pinned = self.vertical_tabs_active() && self.sidebar.read(cx).is_pinned();

        // Render the vertical-tabs hover-card overlay up front so it
        // takes the (re-entrant) sidebar borrow before `theme` claims
        // the immutable cx borrow that the rest of `render` relies on.
        let vertical_tabs_overlay = if self.vertical_tabs_active() {
            self.sidebar.update(cx, |sidebar, cx| {
                sidebar.render_hover_card_overlay(window, cx)
            })
        } else {
            None
        };

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
        #[cfg(target_os = "macos")]
        let terminal_background = self.terminal_theme.background;
        #[cfg(target_os = "macos")]
        let terminal_surface_color: Hsla = Rgba {
            r: f32::from(terminal_background.r) / 255.0,
            g: f32::from(terminal_background.g) / 255.0,
            b: f32::from(terminal_background.b) / 255.0,
            // Seam covers do not get Ghostty's native blur/compositing.
            // Keep them opaque and tiny; a translucent GPUI seam is the
            // exact path that lets bright desktop/window backdrops leak
            // through during fast macOS chrome motion.
            a: 1.0,
        }
        .into();
        #[cfg(target_os = "macos")]
        let chrome_transition_seam_color = terminal_surface_color;
        #[cfg(not(target_os = "macos"))]
        let chrome_transition_seam_color = theme.background.opacity(elevated_ui_surface_opacity);
        #[cfg(target_os = "macos")]
        let chrome_static_seam_color = terminal_surface_color;
        #[cfg(not(target_os = "macos"))]
        let chrome_static_seam_color = theme.title_bar_border;
        #[cfg(target_os = "macos")]
        let pane_divider_color = terminal_separator_over_backdrop(terminal_surface_color, theme);
        #[cfg(not(target_os = "macos"))]
        let pane_divider_color = theme.title_bar_border;
        #[cfg(target_os = "macos")]
        let top_bar_surface_color = theme.title_bar.opacity(ui_surface_opacity);
        #[cfg(not(target_os = "macos"))]
        let top_bar_surface_color = theme.title_bar.opacity(ui_surface_opacity);
        #[cfg(target_os = "macos")]
        let input_bar_surface_color = theme.title_bar.opacity(ui_surface_opacity);
        #[cfg(not(target_os = "macos"))]
        let input_bar_surface_color = theme.title_bar.opacity(ui_surface_opacity);
        #[cfg(target_os = "macos")]
        let elevated_panel_surface_color = theme.background.opacity(elevated_ui_surface_opacity);
        #[cfg(not(target_os = "macos"))]
        let elevated_panel_surface_color = theme.background.opacity(elevated_ui_surface_opacity);
        let terminal_content_left = vertical_tabs_width;
        let terminal_content_width =
            (window_width - terminal_content_left - agent_panel_outer_width).max(0.0);

        let pane_tree_rendered = {
            let pending = self.pending_drag_init.clone();
            let begin_drag_cb = move |split_id: usize, start_pos: f32| {
                if let Ok(mut guard) = pending.lock() {
                    *guard = Some((split_id, start_pos));
                }
            };
            let workspace = cx.weak_entity();
            let focus_surface_cb = move |surface_id: usize, window: &mut Window, cx: &mut App| {
                if let Some(workspace) = workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| {
                        workspace.focus_surface_in_active_tab(surface_id, window, cx);
                    });
                }
            };
            let workspace = cx.weak_entity();
            let rename_surface_cb = move |surface_id: usize, window: &mut Window, cx: &mut App| {
                if let Some(workspace) = workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| {
                        workspace.begin_surface_rename(surface_id, window, cx);
                    });
                }
            };
            let workspace = cx.weak_entity();
            let close_surface_cb = move |surface_id: usize, window: &mut Window, cx: &mut App| {
                if let Some(workspace) = workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| {
                        workspace.close_surface_by_id_in_active_tab(surface_id, window, cx);
                    });
                }
            };
            self.tabs[self.active_tab].pane_tree.render(
                begin_drag_cb,
                focus_surface_cb,
                rename_surface_cb,
                close_surface_cb,
                self.surface_rename.clone(),
                pane_divider_color,
                cx,
            )
        };

        let mut terminal_area = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .bg(theme.transparent)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .child(pane_tree_rendered),
            );

        #[cfg(not(target_os = "macos"))]
        if input_bar_transitioning && input_bar_progress > 0.01 {
            terminal_area = terminal_area.child(
                div()
                    .h(px(CHROME_TRANSITION_SEAM_COVER))
                    .flex_shrink_0()
                    .bg(chrome_transition_seam_color),
            );
        }

        #[cfg(target_os = "macos")]
        let input_bar_reserved_for_layout =
            input_bar_progress > 0.01 || input_bar_snap_guard_active;
        #[cfg(not(target_os = "macos"))]
        let input_bar_reserved_for_layout = input_bar_progress > 0.01;

        if input_bar_reserved_for_layout {
            let input_bar_height = if input_bar_progress > 0.01 {
                43.0 * input_bar_progress
            } else {
                43.0
            };
            let input_bar_content_opacity = if input_bar_progress > 0.01 {
                input_bar_content_progress
            } else {
                0.0
            };
            terminal_area = terminal_area.child(
                div()
                    .overflow_hidden()
                    .h(px(input_bar_height))
                    .flex_shrink_0()
                    .bg(input_bar_surface_color)
                    .child(div().h(px(1.0)).bg(chrome_static_seam_color))
                    .child(
                        div()
                            .h(px(42.0))
                            .opacity(input_bar_content_opacity)
                            .child(self.input_bar.clone()),
                    ),
            );
        }

        let mut main_area = div().relative().flex().flex_1().min_h_0();

        if self.vertical_tabs_active() {
            #[cfg(target_os = "macos")]
            {
                main_area = main_area.child(
                    div()
                        .h_full()
                        .flex_shrink_0()
                        .overflow_hidden()
                        .bg(elevated_panel_surface_color)
                        .child(self.sidebar.clone()),
                );
            }

            #[cfg(not(target_os = "macos"))]
            {
                main_area = main_area.child(self.sidebar.clone());
            }
        }
        #[cfg(target_os = "macos")]
        if !self.vertical_tabs_active() && sidebar_snap_guard_active && vertical_tabs_width > 0.0 {
            main_area = main_area.child(
                div()
                    .w(px(vertical_tabs_width))
                    .h_full()
                    .flex_shrink_0()
                    .bg(elevated_panel_surface_color),
            );
        }

        main_area = main_area.child(terminal_area);

        if self.vertical_tabs_active() && vertical_tabs_width > 0.0 {
            main_area = main_area.child(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px((vertical_tabs_width - 1.0).max(0.0)))
                    .w(px(1.0))
                    .bg(chrome_static_seam_color),
            );
        }

        #[cfg(target_os = "macos")]
        let render_agent_panel = agent_panel_reserved_for_layout;
        #[cfg(not(target_os = "macos"))]
        let render_agent_panel = agent_panel_progress > 0.01;

        if render_agent_panel {
            #[cfg(target_os = "macos")]
            let panel_width = effective_agent_panel_width + 1.0;
            #[cfg(not(target_os = "macos"))]
            let panel_width = animated_panel_width + 1.0;
            #[cfg(target_os = "macos")]
            let agent_panel_content_opacity =
                if self.agent_panel_open || agent_panel_progress > 0.01 {
                    agent_panel_content_progress
                } else {
                    0.0
                };
            #[cfg(not(target_os = "macos"))]
            let agent_panel_content_opacity = agent_panel_content_progress;

            main_area = main_area.child(
                div()
                    .w(px(panel_width))
                    .h_full()
                    .overflow_hidden()
                    .flex_shrink_0()
                    .flex()
                    .flex_row()
                    .bg(elevated_panel_surface_color)
                    .child(
                        div()
                            .id("agent-panel-divider")
                            .relative()
                            .w(px(1.0))
                            .h_full()
                            .flex_shrink_0()
                            .bg(chrome_static_seam_color)
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .bottom_0()
                                    .left(px(-2.0))
                                    .w(px(5.0))
                                    .cursor_col_resize()
                                    .bg(theme.transparent)
                                    .hover(|s| s.bg(chrome_static_seam_color.opacity(0.18)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |this, event: &MouseDownEvent, _window, cx| {
                                                this.release_active_terminal_mouse_selection(cx);
                                                this.agent_panel_drag = Some((
                                                    f32::from(event.position.x),
                                                    effective_agent_panel_width,
                                                ));
                                            },
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .opacity(agent_panel_content_opacity)
                            .child(self.agent_panel.clone()),
                    ),
            );
        }

        if let Some(overlay) = vertical_tabs_overlay {
            main_area = main_area.child(overlay);
        }

        if vertical_tabs_pinned {
            let handle_left = (vertical_tabs_width - 3.0).max(0.0);
            main_area = main_area.child(
                div()
                    .id("vertical-tabs-resize-handle")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(handle_left))
                    .w(px(6.0))
                    .cursor_col_resize()
                    .occlude()
                    .bg(theme.transparent)
                    .hover(|s| s.bg(theme.muted.opacity(0.08)))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                            this.release_active_terminal_mouse_selection(cx);
                            let width = this.sidebar.read(cx).panel_width();
                            this.sidebar_drag = Some((f32::from(event.position.x), width));
                        }),
                    ),
            );
        }

        // Top bar — compact titlebar for one tab, full strip for many
        #[cfg(target_os = "macos")]
        let top_bar_height = if top_chrome_snap_guard_active {
            TOP_BAR_TABS_HEIGHT
        } else {
            self.current_top_bar_height()
        };
        #[cfg(not(target_os = "macos"))]
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
        let leading_pad = if cfg!(target_os = "macos") { 78.0 } else { 8.0 };
        let mut top_bar = div()
            .id("tab-bar")
            .flex()
            .h(px(top_bar_height))
            .items_end()
            .pl(px(leading_pad))
            .pr(px(6.0))
            .bg(top_bar_surface_color);

        #[cfg(target_os = "macos")]
        {
            top_bar = top_bar
                .window_control_area(WindowControlArea::Drag)
                .on_click(|event, window, _cx| {
                    if event.click_count() == 2 {
                        window.titlebar_double_click();
                    }
                });
        }

        #[cfg(not(target_os = "macos"))]
        {
            top_bar = top_bar
                .window_control_area(WindowControlArea::Drag)
                .on_mouse_down(MouseButton::Left, |_, _window, _cx| {
                    #[cfg(target_os = "linux")]
                    _window.start_window_move();
                })
                .on_click(|event, window, _cx| {
                    if event.click_count() == 2 {
                        window.titlebar_double_click();
                    }
                });
        }

        // Tabs container — appears only when there is real tab selection to do.
        // In vertical-tabs mode the side panel owns the tab list so we keep
        // this strip empty even with multiple tabs.
        let show_horizontal_tabs = self.horizontal_tabs_visible();
        let mut tabs_container = div().flex().flex_1().min_w_0().items_end();

        if show_horizontal_tabs {
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
                    // Windows: without `.occlude()` the parent top_bar's
                    // `WindowControlArea::Drag` hit-test swallows the
                    // click (returns HTCAPTION → window drag starts).
                    .occlude()
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
                    // Windows: without `.occlude()` the parent top_bar's
                    // `WindowControlArea::Drag` hit-test routes the click
                    // to the OS (HTCAPTION) and starts a window drag
                    // before GPUI fires `on_click` — so the tab never
                    // activates. Same treatment as the `+`, caption
                    // buttons, and tab-close controls in this file.
                    .occlude()
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

        let vertical_tabs_tooltip = if self.vertical_tabs_active() {
            "Use horizontal tabs"
        } else {
            "Use vertical tabs"
        };
        tab_controls = tab_controls.child(
            div()
                .id("toggle-vertical-tabs")
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
                        vertical_tabs_tooltip,
                        crate::keycaps::first_action_keystroke(&ToggleVerticalTabs, window),
                        window,
                        cx,
                    )
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_vertical_tabs(&ToggleVerticalTabs, window, cx);
                }))
                .child(
                    svg()
                        .path("phosphor/sidebar-fill.svg")
                        .size(px(12.0))
                        .text_color(if self.vertical_tabs_active() {
                            theme.primary
                        } else {
                            theme.muted_foreground.opacity(0.4)
                        }),
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

        // Settings button — only on platforms without a native menu
        // bar. macOS exposes Settings through `App → Settings…` (and
        // ⌘,) so a gear in the chrome would be redundant there. On
        // Windows and Linux it's the primary discovery surface for
        // Settings alongside the command palette.
        #[cfg(not(target_os = "macos"))]
        {
            tab_controls = tab_controls.child(
                div()
                    .id("toggle-settings")
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(22.0))
                    .rounded(px(5.0))
                    .cursor_pointer()
                    .occlude()
                    .hover(|s| s.bg(theme.muted.opacity(0.10)))
                    .tooltip(|window, cx| {
                        chrome_tooltip(
                            "Settings",
                            crate::keycaps::first_action_keystroke(
                                &settings_panel::ToggleSettings,
                                window,
                            ),
                            window,
                            cx,
                        )
                    })
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_settings(&settings_panel::ToggleSettings, window, cx);
                    }))
                    .child(
                        svg().path("phosphor/gear.svg").size(px(12.0)).text_color(
                            theme
                                .muted_foreground
                                .opacity(0.45 + (0.08 * compact_titlebar_progress)),
                        ),
                    ),
            );
        }

        top_bar = top_bar.child(tab_controls);

        // Non-macOS caption buttons: Min / (Max|Restore) / Close.
        // macOS gets its traffic-light cluster from the system. We
        // render these *inside* the top bar so they share the same
        // vertical strip and never occlude terminal content.
        #[cfg(any(target_os = "windows", target_os = "linux"))]
        {
            #[cfg(target_os = "linux")]
            let workspace_handle = cx.weak_entity();
            top_bar = top_bar.child(caption_buttons(
                window,
                theme,
                top_bar_height,
                #[cfg(target_os = "linux")]
                workspace_handle,
            ));
        }

        let mut root = div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.transparent)
            .font_family(theme.mono_font_family.clone());

        // Linux: con paints its own client-side decorations, so we
        // also have to clip the window to a rounded rectangle the
        // same way macOS gets from NSWindow + transparent backdrop
        // and Windows 11 gets from DWM. Wrap with `overflow_hidden`
        // so child surfaces (top bar, terminal pane, modals) all
        // respect the corner radius. 14px matches Mica's perceived
        // radius on Win11 and reads as "windowed" rather than
        // "phone-app sheet".
        #[cfg(target_os = "linux")]
        {
            root = root.rounded(px(14.0)).overflow_hidden();
        }

        root =
            root.key_context("ConWorkspace")
                // Pane drag-to-resize: capture mouse move/up on root so it works
                // even when cursor is over terminal views (which capture mouse events).
                .on_mouse_move({
                    let pending = self.pending_drag_init.clone();
                    cx.listener(move |this, event: &MouseMoveEvent, win, cx| {
                        if let Some((start_x, start_width)) = this.sidebar_drag {
                            let win_w = win.bounds().size.width.as_f32();
                            let agent_w = if this.agent_panel_open {
                                this.agent_panel_width.min(max_agent_panel_width(win_w)) + 1.0
                            } else {
                                0.0
                            };
                            let max_width = max_sidebar_panel_width(win_w, agent_w);
                            let delta = f32::from(event.position.x) - start_x;
                            let new_width = (start_width + delta).clamp(PANEL_MIN_WIDTH, max_width);
                            let current_width = this.sidebar.read(cx).panel_width();
                            if (current_width - new_width).abs() > 0.5 {
                                this.sidebar.update(cx, |sidebar, cx| {
                                    sidebar.set_panel_width(new_width, cx)
                                });
                                cx.notify();
                            }
                            return;
                        }

                        // Agent panel resize drag
                        if let Some((start_x, start_width)) = this.agent_panel_drag {
                            let delta = start_x - f32::from(event.position.x);
                            let max_width = max_agent_panel_width(win.bounds().size.width.as_f32());
                            let new_width =
                                (start_width + delta).clamp(AGENT_PANEL_MIN_WIDTH, max_width);
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

                        // Compute layout-dependent inputs *before* re-borrowing
                        // `this` mutably for the pane tree, otherwise we
                        // collide with the immutable borrow needed by
                        // `vertical_tabs_active` / `sidebar.read`.
                        let win_w = f32::from(win.bounds().size.width);
                        let win_h = f32::from(win.bounds().size.height);
                        let effective_agent_panel_width =
                            this.agent_panel_width.min(max_agent_panel_width(win_w));
                        let agent_panel_drag_width = if this.agent_panel_open {
                            effective_agent_panel_width + 7.0
                        } else {
                            0.0
                        };
                        let vertical_tabs_w =
                            if this.vertical_tabs_active() {
                                this.sidebar.read(cx).occupied_width_with_max(
                                    max_sidebar_panel_width(win_w, agent_panel_drag_width),
                                )
                            } else {
                                0.0
                            };

                        let pane_tree = &mut this.tabs[this.active_tab].pane_tree;

                        if !pane_tree.is_dragging() {
                            return;
                        }

                        // Estimate terminal area from window bounds minus fixed chrome
                        // (tab bar ~38px, input bar ~40px, agent panel if open,
                        // vertical-tabs panel on the leading edge if enabled).
                        let (current_pos, total_size) =
                            if let Some(dir) = pane_tree.dragging_direction() {
                                match dir {
                                    SplitDirection::Horizontal => (
                                        f32::from(event.position.x) - vertical_tabs_w,
                                        win_w - agent_panel_drag_width - vertical_tabs_w,
                                    ),
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
                        if this.sidebar_drag.is_some() {
                            this.sidebar_drag = None;
                            this.save_session(cx);
                            cx.notify();
                            return;
                        }
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
                .on_action(cx.listener(Self::toggle_vertical_tabs))
                .on_action(cx.listener(Self::toggle_settings))
                .on_action(cx.listener(Self::toggle_command_palette))
                .on_action(cx.listener(Self::new_tab))
                .on_action(cx.listener(Self::next_tab))
                .on_action(cx.listener(Self::previous_tab))
                .on_action(cx.listener(Self::select_tab_1))
                .on_action(cx.listener(Self::select_tab_2))
                .on_action(cx.listener(Self::select_tab_3))
                .on_action(cx.listener(Self::select_tab_4))
                .on_action(cx.listener(Self::select_tab_5))
                .on_action(cx.listener(Self::select_tab_6))
                .on_action(cx.listener(Self::select_tab_7))
                .on_action(cx.listener(Self::select_tab_8))
                .on_action(cx.listener(Self::select_tab_9))
                .on_action(cx.listener(Self::close_tab))
                .on_action(cx.listener(Self::close_pane))
                .on_action(cx.listener(Self::toggle_pane_zoom))
                .on_action(cx.listener(Self::split_right))
                .on_action(cx.listener(Self::split_down))
                .on_action(cx.listener(Self::split_left))
                .on_action(cx.listener(Self::split_up))
                .on_action(cx.listener(Self::clear_terminal))
                .on_action(cx.listener(Self::clear_restored_terminal_history_action))
                .on_action(cx.listener(Self::export_workspace_layout))
                .on_action(cx.listener(Self::add_workspace_layout_tabs))
                .on_action(cx.listener(Self::open_workspace_layout_window))
                .on_action(cx.listener(Self::new_surface))
                .on_action(cx.listener(Self::new_surface_split_right))
                .on_action(cx.listener(Self::new_surface_split_down))
                .on_action(cx.listener(Self::next_surface))
                .on_action(cx.listener(Self::previous_surface))
                .on_action(cx.listener(Self::rename_current_surface))
                .on_action(cx.listener(Self::close_surface))
                .on_action(cx.listener(Self::focus_input))
                .on_action(cx.listener(Self::cycle_input_mode))
                .on_action(cx.listener(Self::toggle_pane_scope_picker))
                .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if !this.pane_scope_picker_open {
                        return;
                    }

                    let mods = &event.keystroke.modifiers;
                    let key = event.keystroke.key.as_str();
                    let local_picker_key = !mods.control && !mods.alt && !mods.platform;
                    let unshifted_local_picker_key = local_picker_key && !mods.shift;

                    let mut handled = false;
                    if key == "escape" {
                        this.close_pane_scope_picker(cx);
                        handled = true;
                    } else if local_picker_key && key.eq_ignore_ascii_case("a") {
                        this.set_scope_broadcast(window, cx);
                        handled = true;
                    } else if local_picker_key && key.eq_ignore_ascii_case("f") {
                        this.set_scope_focused(window, cx);
                        handled = true;
                    } else if unshifted_local_picker_key
                        && let Some(digit) = key.chars().next().and_then(|c| c.to_digit(10))
                    {
                        let pane_index = if digit == 0 { 9 } else { (digit - 1) as usize };
                        this.toggle_scope_pane_by_index(pane_index, window, cx);
                        handled = true;
                    }

                    if handled {
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }))
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    // Don't handle workspace shortcuts when a modal overlay is open
                    if this.settings_panel.read(cx).is_overlay_visible()
                        || this.command_palette.read(cx).is_visible()
                    {
                        return;
                    }

                    let mods = &event.keystroke.modifiers;
                    let key = event.keystroke.key.as_str();

                    if key == "escape" && this.surface_rename.take().is_some() {
                        window.prevent_default();
                        cx.stop_propagation();
                        cx.notify();
                        return;
                    }

                    #[cfg(target_os = "macos")]
                    if mods.platform
                        && !mods.control
                        && !mods.alt
                        && matches!(key, "`" | "~" | ">" | "<")
                    {
                        if mods.shift || matches!(key, "~" | "<") {
                            cx.dispatch_action(&crate::PreviousWindow);
                        } else {
                            cx.dispatch_action(&crate::NextWindow);
                        }
                        window.prevent_default();
                        cx.stop_propagation();
                        return;
                    }

                    // Browser-style fallbacks. The configurable actions also bind
                    // Control-Tab / Control-Shift-Tab by default.
                    if mods.platform && mods.shift && key == "[" {
                        this.previous_tab(&PreviousTab, window, cx);
                        window.prevent_default();
                        cx.stop_propagation();
                        return;
                    }

                    if mods.platform && mods.shift && key == "]" {
                        this.next_tab(&NextTab, window, cx);
                        window.prevent_default();
                        cx.stop_propagation();
                    }
                }))
                .child(top_bar)
                .child(main_area);

        if tab_strip_transitioning {
            root = root.child(
                div()
                    .absolute()
                    .top(px(top_bar_height))
                    .left_0()
                    .right_0()
                    .h(px(CHROME_TRANSITION_SEAM_COVER))
                    .bg(chrome_transition_seam_color),
            );
        }

        #[cfg(target_os = "macos")]
        if top_chrome_release_cover_active {
            root = root.child(
                div()
                    .absolute()
                    .top(px(TOP_BAR_COMPACT_HEIGHT))
                    .left(px(terminal_content_left))
                    .right(px(agent_panel_outer_width))
                    .h(px(TOP_BAR_TABS_HEIGHT - TOP_BAR_COMPACT_HEIGHT))
                    .bg(chrome_transition_seam_color),
            );
        }

        #[cfg(target_os = "macos")]
        if sidebar_snap_guard_active {
            if self.vertical_tabs_active() {
                root = root.child(
                    div()
                        .absolute()
                        .top(px(top_bar_height))
                        .bottom_0()
                        .left(px(
                            (vertical_tabs_width - CHROME_MOTION_SEAM_OVERDRAW).max(0.0)
                        ))
                        .w(px(CHROME_MOTION_SEAM_OVERDRAW))
                        .bg(chrome_transition_seam_color),
                );
            }
        }

        #[cfg(target_os = "macos")]
        if sidebar_release_cover_active && self.sidebar_release_cover_width > 0.0 {
            root = root.child(
                div()
                    .absolute()
                    .top(px(top_bar_height))
                    .bottom_0()
                    .left_0()
                    .w(px(self.sidebar_release_cover_width))
                    .bg(chrome_transition_seam_color),
            );
        }

        #[cfg(target_os = "macos")]
        if input_bar_snap_guard_active {
            if self.input_bar_visible {
                root = root.child(
                    div()
                        .absolute()
                        .left(px(terminal_content_left))
                        .right(px(agent_panel_outer_width))
                        .bottom(px(43.0))
                        .h(px(CHROME_MOTION_SEAM_OVERDRAW))
                        .bg(chrome_transition_seam_color),
                );
            }
        }

        #[cfg(target_os = "macos")]
        if input_bar_release_cover_active {
            root = root.child(
                div()
                    .absolute()
                    .left(px(terminal_content_left))
                    .right(px(agent_panel_outer_width))
                    .bottom_0()
                    .h(px(43.0))
                    .bg(chrome_transition_seam_color),
            );
        }

        #[cfg(target_os = "macos")]
        if agent_panel_snap_guard_active {
            if self.agent_panel_open {
                let agent_panel_seam_right = (effective_agent_panel_width
                    - (CHROME_MOTION_SEAM_OVERDRAW - CHROME_TRANSITION_SEAM_COVER))
                    .max(0.0);
                root = root.child(
                    div()
                        .absolute()
                        .top(px(top_bar_height))
                        .bottom_0()
                        .right(px(agent_panel_seam_right))
                        .w(px(CHROME_MOTION_SEAM_OVERDRAW))
                        .bg(chrome_transition_seam_color),
                );
            }
        }

        #[cfg(target_os = "macos")]
        if agent_panel_release_cover_active {
            root = root.child(
                div()
                    .absolute()
                    .top(px(top_bar_height))
                    .bottom_0()
                    .right_0()
                    .w(px(effective_agent_panel_width + 1.0))
                    .bg(chrome_transition_seam_color),
            );
        }

        #[cfg(not(target_os = "macos"))]
        if agent_panel_transitioning && render_agent_panel {
            root = root.child(
                div()
                    .absolute()
                    .top(px(top_bar_height))
                    .bottom_0()
                    .right(px(animated_panel_width))
                    .w(px(CHROME_TRANSITION_SEAM_COVER))
                    .bg(chrome_transition_seam_color),
            );
        }

        // Skill autocomplete popup — rendered at workspace level above ghostty
        if has_skill_popup {
            let theme = cx.theme();
            let popup_available_width = (terminal_content_width - 48.0).max(240.0);
            let popup_width = px((terminal_content_width * 0.34)
                .clamp(320.0, 480.0)
                .min(popup_available_width));
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
                .left(px(terminal_content_left + 24.0))
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
            let popup_available_width = (terminal_content_width - 48.0).max(240.0);
            let popup_width = px((terminal_content_width * 0.32)
                .clamp(320.0, 440.0)
                .min(popup_available_width));
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
                .left(px(terminal_content_left + 24.0))
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
                let popup_available_width = (terminal_content_width - 40.0).max(280.0);
                let popup_width = px((terminal_content_width * 0.38)
                    .clamp(360.0, 520.0)
                    .min(popup_available_width));
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
                let pane_picker_binding =
                    crate::keycaps::first_action_keystroke(&TogglePaneScopePicker, window)
                        .map(|stroke| {
                            crate::keycaps::keycaps_for_stroke(&stroke, theme).into_any_element()
                        })
                        .unwrap_or_else(|| {
                            crate::keycaps::keycaps_for_binding("secondary-'", theme)
                        });
                let local_keycap = |label: &'static str| {
                    let wide = label.chars().count() > 1;
                    div()
                        .h(px(19.0))
                        .min_w(if wide { px(32.0) } else { px(19.0) })
                        .px(px(if wide { 6.0 } else { 0.0 }))
                        .rounded(px(5.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(theme.muted.opacity(0.14))
                        .text_size(px(10.5))
                        .line_height(px(11.0))
                        .font_family(theme.mono_font_family.clone())
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.foreground.opacity(0.74))
                        .child(label)
                };

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
                            .gap(px(4.0))
                            .child(pane_picker_binding)
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .font_family(theme.mono_font_family.clone())
                                    .text_color(theme.muted_foreground.opacity(0.58))
                                    .child("then"),
                            )
                            .child(local_keycap("1-9"))
                            .child(local_keycap("A"))
                            .child(local_keycap("F")),
                    );

                let preset_segment = |id: &'static str, label: &'static str, active: bool| {
                    div().flex_1().child(
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
                            .child(
                                preset_segment("scope-all", "All panes", is_broadcast)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            |this: &mut ConWorkspace,
                                             _: &MouseDownEvent,
                                             window,
                                             cx| {
                                                this.set_scope_broadcast(window, cx);
                                            },
                                        ),
                                    ),
                            )
                            .child(
                                preset_segment("scope-focused", "Focused", is_focused)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            |this: &mut ConWorkspace,
                                             _: &MouseDownEvent,
                                             window,
                                             cx| {
                                                this.set_scope_focused(window, cx);
                                            },
                                        ),
                                    ),
                            ),
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
                        .left(px(terminal_content_left + 20.0))
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

        let settings_visible = self.settings_panel.read(cx).is_overlay_visible();
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

/// One row's worth of presentation data for the vertical-tabs panel.
/// Computed by the workspace from the live tab state and pushed to
/// the panel via `sync_sessions`.
struct VerticalTabPresentation {
    name: String,
    subtitle: Option<String>,
    icon: &'static str,
    is_ssh: bool,
}

/// Smart-name + smart-icon for a vertical-tabs row.
///
/// Priority:
/// 1. **User-supplied label** (set via inline rename or context menu)
///    — terminal-icon, no subtitle.
/// 2. **SSH host** (e.g. `prod-1.example.com`) — globe icon, subtitle
///    is `user@host` if available.
/// 3. **Focused process** parsed out of the OSC-set terminal title
///    (e.g. `vim README.md`, `htop`, `less log.txt`) — icon picked by
///    process kind (editor / monitor / pager / shell), subtitle is the
///    cwd basename.
/// 4. **CWD basename** — terminal icon, no subtitle.
/// 5. **Shell name** (`bash`, `zsh`, `fish`) — terminal icon, no
///    subtitle.
/// 6. Fallback `Tab N` — terminal icon, no subtitle.
fn smart_tab_presentation(
    user_label: Option<&str>,
    ai_label: Option<&str>,
    ai_icon: Option<&'static str>,
    hostname: Option<&str>,
    title: Option<&str>,
    current_dir: Option<&str>,
    tab_index: usize,
) -> VerticalTabPresentation {
    let is_ssh_session = hostname.map(|h| !h.trim().is_empty()).unwrap_or(false);

    // Helper: pick the heuristic icon (used when no AI / SSH signal).
    let heuristic_icon = || {
        if let Some(raw) = title.map(str::trim).filter(|s| !s.is_empty()) {
            parse_focused_process(raw)
                .map(|(_, ic)| ic)
                .unwrap_or("phosphor/terminal.svg")
        } else {
            "phosphor/terminal.svg"
        }
    };

    // 1. User label always wins for the name.
    if let Some(label) = user_label.map(str::trim).filter(|s| !s.is_empty()) {
        let icon = if is_ssh_session {
            "phosphor/globe.svg"
        } else {
            // Prefer the AI-suggested icon for user-labelled tabs;
            // fall back to the heuristic.
            ai_icon.unwrap_or_else(heuristic_icon)
        };
        return VerticalTabPresentation {
            name: label.to_string(),
            subtitle: cwd_subtitle(current_dir),
            icon,
            is_ssh: is_ssh_session,
        };
    }

    // 2. AI label sits between user label and heuristics — never
    //    overrides an explicit user choice, but does override the
    //    "vim README.md" / "htop" parse output.
    if let Some(label) = ai_label.map(str::trim).filter(|s| !s.is_empty()) {
        let icon = if is_ssh_session {
            "phosphor/globe.svg"
        } else {
            ai_icon.unwrap_or_else(heuristic_icon)
        };
        return VerticalTabPresentation {
            name: label.to_string(),
            subtitle: cwd_subtitle(current_dir),
            icon,
            is_ssh: is_ssh_session,
        };
    }

    // 3. SSH host short-name (no AI needed for this).
    if let Some(host) = hostname.map(str::trim).filter(|s| !s.is_empty()) {
        return VerticalTabPresentation {
            name: host.to_string(),
            subtitle: cwd_subtitle(current_dir),
            icon: "phosphor/globe.svg",
            is_ssh: true,
        };
    }

    // 4. Focused-process heuristic.
    if let Some(raw) = title.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some((command, icon)) = parse_focused_process(raw) {
            return VerticalTabPresentation {
                name: command,
                subtitle: cwd_subtitle(current_dir),
                icon,
                is_ssh: false,
            };
        }
    }

    if let Some(dir) = current_dir {
        let path = std::path::Path::new(dir);
        let is_bare_home = matches!(
            path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy()),
            Some(ref name) if name == "home" || name == "Users"
        ) && path
            .parent()
            .and_then(|p| p.parent())
            .map_or(false, |pp| pp.parent().is_none());
        if !is_bare_home {
            if let Some(base) = path.file_name() {
                return VerticalTabPresentation {
                    name: base.to_string_lossy().into_owned(),
                    subtitle: None,
                    icon: "phosphor/terminal.svg",
                    is_ssh: false,
                };
            }
        }
    }

    if let Some(raw) = title.map(str::trim).filter(|s| !s.is_empty()) {
        return VerticalTabPresentation {
            name: raw.to_string(),
            subtitle: None,
            icon: "phosphor/terminal.svg",
            is_ssh: false,
        };
    }

    VerticalTabPresentation {
        name: format!("Tab {}", tab_index + 1),
        subtitle: None,
        icon: "phosphor/terminal.svg",
        is_ssh: false,
    }
}

fn cwd_subtitle(current_dir: Option<&str>) -> Option<String> {
    let dir = current_dir?;
    let home = std::env::var("HOME").ok();
    if let Some(home) = home.as_deref() {
        if dir == home {
            return Some("~".to_string());
        }
        if let Some(rest) = dir.strip_prefix(home) {
            if rest.starts_with('/') {
                let trimmed = format!("~{rest}");
                return Some(shorten_path(&trimmed));
            }
        }
    }
    Some(shorten_path(dir))
}

fn shorten_path(path: &str) -> String {
    const MAX_LEN: usize = 32;
    if path.chars().count() <= MAX_LEN {
        return path.to_string();
    }
    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 2 {
        return path.to_string();
    }
    let last = parts.last().copied().unwrap_or("");
    let parent = parts.get(parts.len() - 2).copied().unwrap_or("");
    let prefix = if path.starts_with('/') { "/" } else { "" };
    format!("{prefix}…/{parent}/{last}")
}

/// Parse a terminal title to extract the focused command and pick an
/// icon for it. Heuristic — terminals OSC-set their title to things
/// like `"vim README.md - ~/proj"` or `"htop"`. We strip trailing
/// `" — cwd"` suffixes, take the first word, and bucket it.
///
/// Returns `None` if the title looks like a bare shell name; the
/// caller falls through to cwd / shell naming so the row reads as a
/// shell session, not as a `bash`-named process.
fn parse_focused_process(title: &str) -> Option<(String, &'static str)> {
    let trimmed = title
        .split(" — ")
        .next()
        .or_else(|| title.split(" - ").next())
        .unwrap_or(title)
        .trim();
    if trimmed.is_empty() {
        return None;
    }
    let first_word = trimmed
        .split(|c: char| c.is_whitespace() || c == ':')
        .next()
        .unwrap_or("")
        .trim_start_matches('/');
    let basename = first_word.rsplit('/').next().unwrap_or(first_word);
    let lower = basename.to_ascii_lowercase();
    match lower.as_str() {
        // Bare shells aren't an interesting "process" — fall through
        // so the row gets named by cwd or user label instead.
        "bash" | "sh" | "zsh" | "fish" | "dash" | "ksh" | "ion" | "nu" | "pwsh" | "powershell"
        | "cmd" | "tmux" | "screen" => None,

        "vim" | "nvim" | "vi" | "neovim" | "nano" | "emacs" | "ed" | "helix" | "hx" | "kakoune"
        | "kak" | "micro" | "code" | "codium" | "subl" => {
            Some((trimmed.to_string(), "phosphor/code.svg"))
        }

        "htop" | "top" | "btop" | "btm" | "atop" | "iotop" | "glances" | "nvtop" | "bashtop"
        | "ctop" | "k9s" => Some((trimmed.to_string(), "phosphor/pulse.svg")),

        "less" | "more" | "most" | "bat" | "cat" | "tail" | "head" | "view" | "man" => {
            Some((trimmed.to_string(), "phosphor/book-open.svg"))
        }

        "ssh" | "mosh" => Some((trimmed.to_string(), "phosphor/globe.svg")),

        "git" | "lazygit" | "tig" | "gh" => Some((trimmed.to_string(), "phosphor/file-code.svg")),

        _ => Some((trimmed.to_string(), "phosphor/terminal.svg")),
    }
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

#[cfg(test)]
struct NewTabSyncPolicy {
    activates_new_tab: bool,
    syncs_sidebar: bool,
    notifies_ui: bool,
    syncs_native_visibility: bool,
    reuses_shared_tab_activation_flow: bool,
}

#[cfg(test)]
mod tests {
    use super::ConWorkspace;

    #[test]
    fn surface_key_bytes_preserves_literal_character_case() {
        assert_eq!(ConWorkspace::surface_key_bytes("A").unwrap(), b"A");
        assert_eq!(ConWorkspace::surface_key_bytes("z").unwrap(), b"z");
    }

    #[test]
    fn surface_key_bytes_matches_named_keys_case_insensitively() {
        assert_eq!(ConWorkspace::surface_key_bytes("ENTER").unwrap(), b"\n");
        assert_eq!(
            ConWorkspace::surface_key_bytes("Ctrl-C").unwrap(),
            vec![0x03]
        );
    }

    #[test]
    fn new_tab_requires_immediate_ui_sync() {
        let policy = ConWorkspace::new_tab_sync_policy_for_tests();
        assert!(policy.activates_new_tab);
        assert!(policy.syncs_sidebar);
        assert!(policy.notifies_ui);
        assert!(policy.syncs_native_visibility);
        assert!(policy.reuses_shared_tab_activation_flow);
    }

    #[test]
    fn promoting_single_tab_to_tab_strip_requires_deferred_top_chrome_refresh() {
        assert!(ConWorkspace::should_defer_top_chrome_refresh_when_tab_strip_appears_for_tests());
    }
}
