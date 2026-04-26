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

/// Minimum gap between two LLM requests for the same tab when the
/// previous one *succeeded*. Holds chatty PROMPT_COMMAND cwd updates
/// in check during steady-state.
const PER_TAB_REQUEST_BUDGET: Duration = Duration::from_secs(5);
/// Same idea but for the post-failure retry path. Shorter so a flaky
/// upstream (rate limit / empty response from a thinking model)
/// doesn't lock the tab out for several seconds.
const PER_TAB_RETRY_BUDGET: Duration = Duration::from_millis(750);
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
    /// Hash key of the last request that *succeeded*. We dedupe on
    /// this — if the context hasn't moved since the last accepted
    /// label, no need to re-ask. Failures DO NOT update this so a
    /// future call with the same context will retry instead of
    /// being silently locked out (the bug in the previous version).
    last_success_key: Option<u64>,
    /// When the last request was dispatched (success or failure).
    /// Used by the budget gates below.
    last_dispatch: Option<Instant>,
    /// `true` if the last completed request returned a usable
    /// summary. False after the engine drops a malformed / empty
    /// response. Distinguishes the success-budget gate from the
    /// (much shorter) retry-budget gate.
    last_was_success: bool,
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
                log::debug!(
                    target: "con_core::tab_summary",
                    "tab_summary skip tab_id={} reason=in_flight",
                    tab_id
                );
                return;
            }
            // Cache hit: only short-circuit when the LAST SUCCESSFUL
            // request used the same context. A failed request must
            // not poison this slot — otherwise a single empty
            // response from the model permanently locks the tab
            // until the user types something new.
            if entry.last_success_key == Some(key) {
                log::debug!(
                    target: "con_core::tab_summary",
                    "tab_summary skip tab_id={} reason=cache_hit",
                    tab_id
                );
                return;
            }
            // Budget gate: success path uses the long budget, failure
            // path uses the short retry budget.
            let budget = if entry.last_was_success {
                PER_TAB_REQUEST_BUDGET
            } else {
                PER_TAB_RETRY_BUDGET
            };
            if let Some(t) = entry.last_dispatch {
                if t.elapsed() < budget {
                    log::debug!(
                        target: "con_core::tab_summary",
                        "tab_summary skip tab_id={} reason=budget elapsed={:?} budget={:?}",
                        tab_id,
                        t.elapsed(),
                        budget,
                    );
                    return;
                }
            }
            entry.in_flight = true;
            entry.last_dispatch = Some(Instant::now());
        }

        let config = self.config.clone();
        let state = self.state.clone();
        self.runtime.spawn(async move {
            let result = request_summary(&config, &req).await;

            if let Some(entry) = state.lock().get_mut(&tab_id) {
                entry.in_flight = false;
                entry.last_was_success = result.is_some();
                if result.is_some() {
                    entry.last_success_key = Some(key);
                }
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
        "Pick a short, professional label + one icon for a terminal tab.\n\
         Output ONE line, format: LABEL|ICON. Nothing before, nothing after.\n\
         \n\
         LABEL: 1–3 words, ≤24 chars, Title Case. What is the tab FOR?\n\
         Examples: Build Watch, DB Shell, Test Run, Logs, Editor, Twosum, Study, API Tests.\n\
         No emoji, no quotes, no \"tab\". Do not echo \"bash\"/\"zsh\".\n\
         If only signal is cwd, use Title-Case of cwd basename\n\
         (cwd /home/u/proj/study → Study).\n\
         If signal is genuinely thin (no cwd, no commands, empty output), label \"Shell\".\n\
         \n\
         ICON keyword (pick ONE, exact spelling):\n\
         terminal — generic shell / build / test / scripts\n\
         code — vim/nvim/emacs/nano/helix/code editor in use\n\
         pulse — htop/top/btop/k9s/tail -f monitors\n\
         book — less/more/man/bat pagers\n\
         file — git/lazygit/tig version-control tools\n\
         globe — ssh/curl/http remote / network tools\n\
         \n\
         Context:\n\
         cwd: {cwd}\n\
         title: {title}\n\
         commands: {recent_cmds}\n\
         recent output:\n{recent_output}\n\
         \n\
         Decide and emit the line now. Do not show your work.\n\
         Final output (single line, LABEL|ICON):",
        cwd = req.cwd.as_deref().unwrap_or("(unknown)"),
        title = req.title.as_deref().unwrap_or("(unknown)"),
        recent_cmds = if recent_cmds == "(none captured by Con — user may have typed directly)" {
            "(none)".to_string()
        } else {
            recent_cmds
        },
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
    // Moonshot / OpenAI-style provider return empty text. A short,
    // task-aligned preamble fixes that.
    //
    // Token budget is generous (512) because thinking models —
    // Kimi K2.6 / kimi-k2-thinking (see
    // 3pp/models.dev/providers/moonshotai/models/kimi-k2.6.toml,
    // `reasoning = true`, `interleaved.field = "reasoning_content"`),
    // GPT-5, Claude Sonnet 4.5/4.7 reasoning — consume reasoning
    // tokens BEFORE the visible response. A 64-token cap was
    // burning the entire budget on chain-of-thought and surfacing
    // as "Response contained no message or tool call (empty)". The
    // visible response is at most ~30 chars, so 512 costs nothing
    // on non-thinking providers and gives reasoning models room.
    //
    // `complete_with_options` itself drives this through rig's
    // streaming API rather than `Prompt::prompt`, because rig's
    // non-streaming openai-compatible parser only reads
    // `choices[0].message.content` — reasoning models that emit
    // their final answer into `reasoning_content` would otherwise
    // come back empty. The streaming path supports the
    // `reasoning_content` channel and `MultiTurnStreamItem::FinalResponse`
    // already aggregates the visible text for us.
    let preamble = "You label terminal tabs in a developer's IDE-style sidebar. \
                    Output exactly one line in the format LABEL|ICON. Nothing else.";
    // 2048 is sized for k2.6-class reasoning models — your log
    // showed a ~1500-char chain-of-thought running out at "Let me
    // double-check constraints:\n-" before reaching the final
    // LABEL|ICON line. 2048 gives the model room to think and
    // still emit the structured answer; non-thinking providers
    // ignore the extra budget (they only generate ~30 chars).
    let raw = match provider.complete_with_options(&prompt, preamble, 2048).await {
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
/// fences, surrounding quotes, multi-line reasoning preambles, and
/// case variations on the icon keyword. Returns `None` for anything
/// we can't safely use.
fn parse_response(raw: &str) -> Option<(String, TabIconKind)> {
    // Reasoning models (Kimi K2.6, DeepSeek-R1, …) often emit a
    // multi-line answer like:
    //   Let me think about this tab.
    //   The user is auditing disk usage.
    //   Disk audit|terminal
    // …so we walk every line and pick the first one that parses
    // to a valid LABEL|ICON pair, instead of demanding the answer
    // be on the first non-empty non-fence line.
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("```") {
            continue;
        }
        if let Some(parsed) = parse_label_icon_line(line) {
            return Some(parsed);
        }
    }
    None
}

fn parse_label_icon_line(line: &str) -> Option<(String, TabIconKind)> {
    let (label_part, icon_part) = line.split_once('|')?;

    let label = label_part
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'')
        .trim()
        .to_string();
    if label.is_empty() || label.contains('\n') {
        return None;
    }
    // Reject obviously bad labels: anything that includes "tab" is
    // explicitly disallowed by the prompt. "Shell" + terminal IS
    // allowed — the prompt explicitly invites it as the
    // thin-signal default — so a response of "Shell|terminal"
    // means "model genuinely could not find anything more
    // specific", which is a useful label.
    let lower = label.to_ascii_lowercase();
    if lower == "tab" || lower.contains(" tab") || lower.starts_with("tab ") {
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
    fn parse_skips_reasoning_preamble() {
        // Reasoning models emit a multi-line answer where the
        // useful LABEL|ICON line isn't the first one. Verify the
        // parser walks past the preamble.
        let raw = "Let me think about this tab.\nThe user is auditing disk usage.\nDisk audit|terminal";
        let (label, icon) = parse_response(raw).unwrap();
        assert_eq!(label, "Disk audit");
        assert_eq!(icon, TabIconKind::Terminal);
    }

    #[test]
    fn parse_skips_reasoning_with_pipe_in_preamble() {
        // Preamble contains a '|' that doesn't validate — the
        // parser should keep walking instead of erroring.
        let raw = "Step 1 | inspect title.\nStep 2 | inspect cwd.\nMonitor|pulse";
        let (label, icon) = parse_response(raw).unwrap();
        assert_eq!(label, "Monitor");
        assert_eq!(icon, TabIconKind::Pulse);
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
    fn parse_accepts_shell_terminal_for_thin_signal() {
        // The new prompt explicitly invites "Shell|terminal" when
        // there's no other signal. That's a real, useful label —
        // not a sentinel — so the parser should accept it.
        let (label, icon) = parse_response("Shell|terminal").unwrap();
        assert_eq!(label, "Shell");
        assert_eq!(icon, TabIconKind::Terminal);
    }

    #[test]
    fn parse_rejects_label_containing_tab_word() {
        // Prompt explicitly disallows the word "tab" in labels.
        assert!(parse_response("Build tab|terminal").is_none());
        assert!(parse_response("Tab 5|terminal").is_none());
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

    /// PerTabState gate logic — pure simulation of the conditions in
    /// `request()` so we can lock down "a failed call must not
    /// poison the cache" without having to spin up a real engine
    /// + tokio runtime.
    fn should_dispatch(state: &PerTabState, key: u64) -> bool {
        if state.in_flight {
            return false;
        }
        if state.last_success_key == Some(key) {
            return false;
        }
        let budget = if state.last_was_success {
            PER_TAB_REQUEST_BUDGET
        } else {
            PER_TAB_RETRY_BUDGET
        };
        if let Some(t) = state.last_dispatch {
            if t.elapsed() < budget {
                return false;
            }
        }
        true
    }

    #[test]
    fn cache_blocks_resend_for_same_context_after_success() {
        let mut s = PerTabState::default();
        s.last_success_key = Some(42);
        s.last_was_success = true;
        s.last_dispatch = Some(Instant::now());
        assert!(!should_dispatch(&s, 42), "same context after success → skip");
    }

    #[test]
    fn cache_does_not_block_after_failure_with_same_context() {
        // The bug: previous version stored the dispatch key in
        // `last_key` BEFORE the LLM call, so a failed call locked
        // the tab out forever for that context. Verify the new
        // success-keyed cache lets the next call through.
        let mut s = PerTabState::default();
        s.last_success_key = None;
        s.last_was_success = false;
        // Pretend the last dispatch was 1 s ago — beyond the
        // failure-retry budget (750 ms) but well under the success
        // budget (5 s). Failure path should let it through.
        s.last_dispatch = Some(Instant::now() - Duration::from_secs(1));
        assert!(
            should_dispatch(&s, 99),
            "failure → context unchanged → next call must retry"
        );
    }

    #[test]
    fn retry_budget_holds_inside_750ms() {
        let mut s = PerTabState::default();
        s.last_dispatch = Some(Instant::now());
        s.last_was_success = false;
        // Just under the retry budget — should still hold.
        assert!(
            !should_dispatch(&s, 99),
            "retry budget should hold for the first 750 ms"
        );
    }

    #[test]
    fn in_flight_blocks_everything() {
        let mut s = PerTabState::default();
        s.in_flight = true;
        assert!(!should_dispatch(&s, 0));
    }

    #[test]
    fn new_context_after_success_dispatches() {
        let mut s = PerTabState::default();
        s.last_success_key = Some(1);
        s.last_was_success = true;
        s.last_dispatch = Some(Instant::now() - Duration::from_secs(10));
        // Different key, both budgets satisfied.
        assert!(should_dispatch(&s, 2));
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
