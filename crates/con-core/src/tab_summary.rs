//! Background AI engine that produces a short label + curated icon
//! for each open terminal tab.
//!
//! Why this exists
//! ---
//! The vertical-tabs panel needs a name + icon for every tab. The
//! cheap path is `parse_focused_process(title)` — fast, deterministic,
//! never needs the network — but it caps out at things the OSC title
//! actually surfaces (`vim README.md`, `htop`, `ssh host`). It can't
//! tell that a `bash` shell that just ran `cargo test` is "Test run"
//! or that a directory full of `.tf` files is "Terraform".
//!
//! This engine asks the user's already-configured **suggestion model**
//! (the same fast/cheap model the inline shell-completion engine
//! uses) for a 1–3 word label and one icon from a closed set. It's
//! gated by `agent.suggestion_model.enabled` — the same toggle —
//! so users who turned that off get no extra LLM traffic.
//!
//! Contract with the model
//! ---
//! The prompt forces a single-line `LABEL|ICON` format with `ICON`
//! constrained to one of the six [`TabIconKind`] variants. Anything
//! that doesn't parse cleanly is dropped on the floor — the panel
//! falls back to the heuristic name and we never silently render a
//! garbage label.
//!
//! Throttling
//! ---
//! - Per-tab cache keyed on `(cwd, top-3-recent-commands, title)` so
//!   we don't re-ask while context hasn't moved.
//! - At most one in-flight request per tab.
//! - At most one request per 5 s per tab as a budget guard against
//!   chatty PROMPT_COMMAND cwd updates.

use con_agent::AgentConfig;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;

/// Minimum gap between two LLM requests for the same tab.
const PER_TAB_REQUEST_BUDGET: Duration = Duration::from_secs(5);
/// Cap on the model's label string. Sanity bound; the model is
/// instructed to stay under ~24 chars.
const LABEL_MAX_LEN: usize = 32;

/// The closed set of icons the model is allowed to choose from.
///
/// This **is** the icon vocabulary for the vertical-tabs panel — the
/// keyword the model returns is mapped to a Phosphor SVG path by
/// [`TabIconKind::svg_path`]. No emoji. No free-form image references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabIconKind {
    Terminal,
    Code,
    Pulse,
    BookOpen,
    FileCode,
    Globe,
}

impl TabIconKind {
    /// Phosphor SVG path under `assets/icons/phosphor/`. Stable across
    /// callers so tests and rendering agree.
    pub fn svg_path(&self) -> &'static str {
        match self {
            TabIconKind::Terminal => "phosphor/terminal.svg",
            TabIconKind::Code => "phosphor/code.svg",
            TabIconKind::Pulse => "phosphor/pulse.svg",
            TabIconKind::BookOpen => "phosphor/book-open.svg",
            TabIconKind::FileCode => "phosphor/file-code.svg",
            TabIconKind::Globe => "phosphor/globe.svg",
        }
    }

    fn from_keyword(s: &str) -> Option<TabIconKind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "terminal" | "shell" | "console" => Some(TabIconKind::Terminal),
            "code" | "editor" | "vim" | "nvim" | "emacs" | "nano" | "helix" | "kakoune" => {
                Some(TabIconKind::Code)
            }
            "pulse" | "monitor" | "htop" | "top" | "btop" => Some(TabIconKind::Pulse),
            "book" | "book-open" | "pager" | "less" | "more" | "man" => Some(TabIconKind::BookOpen),
            "file" | "file-code" | "git" | "lazygit" | "tig" => Some(TabIconKind::FileCode),
            "globe" | "ssh" | "remote" | "network" => Some(TabIconKind::Globe),
            _ => None,
        }
    }
}

/// Result the engine pushes back to the workspace once the model
/// responds.
#[derive(Debug, Clone)]
pub struct TabSummary {
    pub tab_id: u64,
    pub label: String,
    pub icon: TabIconKind,
}

/// Inputs the engine consults when building a prompt. Workspace fills
/// this from the focused terminal pane's live state.
#[derive(Debug, Clone, Default)]
pub struct TabSummaryRequest {
    /// Stable identifier for the tab (the workspace uses tab index +
    /// monotonic counter so reorders don't confuse the cache).
    pub tab_id: u64,
    pub cwd: Option<String>,
    /// Most recent shell commands the user explicitly executed via
    /// the input bar / control socket. Empty when the user types
    /// directly into the terminal pane (we don't intercept those).
    /// Truncated to 3 by the engine.
    pub recent_commands: Vec<String>,
    /// Tail of the visible terminal scrollback — the same lines the
    /// user can see right now in the pane. This is the strongest
    /// signal we have for "what is this tab actually doing", because
    /// it works the same whether commands came from the input bar,
    /// the agent, the control socket, or the user typing directly.
    /// Truncated to 12 lines by the engine.
    pub recent_output: Vec<String>,
    /// Live OSC title of the focused terminal.
    pub title: Option<String>,
    /// SSH hostname if known. Pre-empts the LLM call: SSH tabs are
    /// always labelled by host with a globe icon, no model needed.
    pub ssh_host: Option<String>,
}

#[derive(Default)]
struct PerTabState {
    /// Hash key of the last request we sent — skip if context hasn't
    /// changed.
    last_key: Option<u64>,
    /// When the last request fired. Used to enforce
    /// `PER_TAB_REQUEST_BUDGET`.
    last_dispatch: Option<Instant>,
    /// `true` while a request is in flight; prevents duplicate work.
    in_flight: bool,
}

/// Background engine. Cheap to construct; safe to call `request` from
/// the GPUI render path (work fans out to a tokio runtime, results
/// arrive on a `crossbeam` channel that the caller drains).
pub struct TabSummaryEngine {
    state: Arc<Mutex<HashMap<u64, PerTabState>>>,
    config: AgentConfig,
    runtime: Arc<Runtime>,
}

impl TabSummaryEngine {
    pub fn new(config: AgentConfig, runtime: Arc<Runtime>) -> Self {
        Self {
            state: Arc::new(Mutex::new(HashMap::new())),
            config,
            runtime,
        }
    }

    /// Drop cached state — called when settings change so a new
    /// model gets a clean slate.
    pub fn clear(&self) {
        self.state.lock().clear();
    }

    /// Forget a single tab's cache (e.g. after the tab is closed).
    pub fn forget(&self, tab_id: u64) {
        self.state.lock().remove(&tab_id);
    }

    /// Ask the engine to produce a [`TabSummary`] for `req`. Returns
    /// immediately. The result (if one is produced — many calls are
    /// no-ops because of caching / budget / SSH short-circuit) is
    /// delivered through `callback`, which fires from the tokio
    /// worker thread.
    ///
    /// Workspace passes a callback that just sends on a `crossbeam`
    /// channel; the main loop drains it on the next idle tick.
    pub fn request(
        &self,
        req: TabSummaryRequest,
        callback: impl FnOnce(TabSummary) + Send + 'static,
    ) {
        // SSH tab: short-circuit, no LLM call.
        if let Some(host) = req
            .ssh_host
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            // Use the host's first label as the short name (e.g.
            // `prod-1.example.com` -> `prod-1`).
            let short = host.split('.').next().unwrap_or(host).to_string();
            callback(TabSummary {
                tab_id: req.tab_id,
                label: truncate_label(&short),
                icon: TabIconKind::Globe,
            });
            return;
        }

        let tab_id = req.tab_id;
        let key = context_hash(&req);

        {
            let mut guard = self.state.lock();
            let entry = guard.entry(tab_id).or_default();

            if entry.in_flight {
                return;
            }
            if entry.last_key == Some(key) {
                return;
            }
            if let Some(t) = entry.last_dispatch {
                if t.elapsed() < PER_TAB_REQUEST_BUDGET {
                    return;
                }
            }
            entry.in_flight = true;
            entry.last_dispatch = Some(Instant::now());
            entry.last_key = Some(key);
        }

        let config = self.config.clone();
        let state = self.state.clone();
        self.runtime.spawn(async move {
            let result = request_summary(&config, &req).await;

            // Always clear in-flight, even on parse failure / network
            // error — a future context change will retry.
            if let Some(entry) = state.lock().get_mut(&tab_id) {
                entry.in_flight = false;
            }

            if let Some((label, icon)) = result {
                callback(TabSummary {
                    tab_id,
                    label,
                    icon,
                });
            }
        });
    }
}

fn context_hash(req: &TabSummaryRequest) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    req.cwd.hash(&mut h);
    req.title.hash(&mut h);
    for cmd in req.recent_commands.iter().take(3) {
        cmd.hash(&mut h);
    }
    // Hash only the last few output lines — that's where new content
    // appears. Hashing the entire scrollback would re-fire every time
    // any line scrolled out, even for stable workloads (`htop`,
    // `tail -f`).
    for line in req.recent_output.iter().rev().take(5) {
        line.hash(&mut h);
    }
    h.finish()
}

fn truncate_label(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= LABEL_MAX_LEN {
        trimmed.to_string()
    } else {
        let mut out: String = trimmed.chars().take(LABEL_MAX_LEN.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Build the prompt + parse the response. Returns `None` whenever the
/// model output is malformed — the panel falls back to the heuristic
/// name in that case, which is the safe default.
async fn request_summary(
    config: &AgentConfig,
    req: &TabSummaryRequest,
) -> Option<(String, TabIconKind)> {
    use con_agent::AgentProvider;

    let recent_cmds = if req.recent_commands.is_empty() {
        "(none captured by Con — user may have typed directly)".to_string()
    } else {
        req.recent_commands
            .iter()
            .take(3)
            .map(|c| format!("- {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let recent_output = if req.recent_output.is_empty() {
        "(empty)".to_string()
    } else {
        req.recent_output
            .iter()
            .rev()
            .take(12)
            .rev()
            .map(|line| {
                // Trim each line so the model isn't distracted by
                // trailing whitespace or huge ANSI-stripped runs.
                let line = line.trim_end();
                if line.chars().count() > 200 {
                    let mut s: String = line.chars().take(200).collect();
                    s.push('…');
                    s
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let prompt = format!(
        "You name terminal tabs for a developer's vertical-tabs panel.\n\
         Given the tab's live context, pick a short, professional label and one icon.\n\
         \n\
         Rules:\n\
         - Output EXACTLY one line in the format LABEL|ICON. Nothing else.\n\
         - LABEL: 1–3 words, ≤24 chars, Title Case. Describe what the tab is FOR\n\
           (e.g. \"Build watch\", \"DB shell\", \"Test run\", \"Logs\", \"Editor\",\n\
           \"Twosum\", \"Study\", \"API tests\").\n\
           Do NOT use emoji. Do NOT quote. Do NOT include the word \"tab\".\n\
           Do NOT just echo the shell name (\"bash\", \"zsh\").\n\
           If the only signal is the cwd, use a Title-Case version of the cwd's\n\
           basename (e.g. cwd /home/u/proj/study → \"Study\").\n\
         - ICON: pick ONE keyword from this fixed set, exact spelling:\n\
           terminal | code | pulse | book | file | globe\n\
           - terminal — generic shell, build, test, ad-hoc commands, scripts\n\
           - code — editor (vim/nvim/emacs/nano/helix/code) currently in use\n\
           - pulse — long-running monitor (htop/top/btop/k9s/tail -f)\n\
           - book — pager / docs viewer (less/more/man/bat)\n\
           - file — version control / file ops (git/lazygit/tig)\n\
           - globe — remote session / network tool (ssh/curl/http)\n\
         - Read the recent output bottom-up — the most recent lines are\n\
           usually the most informative.\n\
         \n\
         Context:\n\
         cwd: {cwd}\n\
         title: {title}\n\
         commands captured by Con:\n{recent_cmds}\n\
         recent terminal output (oldest first):\n{recent_output}",
        cwd = req.cwd.as_deref().unwrap_or("(unknown)"),
        title = req.title.as_deref().unwrap_or("(unknown)"),
        recent_cmds = recent_cmds,
        recent_output = recent_output,
    );

    let provider = AgentProvider::new(config.clone());
    log::info!(
        target: "con_core::tab_summary",
        "tab_summary request tab_id={} provider={:?} model={} cwd={:?} title={:?}",
        req.tab_id,
        config.provider,
        config.effective_model(&config.provider),
        req.cwd.as_deref().unwrap_or(""),
        req.title.as_deref().unwrap_or(""),
    );
    // Override the default `complete()` preamble — that one tells
    // the model it's a shell-completion assistant, which fights
    // the LABEL|ICON instruction in our user prompt and made every
    // Moonshot/OpenAI-style provider return empty text. A short,
    // task-aligned preamble fixes that.
    //
    // The token budget is generous (512) because thinking models
    // like Kimi K2 (Moonshot k2p6), GPT-5, and Claude Sonnet
    // 4.5/4.7 reasoning consume reasoning tokens BEFORE the
    // visible response. A 64-token cap was burning the entire
    // budget on chain-of-thought and surfacing as
    // "ResponseError: Response contained no message or tool call
    // (empty)" — Moonshot k2p6 was hitting this every time. The
    // visible response is at most ~30 chars, so a 512-token cap
    // costs nothing on non-thinking providers and unblocks the
    // thinking ones.
    let preamble = "You label terminal tabs in a developer's IDE-style sidebar. \
                    Output exactly one line in the format LABEL|ICON. Nothing else.";
    let raw = match provider.complete_with_options(&prompt, preamble, 512).await {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                target: "con_core::tab_summary",
                "tab_summary completion failed tab_id={}: {}",
                req.tab_id,
                e
            );
            return None;
        }
    };
    let trimmed = raw.trim();
    log::info!(
        target: "con_core::tab_summary",
        "tab_summary response tab_id={} raw={:?}",
        req.tab_id,
        trimmed,
    );
    let parsed = parse_response(&raw);
    if parsed.is_none() {
        log::warn!(
            target: "con_core::tab_summary",
            "tab_summary parse rejected tab_id={} raw={:?} — keeping heuristic name",
            req.tab_id,
            trimmed,
        );
    } else if let Some((label, icon)) = &parsed {
        log::info!(
            target: "con_core::tab_summary",
            "tab_summary parsed tab_id={} label={:?} icon={:?}",
            req.tab_id,
            label,
            icon,
        );
    }
    parsed
}

/// Parse a `LABEL|ICON` response. Tolerates leading whitespace, code
/// fences, surrounding quotes, and case variations on the icon
/// keyword. Returns `None` for anything we can't safely use.
fn parse_response(raw: &str) -> Option<(String, TabIconKind)> {
    let line = raw
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("```"))?;

    let (label_part, icon_part) = line.split_once('|')?;

    let label = label_part
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'')
        .trim()
        .to_string();
    if label.is_empty() {
        return None;
    }
    if label.contains('\n') {
        return None;
    }
    // Reject obviously bad labels.
    let lower = label.to_ascii_lowercase();
    if lower == "tab" || lower == "shell" && icon_part.trim().eq_ignore_ascii_case("terminal") {
        // "Shell|terminal" is the model's "I don't know" fallback
        // per the prompt; treat that as no-result so we keep the
        // heuristic name (which knows the cwd basename, etc.)
        // instead of literally rendering "Shell".
        return None;
    }

    let icon = TabIconKind::from_keyword(icon_part)?;
    Some((truncate_label(&label), icon))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clean_response() {
        let (label, icon) = parse_response("Build watch|terminal").unwrap();
        assert_eq!(label, "Build watch");
        assert_eq!(icon, TabIconKind::Terminal);
    }

    #[test]
    fn parse_quoted_label() {
        let (label, icon) = parse_response("\"Test run\"|pulse").unwrap();
        assert_eq!(label, "Test run");
        assert_eq!(icon, TabIconKind::Pulse);
    }

    #[test]
    fn parse_strips_code_fence() {
        let raw = "```\nLogs|book\n```";
        let (label, icon) = parse_response(raw).unwrap();
        assert_eq!(label, "Logs");
        assert_eq!(icon, TabIconKind::BookOpen);
    }

    #[test]
    fn parse_rejects_missing_pipe() {
        assert!(parse_response("just a label").is_none());
    }

    #[test]
    fn parse_rejects_unknown_icon() {
        assert!(parse_response("Foo|sparkle").is_none());
    }

    #[test]
    fn parse_rejects_shell_terminal_fallback() {
        // The prompt's "I don't know" sentinel — keep the heuristic.
        assert!(parse_response("Shell|terminal").is_none());
    }

    #[test]
    fn parse_aliases_editor_to_code() {
        let (_, icon) = parse_response("Notes|editor").unwrap();
        assert_eq!(icon, TabIconKind::Code);
    }

    #[test]
    fn truncate_long_label() {
        let long = "a".repeat(50);
        let out = truncate_label(&long);
        assert!(out.chars().count() <= LABEL_MAX_LEN);
        assert!(out.ends_with('…'));
    }

    /// Tab reorder math (mirrors `on_sidebar_reorder` in workspace.rs).
    ///
    /// Slot semantics: `to ∈ 0..=tabs.len()`. Slot K with K < len
    /// means "insert before row K"; slot len means "after the last
    /// row". After `Vec::remove(from)` shifts subsequent indexes
    /// down by one, the resulting insert index is:
    ///   from < to → to - 1
    ///   from > to → to
    /// from == to or from + 1 == to → no-op (drop on the same row's
    /// top-half slot, or the slot just below — same place).
    ///
    /// Kept here because workspace.rs is too large to compile as a
    /// test target on this machine without bumping rustc's stack
    /// (we hit that earlier). The helper itself is pure so testing
    /// it next to other con-core logic is fine.
    fn apply_reorder<T: Clone>(items: &mut Vec<T>, from: usize, to: usize) {
        if from >= items.len() || to > items.len() {
            return;
        }
        if from == to || from + 1 == to {
            return;
        }
        let insert_at = if from < to { to - 1 } else { to };
        let item = items.remove(from);
        items.insert(insert_at, item);
    }

    #[test]
    fn reorder_drag_to_top_from_middle() {
        let mut v = vec!["a", "b", "c"];
        apply_reorder(&mut v, 1, 0);
        assert_eq!(v, vec!["b", "a", "c"]);
    }

    #[test]
    fn reorder_drag_to_top_from_bottom() {
        let mut v = vec!["a", "b", "c"];
        apply_reorder(&mut v, 2, 0);
        assert_eq!(v, vec!["c", "a", "b"]);
    }

    #[test]
    fn reorder_drag_to_bottom_from_top() {
        // Slot 3 == "after the last row" — this is the case that
        // didn't work before the half-row scheme.
        let mut v = vec!["a", "b", "c"];
        apply_reorder(&mut v, 0, 3);
        assert_eq!(v, vec!["b", "c", "a"]);
    }

    #[test]
    fn reorder_drag_to_bottom_from_middle() {
        let mut v = vec!["a", "b", "c"];
        apply_reorder(&mut v, 1, 3);
        assert_eq!(v, vec!["a", "c", "b"]);
    }

    #[test]
    fn reorder_self_drop_top_half_is_noop() {
        let mut v = vec!["a", "b", "c"];
        apply_reorder(&mut v, 1, 1);
        assert_eq!(v, vec!["a", "b", "c"]);
    }

    #[test]
    fn reorder_self_drop_bottom_half_is_noop() {
        // Slot 2 from row 1 means "below row 1" — same place.
        let mut v = vec!["a", "b", "c"];
        apply_reorder(&mut v, 1, 2);
        assert_eq!(v, vec!["a", "b", "c"]);
    }

    #[test]
    fn reorder_drag_middle_to_one_below() {
        let mut v = vec!["a", "b", "c", "d"];
        apply_reorder(&mut v, 1, 3);
        assert_eq!(v, vec!["a", "c", "b", "d"]);
    }
}
