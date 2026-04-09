use crossbeam_channel::Sender;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::context::PaneMode;
use crate::context::{PaneActionRecord, PaneShellContext};
use crate::control::{
    PaneAddressSpace, PaneControlCapability, PaneControlChannel, PaneProtocolAttachment,
    PaneVisibleTarget, PaneVisibleTargetKind, TmuxControlMode, TmuxControlState,
};
use crate::shell_probe::ShellProbeResult;
use crate::tmux::{TmuxCapture, TmuxExecLocation, TmuxExecResult, TmuxSnapshot};

/// Error type for agent tool execution
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("command failed: {0}")]
    CommandFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

// ── Path validation ─────────────────────────────────────────────────

/// Validate and canonicalize a path, ensuring it stays within the allowed root.
/// Used by all file tools to prevent path traversal attacks.
fn validate_path(raw: &str, allowed_root: &Path) -> Result<PathBuf, ToolError> {
    let path = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        allowed_root.join(raw)
    };
    let canonical = path
        .canonicalize()
        .map_err(|e| ToolError::CommandFailed(format!("path '{}': {}", raw, e)))?;
    if !canonical.starts_with(allowed_root) {
        return Err(ToolError::CommandFailed(format!(
            "path '{}' is outside the allowed directory '{}'",
            raw,
            allowed_root.display()
        )));
    }
    Ok(canonical)
}

/// Validate a path for write operations where the file may not exist yet.
/// Canonicalizes the parent directory and verifies it's within the allowed root.
fn validate_path_for_write(raw: &str, allowed_root: &Path) -> Result<PathBuf, ToolError> {
    let path = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        allowed_root.join(raw)
    };
    let parent = path
        .parent()
        .ok_or_else(|| ToolError::CommandFailed("invalid path: no parent directory".into()))?;
    // Create parent if needed, then canonicalize
    if !parent.exists() {
        std::fs::create_dir_all(parent)?;
    }
    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| ToolError::CommandFailed(format!("parent of '{}': {}", raw, e)))?;
    if !canonical_parent.starts_with(allowed_root) {
        return Err(ToolError::CommandFailed(format!(
            "path '{}' is outside the allowed directory '{}'",
            raw,
            allowed_root.display()
        )));
    }
    let file_name = path
        .file_name()
        .ok_or_else(|| ToolError::CommandFailed("invalid path: no filename".into()))?;
    Ok(canonical_parent.join(file_name))
}

// ── terminal_exec (visible) ─────────────────────────────────────────

/// Request to execute a command in a visible terminal pane.
/// When `pane_index` is None, targets the focused pane.
#[derive(Debug)]
pub struct TerminalExecRequest {
    pub command: String,
    pub working_dir: Option<String>,
    pub pane_index: Option<usize>,
    pub response_tx: Sender<TerminalExecResponse>,
}

/// Response from a visible terminal execution.
#[derive(Debug, Clone)]
pub struct TerminalExecResponse {
    pub output: String,
    pub exit_code: Option<i32>,
}

#[derive(Deserialize)]
pub struct TerminalExecArgs {
    pub command: String,
    pub pane_index: Option<usize>,
}

/// Tool that executes commands in the user's visible terminal.
///
/// Unlike `ShellExecTool` (which runs commands in a hidden subprocess),
/// this tool writes commands directly to the terminal PTY. The user sees
/// the command execute in real time — full transparency.
///
/// Communication flow:
/// 1. Tool sends `TerminalExecRequest` to the workspace via channel
/// 2. Workspace writes command to focused PTY
/// 3. Shell integration (OSC 133) reports completion with output
/// 4. Workspace sends `TerminalExecResponse` back
/// 5. Tool returns the output to the agent
pub struct TerminalExecTool {
    request_tx: Sender<TerminalExecRequest>,
}

impl TerminalExecTool {
    pub fn new(request_tx: Sender<TerminalExecRequest>) -> Self {
        Self { request_tx }
    }
}

impl Tool for TerminalExecTool {
    const NAME: &'static str = "terminal_exec";
    type Error = ToolError;
    type Args = TerminalExecArgs;
    type Output = ShellExecOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Execute a command visibly in a con terminal pane only when that pane's control_capabilities include exec_visible_shell. pane_index refers to a con pane from list_panes, not a tmux pane/window/editor target. For tmux, vim, nvim, agent CLIs, and other TUIs, inspect first and use send_keys only when intentional.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "pane_index": {
                        "type": "integer",
                        "description": "Target pane index (from list_panes). Omit to use the focused pane."
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);

        self.request_tx
            .send(TerminalExecRequest {
                command: args.command,
                working_dir: None,
                pane_index: args.pane_index,
                response_tx,
            })
            .map_err(|_| ToolError::CommandFailed("Terminal exec channel closed".into()))?;

        // Wait for the terminal to execute and report back.
        // 60s timeout: most commands finish quickly, but builds/tests can take longer.
        let response = tokio::task::block_in_place(|| {
            response_rx
                .recv_timeout(std::time::Duration::from_secs(60))
                .map_err(|_| {
                    ToolError::CommandFailed(
                        "Terminal exec timed out (60s) — the command may still be running".into(),
                    )
                })
        })?;

        Ok(ShellExecOutput {
            stdout: response.output,
            stderr: String::new(),
            exit_code: response.exit_code,
        })
    }
}

// ── shell_exec (background) ────────────────────────────────────────

#[derive(Deserialize)]
pub struct ShellExecArgs {
    pub command: String,
    pub working_dir: Option<String>,
}

#[derive(Serialize)]
pub struct ShellExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

pub struct ShellExecTool;

impl Tool for ShellExecTool {
    const NAME: &'static str = "shell_exec";
    type Error = ToolError;
    type Args = ShellExecArgs;
    type Output = ShellExecOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Execute a shell command in a hidden LOCAL background process on the con workspace machine. Output is captured but not shown in the terminal. Never use this for remote SSH/tmux inspection or remote mutations.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "working_dir": {
                        "type": "string",
                        "description": "Optional working directory"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut cmd = std::process::Command::new(&shell);
        cmd.arg("-c").arg(&args.command);
        if let Some(dir) = &args.working_dir {
            cmd.current_dir(dir);
        }
        let output = cmd.output()?;
        Ok(ShellExecOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code(),
        })
    }
}

// ── file_read ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct FileReadArgs {
    pub path: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
}

pub struct FileReadTool {
    allowed_root: PathBuf,
}

impl FileReadTool {
    pub fn new(allowed_root: PathBuf) -> Self {
        Self { allowed_root }
    }
}

impl Tool for FileReadTool {
    const NAME: &'static str = "file_read";
    type Error = ToolError;
    type Args = FileReadArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read a LOCAL file on the workspace machine. This tool CANNOT read files on remote SSH hosts. For remote files, use read_pane to see editor content or send_keys to run cat in a remote shell.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to read"
                    },
                    "start_line": {
                        "type": "integer",
                        "description": "Optional start line (1-indexed)"
                    },
                    "end_line": {
                        "type": "integer",
                        "description": "Optional end line (1-indexed)"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = validate_path(&args.path, &self.allowed_root)?;
        let content = std::fs::read_to_string(&path)?;
        let lines: Vec<&str> = content.lines().collect();
        let start = args
            .start_line
            .unwrap_or(1)
            .saturating_sub(1)
            .min(lines.len());
        let end = args
            .end_line
            .unwrap_or(lines.len())
            .min(lines.len())
            .max(start);
        Ok(lines[start..end].join("\n"))
    }
}

// ── file_write ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct FileWriteArgs {
    pub path: String,
    pub content: String,
}

pub struct FileWriteTool {
    allowed_root: PathBuf,
}

impl FileWriteTool {
    pub fn new(allowed_root: PathBuf) -> Self {
        Self { allowed_root }
    }
}

impl Tool for FileWriteTool {
    const NAME: &'static str = "file_write";
    type Error = ToolError;
    type Args = FileWriteArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Write content to a LOCAL file on the workspace machine. Creates the file if it doesn't exist. CANNOT write to remote SSH hosts — use send_keys to operate remote editors or shell redirects instead."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to write"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write"
                    }
                },
                "required": ["path", "content"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = validate_path_for_write(&args.path, &self.allowed_root)?;
        std::fs::write(&path, &args.content)?;
        Ok(format!(
            "Wrote {} bytes to {}",
            args.content.len(),
            args.path
        ))
    }
}

// ── edit_file ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct EditFileArgs {
    pub path: String,
    pub old_text: String,
    pub new_text: String,
}

/// Surgical file editing: finds `old_text` in the file and replaces it with `new_text`.
/// Much safer than file_write (which overwrites the entire file).
pub struct EditFileTool {
    allowed_root: PathBuf,
}

impl EditFileTool {
    pub fn new(allowed_root: PathBuf) -> Self {
        Self { allowed_root }
    }
}

impl Tool for EditFileTool {
    const NAME: &'static str = "edit_file";
    type Error = ToolError;
    type Args = EditFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Edit a LOCAL file on the workspace machine by replacing a specific text snippet. CANNOT edit files on remote SSH hosts — use send_keys with editor commands instead.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to edit"
                    },
                    "old_text": {
                        "type": "string",
                        "description": "The exact text to find and replace (must match exactly)"
                    },
                    "new_text": {
                        "type": "string",
                        "description": "The replacement text"
                    }
                },
                "required": ["path", "old_text", "new_text"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = validate_path(&args.path, &self.allowed_root)?;
        let content = std::fs::read_to_string(&path)?;

        let count = content.matches(&args.old_text).count();
        if count == 0 {
            return Err(ToolError::CommandFailed(format!(
                "old_text not found in {}",
                args.path
            )));
        }
        if count > 1 {
            return Err(ToolError::CommandFailed(format!(
                "old_text matches {} times in {} — must be unique. Provide more surrounding context.",
                count, args.path
            )));
        }

        let new_content = content.replacen(&args.old_text, &args.new_text, 1);
        std::fs::write(&path, &new_content)?;

        let old_lines = args.old_text.lines().count();
        let new_lines = args.new_text.lines().count();
        Ok(format!(
            "Edited {}: replaced {} line(s) with {} line(s)",
            args.path, old_lines, new_lines
        ))
    }
}

// ── list_files ─────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListFilesArgs {
    pub path: Option<String>,
    pub pattern: Option<String>,
    pub max_depth: Option<usize>,
}

/// List files in a directory, optionally filtered by glob pattern.
pub struct ListFilesTool {
    allowed_root: PathBuf,
}

impl ListFilesTool {
    pub fn new(allowed_root: PathBuf) -> Self {
        Self { allowed_root }
    }
}

impl Tool for ListFilesTool {
    const NAME: &'static str = "list_files";
    type Error = ToolError;
    type Args = ListFilesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List LOCAL files and directories on the workspace machine. CANNOT list files on remote SSH hosts.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory to list (defaults to cwd)"
                    },
                    "pattern": {
                        "type": "string",
                        "description": "Glob pattern to filter (e.g. '*.rs', '**/*.toml')"
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Maximum directory depth (default: 3)"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let dir = match &args.path {
            Some(p) => validate_path(p, &self.allowed_root)?,
            None => self.allowed_root.clone(),
        };
        let max_depth = args.max_depth.unwrap_or(3);

        // Try git ls-files first (respects .gitignore, fast)
        let git_listing = std::process::Command::new("git")
            .args(["ls-files", "--cached", "--others", "--exclude-standard"])
            .current_dir(&dir)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .filter(|s| !s.trim().is_empty());

        // Fall back to find for non-git directories
        let stdout = match git_listing {
            Some(listing) => {
                let filtered: Vec<&str> = listing
                    .lines()
                    .filter(|path| {
                        let depth = path.chars().filter(|c| *c == '/').count();
                        if depth >= max_depth {
                            return false;
                        }
                        if let Some(ref pattern) = args.pattern {
                            let filename = path.rsplit('/').next().unwrap_or(path);
                            glob_match(pattern, filename)
                        } else {
                            true
                        }
                    })
                    .collect();
                filtered.join("\n")
            }
            None => {
                let mut cmd = std::process::Command::new("find");
                cmd.arg(&dir);
                cmd.args(["-maxdepth", &max_depth.to_string()]);
                cmd.args(["-not", "-path", "*/.git/*"]);
                if let Some(ref pattern) = args.pattern {
                    cmd.args(["-name", pattern]);
                }
                cmd.args(["-type", "f"]);
                let output = cmd.output()?;
                String::from_utf8_lossy(&output.stdout).to_string()
            }
        };

        let mut files: Vec<&str> = stdout.lines().collect();
        files.sort();

        let total = files.len();
        let listing: String = files.into_iter().take(500).collect::<Vec<_>>().join("\n");

        if total > 500 {
            Ok(format!("{}\n... ({} more files)", listing, total - 500))
        } else {
            Ok(listing)
        }
    }
}

// ── search ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SearchArgs {
    pub pattern: String,
    pub path: Option<String>,
    pub file_pattern: Option<String>,
}

#[derive(Serialize)]
pub struct SearchOutput {
    pub matches: Vec<SearchMatch>,
    pub truncated: bool,
}

#[derive(Serialize)]
pub struct SearchMatch {
    pub file: String,
    pub line: usize,
    pub text: String,
}

pub struct SearchTool {
    allowed_root: PathBuf,
}

impl SearchTool {
    pub fn new(allowed_root: PathBuf) -> Self {
        Self { allowed_root }
    }
}

impl Tool for SearchTool {
    const NAME: &'static str = "search";
    type Error = ToolError;
    type Args = SearchArgs;
    type Output = SearchOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search for text in LOCAL files on the workspace machine using grep. CANNOT search remote SSH hosts. Returns matching lines with file paths and line numbers.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Search pattern (passed to grep -rn)"
                    },
                    "path": {
                        "type": "string",
                        "description": "Directory to search in (defaults to cwd)"
                    },
                    "file_pattern": {
                        "type": "string",
                        "description": "Glob pattern for files (e.g. '*.rs')"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let search_dir = match &args.path {
            Some(p) => validate_path(p, &self.allowed_root)?,
            None => self.allowed_root.clone(),
        };

        let mut cmd = std::process::Command::new("grep");
        cmd.args(["-rn", "--max-count=100"]);
        if let Some(ref fp) = args.file_pattern {
            cmd.args(["--include", fp]);
        }
        cmd.arg("--").arg(&args.pattern);
        cmd.arg(&search_dir);

        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut matches = Vec::new();
        let lines: Vec<&str> = stdout.lines().collect();
        let truncated = lines.len() >= 100;

        for line in lines.into_iter().take(100) {
            let mut parts = line.splitn(3, ':');
            if let (Some(file), Some(line_no), Some(text)) =
                (parts.next(), parts.next(), parts.next())
            {
                matches.push(SearchMatch {
                    file: file.to_string(),
                    line: line_no.parse().unwrap_or(0),
                    text: text.to_string(),
                });
            }
        }

        Ok(SearchOutput { matches, truncated })
    }
}

// ── Pane interaction layer ──────────────────────────────────────────
//
// Panes are first-class addressable entities. The agent discovers them
// via `list_panes`, reads content via `read_pane`, and sends input via
// `send_keys`. This abstraction supports shell, TUI, and future
// agent-in-pane scenarios uniformly.
//
// Communication follows the same channel pattern as TerminalExecTool:
// Tool → PaneRequest → Workspace → PaneResponse → Tool

/// Metadata about a single terminal pane.
///
/// Includes runtime and control state so the agent does not execute against
/// the wrong target or over-trust missing backend facts.
#[derive(Debug, Clone, Serialize)]
pub struct PaneInfo {
    pub index: usize,
    pub title: String,
    pub cwd: Option<String>,
    pub is_focused: bool,
    pub rows: usize,
    pub cols: usize,
    /// Whether the PTY child process is still running.
    pub is_alive: bool,
    /// Proven hostname when the backend can actually supply one.
    pub hostname: Option<String>,
    /// Confidence for the effective hostname, when detected.
    pub hostname_confidence: Option<crate::context::PaneConfidence>,
    /// Evidence source for the effective hostname, when detected.
    pub hostname_source: Option<crate::context::PaneEvidenceSource>,
    /// Current verified front-state for the pane.
    pub front_state: crate::context::PaneFrontState,
    /// Current pane mode: shell, tmux-like multiplexer, or another TUI.
    pub mode: PaneMode,
    /// Whether shell metadata like cwd and last_command is likely fresh for the visible app.
    pub shell_metadata_fresh: bool,
    /// Whether the most recent typed shell-context snapshot still matches the current shell frame.
    pub shell_context_fresh: bool,
    /// What the backend can authoritatively observe for this pane today.
    pub observation_support: crate::context::PaneObservationSupport,
    /// The only address space valid for pane_index today.
    pub address_space: PaneAddressSpace,
    /// The best-known visible target inside this con pane.
    pub visible_target: PaneVisibleTarget,
    /// Nested runtime/control targets from outer shell toward the front-most visible app.
    pub target_stack: Vec<PaneVisibleTarget>,
    /// tmux adapter state when a tmux layer is present in this pane.
    pub tmux_control: Option<TmuxControlState>,
    /// Explicit protocol attachments currently available on this pane.
    pub control_attachments: Vec<PaneProtocolAttachment>,
    /// Control channels con can use on this pane.
    pub control_channels: Vec<PaneControlChannel>,
    /// Capabilities currently available on this pane.
    pub control_capabilities: Vec<PaneControlCapability>,
    /// Addressing and control notes the agent should respect.
    pub control_notes: Vec<String>,
    /// The top-most active runtime scope, when detected.
    pub active_scope: Option<crate::context::PaneRuntimeScope>,
    /// Detected external agent CLI, when visible.
    pub agent_cli: Option<String>,
    /// Evidence behind the current runtime summary.
    pub evidence: Vec<crate::context::PaneEvidence>,
    /// Structured runtime scopes inferred from pane-local evidence.
    pub runtime_stack: Vec<crate::context::PaneRuntimeScope>,
    /// Last verified shell-frame stack captured for this pane.
    pub last_verified_runtime_stack: Vec<crate::context::PaneRuntimeScope>,
    /// Warnings about stale or advisory runtime metadata.
    pub runtime_warnings: Vec<String>,
    /// Last typed shell-context snapshot captured for this pane.
    pub shell_context: Option<PaneShellContext>,
    /// Recent con-originated actions for this pane.
    pub recent_actions: Vec<PaneActionRecord>,
    /// Weak observation hints derived from the current visible screen snapshot.
    pub screen_hints: Vec<crate::context::PaneObservationHint>,
    /// tmux session hint when detected from the pane itself.
    pub tmux_session: Option<String>,
    /// Whether shell integration (OSC 133) is active.
    pub has_shell_integration: bool,
    /// Most recent command text when the backend can prove it.
    pub last_command: Option<String>,
    /// Exit code of the last command.
    pub last_exit_code: Option<i32>,
    /// A command is currently executing (between OSC 133 C and D).
    /// Only reliable when has_shell_integration is true.
    pub is_busy: bool,
}

/// A request from a pane tool to the workspace.
#[derive(Debug)]
pub struct PaneRequest {
    pub query: PaneQuery,
    pub response_tx: Sender<PaneResponse>,
}

/// Pane query types — the workspace interprets these against PaneTree/Grid.
#[derive(Debug)]
pub enum PaneQuery {
    /// List all panes with metadata.
    List,
    /// Read recent output from a specific pane.
    ReadContent { pane_index: usize, lines: usize },
    /// Send raw keystrokes to a specific pane (for TUI interaction, Ctrl-C, etc.).
    SendKeys { pane_index: usize, keys: String },
    /// Search scrollback + visible screen for a text pattern.
    SearchText {
        pane_index: Option<usize>,
        pattern: String,
        max_matches: usize,
    },
    /// Return tmux adapter state for a pane whose target stack contains tmux.
    InspectTmux { pane_index: usize },
    /// Query tmux windows/panes through a same-session tmux control anchor.
    TmuxList { pane_index: usize },
    /// Capture pane content from a tmux pane target through a same-session tmux control anchor.
    TmuxCapture {
        pane_index: usize,
        target: Option<String>,
        lines: usize,
    },
    /// Send literal text or tmux key names to a tmux pane target through a same-session tmux control anchor.
    TmuxSendKeys {
        pane_index: usize,
        target: String,
        literal_text: Option<String>,
        key_names: Vec<String>,
        append_enter: bool,
    },
    /// Run a command through tmux itself by creating a new tmux target.
    TmuxRunCommand {
        pane_index: usize,
        target: Option<String>,
        location: TmuxExecLocation,
        command: String,
        window_name: Option<String>,
        cwd: Option<String>,
        detached: bool,
    },
    /// Run a read-only shell-scoped probe in a pane with a proven fresh shell prompt.
    ProbeShellContext { pane_index: usize },
    /// Lightweight busy check for a single pane (used by wait_for polling).
    /// Returns only is_busy + has_shell_integration, avoiding full List forensics.
    CheckBusy { pane_index: usize },
    /// Wait for a pane to become idle or match a pattern.
    WaitFor {
        pane_index: usize,
        timeout_secs: Option<u64>,
        pattern: Option<String>,
    },
    /// Create a new terminal pane (tab), optionally running a command in it.
    CreatePane { command: Option<String> },
}

/// Response from the workspace to a pane tool.
#[derive(Debug, Clone)]
pub enum PaneResponse {
    PaneList(Vec<PaneInfo>),
    Content(String),
    KeysSent,
    TmuxInfo(TmuxControlState),
    TmuxList(TmuxSnapshot),
    TmuxCapture(TmuxCapture),
    TmuxExec(TmuxExecResult),
    ShellProbe(ShellProbeResult),
    /// Search results: Vec of (pane_index, line_number, line_text).
    SearchResults(Vec<(usize, usize, String)>),
    /// Lightweight busy-check response.
    BusyStatus {
        is_busy: bool,
        has_shell_integration: bool,
    },
    /// Response from a wait_for operation.
    WaitComplete {
        status: String,
        output: String,
    },
    /// A new pane was created successfully.
    PaneCreated {
        pane_index: usize,
    },
    Error(String),
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TmuxTargetKindFilter {
    Any,
    Shell,
    AgentCli,
}

#[derive(Debug, Clone, Serialize)]
pub struct TmuxFindTargetsResult {
    pub kind: TmuxTargetKindFilter,
    pub best_match: Option<crate::tmux::TmuxPaneInfo>,
    pub matches: Vec<crate::tmux::TmuxPaneInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TmuxEnsureShellTargetResult {
    pub created: bool,
    pub pane: crate::tmux::TmuxPaneInfo,
    pub creation: Option<TmuxExecResult>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkTargetIntent {
    VisibleShell,
    RemoteShell,
    TmuxWorkspace,
    TmuxShell,
    AgentCli,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkTargetControlPath {
    VisibleShellExec,
    TmuxQuery,
    TmuxShellTarget,
    TmuxAgentTarget,
    VisibleAgentUi,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkTargetCandidate {
    pub pane_index: usize,
    pub pane_title: String,
    pub host: Option<String>,
    pub control_path: WorkTargetControlPath,
    pub visible_target: PaneVisibleTarget,
    pub tmux_mode: Option<TmuxControlMode>,
    pub tmux_target: Option<crate::tmux::TmuxPaneInfo>,
    pub requires_preparation: bool,
    pub suggested_tool: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolveWorkTargetResult {
    pub intent: WorkTargetIntent,
    pub best_match: Option<WorkTargetCandidate>,
    pub candidates: Vec<WorkTargetCandidate>,
}

fn recv_pane_response(
    response_rx: crossbeam_channel::Receiver<PaneResponse>,
    timeout_secs: u64,
    timeout_label: &'static str,
) -> Result<PaneResponse, ToolError> {
    tokio::task::block_in_place(|| {
        response_rx
            .recv_timeout(std::time::Duration::from_secs(timeout_secs))
            .map_err(|_| ToolError::CommandFailed(format!("{timeout_label} timed out")))
    })
}

fn pane_query_tmux_list(
    pane_tx: &Sender<PaneRequest>,
    pane_index: usize,
) -> Result<TmuxSnapshot, ToolError> {
    let (response_tx, response_rx) = crossbeam_channel::bounded(1);
    pane_tx
        .send(PaneRequest {
            query: PaneQuery::TmuxList { pane_index },
            response_tx,
        })
        .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

    match recv_pane_response(response_rx, 12, "tmux list")? {
        PaneResponse::TmuxList(snapshot) => Ok(snapshot),
        PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
        _ => Err(ToolError::CommandFailed("Unexpected response".into())),
    }
}

fn pane_query_tmux_run_command(
    pane_tx: &Sender<PaneRequest>,
    pane_index: usize,
    target: Option<String>,
    location: TmuxExecLocation,
    command: String,
    window_name: Option<String>,
    cwd: Option<String>,
    detached: bool,
) -> Result<TmuxExecResult, ToolError> {
    let (response_tx, response_rx) = crossbeam_channel::bounded(1);
    pane_tx
        .send(PaneRequest {
            query: PaneQuery::TmuxRunCommand {
                pane_index,
                target,
                location,
                command,
                window_name,
                cwd,
                detached,
            },
            response_tx,
        })
        .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

    match recv_pane_response(response_rx, 12, "tmux run-command")? {
        PaneResponse::TmuxExec(exec) => Ok(exec),
        PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
        _ => Err(ToolError::CommandFailed("Unexpected response".into())),
    }
}

fn pane_query_list(pane_tx: &Sender<PaneRequest>) -> Result<Vec<PaneInfo>, ToolError> {
    let (response_tx, response_rx) = crossbeam_channel::bounded(1);
    pane_tx
        .send(PaneRequest {
            query: PaneQuery::List,
            response_tx,
        })
        .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

    match recv_pane_response(response_rx, 5, "pane list")? {
        PaneResponse::PaneList(panes) => Ok(panes),
        PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
        _ => Err(ToolError::CommandFailed("Unexpected response".into())),
    }
}

fn tmux_command_basename(command: &str) -> &str {
    command.rsplit('/').next().unwrap_or(command)
}

fn is_tmux_shell_command(command: &str) -> bool {
    matches!(
        tmux_command_basename(command).to_ascii_lowercase().as_str(),
        "bash" | "zsh" | "sh" | "fish" | "dash" | "ksh" | "tcsh" | "csh" | "nu"
    )
}

fn is_tmux_agent_cli_command(command: &str) -> bool {
    matches!(
        tmux_command_basename(command).to_ascii_lowercase().as_str(),
        "codex" | "claude" | "claude-code" | "opencode" | "open-code"
    )
}

fn sort_tmux_matches(matches: &mut [crate::tmux::TmuxPaneInfo]) {
    matches.sort_by(|a, b| {
        b.pane_active
            .cmp(&a.pane_active)
            .then_with(|| b.window_active.cmp(&a.window_active))
            .then_with(|| a.session_name.cmp(&b.session_name))
            .then_with(|| a.window_index.cmp(&b.window_index))
            .then_with(|| a.pane_index.cmp(&b.pane_index))
    });
}

fn normalize_lower(value: &Option<String>) -> Option<String> {
    value.as_ref().map(|v| v.to_ascii_lowercase())
}

fn pane_matches_host(pane: &PaneInfo, host_contains: Option<&String>) -> bool {
    host_contains.is_none_or(|needle| {
        pane.hostname
            .as_ref()
            .is_some_and(|host| host.to_ascii_lowercase().contains(needle))
    })
}

fn pane_matches_cwd(pane: &PaneInfo, cwd_contains: Option<&String>) -> bool {
    cwd_contains.is_none_or(|needle| {
        pane.cwd
            .as_ref()
            .is_some_and(|cwd| cwd.to_ascii_lowercase().contains(needle))
    })
}

fn pane_has_capability(pane: &PaneInfo, capability: PaneControlCapability) -> bool {
    pane.control_capabilities.contains(&capability)
}

fn pane_has_tmux_native(pane: &PaneInfo) -> bool {
    pane.tmux_control
        .as_ref()
        .is_some_and(|tmux| tmux.mode == TmuxControlMode::Native)
}

fn pane_has_tmux_layer(pane: &PaneInfo) -> bool {
    pane.tmux_control.is_some() || pane.tmux_session.is_some()
}

fn pane_has_remote_shell_context(pane: &PaneInfo) -> bool {
    pane.hostname.is_some()
        || pane
            .runtime_stack
            .iter()
            .any(|scope| scope.kind == crate::context::PaneScopeKind::RemoteShell)
        || pane
            .last_verified_runtime_stack
            .iter()
            .any(|scope| scope.kind == crate::context::PaneScopeKind::RemoteShell)
}

fn pane_is_visible_agent_cli(pane: &PaneInfo, agent_name: Option<&String>) -> bool {
    let matches_name =
        |value: &str| agent_name.is_none_or(|needle| value.to_ascii_lowercase().contains(needle));
    pane.visible_target.kind == PaneVisibleTargetKind::AgentCli
        || pane.agent_cli.as_deref().is_some_and(matches_name)
}

fn sort_work_target_candidates(candidates: &mut [(i32, WorkTargetCandidate)]) {
    candidates.sort_by(|(score_a, cand_a), (score_b, cand_b)| {
        score_b
            .cmp(score_a)
            .then_with(|| cand_a.pane_index.cmp(&cand_b.pane_index))
            .then_with(|| cand_a.pane_title.cmp(&cand_b.pane_title))
    });
}

fn make_visible_shell_candidate(pane: &PaneInfo, reason: String) -> WorkTargetCandidate {
    WorkTargetCandidate {
        pane_index: pane.index,
        pane_title: pane.title.clone(),
        host: pane.hostname.clone(),
        control_path: WorkTargetControlPath::VisibleShellExec,
        visible_target: pane.visible_target.clone(),
        tmux_mode: pane.tmux_control.as_ref().map(|tmux| tmux.mode),
        tmux_target: None,
        requires_preparation: false,
        suggested_tool: "terminal_exec".to_string(),
        reason,
    }
}

fn make_tmux_workspace_candidate(
    pane: &PaneInfo,
    reason: String,
    suggested_tool: &str,
) -> WorkTargetCandidate {
    WorkTargetCandidate {
        pane_index: pane.index,
        pane_title: pane.title.clone(),
        host: pane.hostname.clone(),
        control_path: WorkTargetControlPath::TmuxQuery,
        visible_target: pane.visible_target.clone(),
        tmux_mode: pane.tmux_control.as_ref().map(|tmux| tmux.mode),
        tmux_target: None,
        requires_preparation: false,
        suggested_tool: suggested_tool.to_string(),
        reason,
    }
}

fn make_tmux_target_candidate(
    pane: &PaneInfo,
    tmux_target: Option<crate::tmux::TmuxPaneInfo>,
    control_path: WorkTargetControlPath,
    requires_preparation: bool,
    suggested_tool: &str,
    reason: String,
) -> WorkTargetCandidate {
    WorkTargetCandidate {
        pane_index: pane.index,
        pane_title: pane.title.clone(),
        host: pane.hostname.clone(),
        control_path,
        visible_target: pane.visible_target.clone(),
        tmux_mode: pane.tmux_control.as_ref().map(|tmux| tmux.mode),
        tmux_target,
        requires_preparation,
        suggested_tool: suggested_tool.to_string(),
        reason,
    }
}

fn resolve_work_target_candidates(
    pane_tx: &Sender<PaneRequest>,
    panes: Vec<PaneInfo>,
    intent: WorkTargetIntent,
    preferred_pane_index: Option<usize>,
    host_contains: Option<String>,
    cwd_contains: Option<String>,
    agent_name: Option<String>,
    limit: usize,
) -> Result<ResolveWorkTargetResult, ToolError> {
    let host_contains = normalize_lower(&host_contains);
    let cwd_contains = normalize_lower(&cwd_contains);
    let agent_name = normalize_lower(&agent_name);
    let mut candidates = Vec::new();

    for pane in panes {
        if let Some(preferred_index) = preferred_pane_index {
            if pane.index != preferred_index {
                continue;
            }
        }
        if !pane_matches_host(&pane, host_contains.as_ref()) {
            continue;
        }

        match intent {
            WorkTargetIntent::VisibleShell => {
                if !pane_has_capability(&pane, PaneControlCapability::ExecVisibleShell)
                    || !pane_matches_cwd(&pane, cwd_contains.as_ref())
                {
                    continue;
                }
                let mut score = 100;
                if pane.is_focused {
                    score += 5;
                }
                if pane_has_remote_shell_context(&pane) {
                    score += 10;
                }
                let reason = if pane_has_remote_shell_context(&pane) {
                    "This pane exposes visible shell execution and has proven remote shell context."
                        .to_string()
                } else {
                    "This pane exposes visible shell execution on the current shell prompt."
                        .to_string()
                };
                candidates.push((score, make_visible_shell_candidate(&pane, reason)));
            }
            WorkTargetIntent::RemoteShell => {
                if !pane_has_capability(&pane, PaneControlCapability::ExecVisibleShell)
                    || !pane_has_remote_shell_context(&pane)
                    || !pane_matches_cwd(&pane, cwd_contains.as_ref())
                {
                    continue;
                }
                let mut score = 120;
                if pane.is_focused {
                    score += 5;
                }
                if pane.hostname.is_some() {
                    score += 10;
                }
                candidates.push((
                    score,
                    make_visible_shell_candidate(
                        &pane,
                        "This pane is a proven remote shell target with visible shell execution."
                            .to_string(),
                    ),
                ));
            }
            WorkTargetIntent::TmuxWorkspace => {
                if !pane_has_tmux_layer(&pane) {
                    continue;
                }
                let (score, suggested_tool, reason) = if pane_has_tmux_native(&pane) {
                    (
                        140 + if pane.is_focused { 1 } else { 0 },
                        "tmux_list_targets",
                        "This pane has a native tmux control anchor, so tmux targets can be queried directly.",
                    )
                } else {
                    (
                        80 + if pane.is_focused { 1 } else { 0 },
                        "probe_shell_context",
                        "This pane has a tmux layer, but native tmux control is not established yet.",
                    )
                };
                candidates.push((
                    score,
                    make_tmux_workspace_candidate(&pane, reason.to_string(), suggested_tool),
                ));
            }
            WorkTargetIntent::TmuxShell => {
                if !pane_has_tmux_layer(&pane) {
                    continue;
                }
                if pane_has_tmux_native(&pane) {
                    let snapshot = pane_query_tmux_list(pane_tx, pane.index)?;
                    let mut matches = snapshot
                        .panes
                        .into_iter()
                        .filter(|tmux_pane| {
                            tmux_pane
                                .pane_current_command
                                .as_deref()
                                .is_some_and(is_tmux_shell_command)
                                && cwd_contains.as_ref().is_none_or(|needle| {
                                    tmux_pane
                                        .pane_current_path
                                        .as_deref()
                                        .unwrap_or_default()
                                        .to_ascii_lowercase()
                                        .contains(needle)
                                })
                        })
                        .collect::<Vec<_>>();
                    sort_tmux_matches(&mut matches);
                    if let Some(target) = matches.into_iter().next() {
                        candidates.push((
                            160 + if pane.is_focused { 1 } else { 0 },
                            make_tmux_target_candidate(
                                &pane,
                                Some(target),
                                WorkTargetControlPath::TmuxShellTarget,
                                false,
                                "tmux_send_keys",
                                "This pane has native tmux control and an existing tmux shell target that matches the request.".to_string(),
                            ),
                        ));
                    } else {
                        candidates.push((
                            150 + if pane.is_focused { 1 } else { 0 },
                            make_tmux_target_candidate(
                                &pane,
                                None,
                                WorkTargetControlPath::TmuxShellTarget,
                                true,
                                "tmux_ensure_shell_target",
                                "This pane has native tmux control, but no matching shell target is currently known. Use tmux_ensure_shell_target first.".to_string(),
                            ),
                        ));
                    }
                } else {
                    candidates.push((
                        70 + if pane.is_focused { 1 } else { 0 },
                        make_tmux_workspace_candidate(
                            &pane,
                            "This pane appears to be a tmux workspace, but a native tmux control anchor is not established yet.".to_string(),
                            "probe_shell_context",
                        ),
                    ));
                }
            }
            WorkTargetIntent::AgentCli => {
                if pane_is_visible_agent_cli(&pane, agent_name.as_ref()) {
                    candidates.push((
                        170 + if pane.is_focused { 1 } else { 0 },
                        WorkTargetCandidate {
                            pane_index: pane.index,
                            pane_title: pane.title.clone(),
                            host: pane.hostname.clone(),
                            control_path: WorkTargetControlPath::VisibleAgentUi,
                            visible_target: pane.visible_target.clone(),
                            tmux_mode: pane.tmux_control.as_ref().map(|tmux| tmux.mode),
                            tmux_target: None,
                            requires_preparation: false,
                            suggested_tool: "send_keys".to_string(),
                            reason: "The visible foreground target already appears to be the requested agent CLI.".to_string(),
                        },
                    ));
                    continue;
                }
                if pane_has_tmux_native(&pane) {
                    let snapshot = pane_query_tmux_list(pane_tx, pane.index)?;
                    let mut matches = snapshot
                        .panes
                        .into_iter()
                        .filter(|tmux_pane| {
                            tmux_pane
                                .pane_current_command
                                .as_deref()
                                .is_some_and(|command| {
                                    is_tmux_agent_cli_command(command)
                                        && agent_name.as_ref().is_none_or(|needle| {
                                            command.to_ascii_lowercase().contains(needle)
                                        })
                                })
                        })
                        .collect::<Vec<_>>();
                    sort_tmux_matches(&mut matches);
                    if let Some(target) = matches.into_iter().next() {
                        candidates.push((
                            165 + if pane.is_focused { 1 } else { 0 },
                            make_tmux_target_candidate(
                                &pane,
                                Some(target),
                                WorkTargetControlPath::TmuxAgentTarget,
                                false,
                                "tmux_send_keys",
                                "This pane has native tmux control and already contains a matching agent CLI target.".to_string(),
                            ),
                        ));
                    }
                }
            }
        }
    }

    sort_work_target_candidates(&mut candidates);
    let mut candidates = candidates
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect::<Vec<_>>();
    candidates.truncate(limit.max(1));

    Ok(ResolveWorkTargetResult {
        intent,
        best_match: candidates.first().cloned(),
        candidates,
    })
}

// ── list_panes tool ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListPanesArgs {}

pub struct ListPanesTool {
    pane_tx: Sender<PaneRequest>,
}

impl ListPanesTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }
}

impl Tool for ListPanesTool {
    const NAME: &'static str = "list_panes";
    type Error = ToolError;
    type Args = ListPanesArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List all terminal panes currently open. Returns each pane's index, title, working directory, dimensions, current verified front-state, current runtime_stack, last_verified_runtime_stack, backend-support flags, typed shell context, recent con actions, current visible-screen observation hints, and control state: address space, visible target, nested target_stack, explicit control_attachments, control channels, control capabilities, and notes. Use this before acting in tmux/TUI panes so you do not confuse a con pane with a tmux pane or over-trust stale shell metadata. If a pane exposes query_tmux, exec_tmux_command, or send_tmux_keys, prefer tmux-native tools over outer-pane send_keys.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn call(&self, _args: Self::Args) -> Result<Self::Output, Self::Error> {
        log::info!("[list_panes] Tool called, sending PaneQuery::List");
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);
        self.pane_tx
            .send(PaneRequest {
                query: PaneQuery::List,
                response_tx,
            })
            .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;
        log::info!("[list_panes] PaneRequest sent, waiting for response...");

        let response = tokio::task::block_in_place(|| {
            response_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .map_err(|e| {
                    log::error!("[list_panes] Timed out waiting for response: {}", e);
                    ToolError::CommandFailed("Pane query timed out".into())
                })
        })?;
        log::info!("[list_panes] Got response");

        match response {
            PaneResponse::PaneList(panes) => {
                serde_json::to_value(&panes).map_err(|e| ToolError::CommandFailed(e.to_string()))
            }
            PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
            _ => Err(ToolError::CommandFailed("Unexpected response".into())),
        }
    }
}

// ── tmux_inspect tool ──────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TmuxInspectArgs {
    pub pane_index: usize,
}

pub struct TmuxInspectTool {
    pane_tx: Sender<PaneRequest>,
}

impl TmuxInspectTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }
}

impl Tool for TmuxInspectTool {
    const NAME: &'static str = "tmux_inspect";
    type Error = ToolError;
    type Args = TmuxInspectArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Inspect the tmux adapter state for a specific con pane. Returns tmux adapter details only when tmux has been authoritatively detected for that pane, including the explicit reason native tmux pane/window control is or is not available. Use this when a pane's target_stack includes tmux.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pane_index": {
                        "type": "integer",
                        "description": "Target con pane index (from list_panes)"
                    }
                },
                "required": ["pane_index"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);
        self.pane_tx
            .send(PaneRequest {
                query: PaneQuery::InspectTmux {
                    pane_index: args.pane_index,
                },
                response_tx,
            })
            .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

        let response = tokio::task::block_in_place(|| {
            response_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .map_err(|_| ToolError::CommandFailed("Pane query timed out".into()))
        })?;

        match response {
            PaneResponse::TmuxInfo(info) => {
                serde_json::to_value(&info).map_err(|e| ToolError::CommandFailed(e.to_string()))
            }
            PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
            _ => Err(ToolError::CommandFailed("Unexpected response".into())),
        }
    }
}

// ── tmux_list tool ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TmuxListArgs {
    pub pane_index: usize,
}

pub struct TmuxListTool {
    pane_tx: Sender<PaneRequest>,
}

impl TmuxListTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }
}

impl Tool for TmuxListTool {
    const NAME: &'static str = "tmux_list_targets";
    type Error = ToolError;
    type Args = TmuxListArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List tmux panes across the current tmux session using a proven same-session tmux control anchor from the given con pane. This is the preferred first step whenever list_panes shows tmux native control. Returns tmux session/window/pane ids plus pane_current_command and pane_current_path so the agent can target Codex CLI, Claude Code, OpenCode, or shell panes inside tmux explicitly.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pane_index": {
                        "type": "integer",
                        "description": "Target con pane index (from list_panes). Requires tmux native control on that pane."
                    }
                },
                "required": ["pane_index"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);
        self.pane_tx
            .send(PaneRequest {
                query: PaneQuery::TmuxList {
                    pane_index: args.pane_index,
                },
                response_tx,
            })
            .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

        let response = tokio::task::block_in_place(|| {
            response_rx
                .recv_timeout(std::time::Duration::from_secs(12))
                .map_err(|_| ToolError::CommandFailed("tmux list timed out".into()))
        })?;

        match response {
            PaneResponse::TmuxList(snapshot) => {
                serde_json::to_value(&snapshot).map_err(|e| ToolError::CommandFailed(e.to_string()))
            }
            PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
            _ => Err(ToolError::CommandFailed("Unexpected response".into())),
        }
    }
}

// ── tmux_capture_pane tool ────────────────────────────────────────

#[derive(Deserialize)]
pub struct TmuxCaptureArgs {
    pub pane_index: usize,
    pub target: Option<String>,
    #[serde(default = "default_tmux_capture_lines")]
    pub lines: usize,
}

fn default_tmux_capture_lines() -> usize {
    120
}

pub struct TmuxCaptureTool {
    pane_tx: Sender<PaneRequest>,
}

impl TmuxCaptureTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }
}

impl Tool for TmuxCaptureTool {
    const NAME: &'static str = "tmux_capture_pane";
    type Error = ToolError;
    type Args = TmuxCaptureArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Capture scrollback from a tmux pane through a proven same-session tmux control anchor. This is the correct way to inspect a tmux pane's content without confusing it with the outer con pane. If target is omitted, captures the current tmux pane of the anchor shell.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pane_index": {
                        "type": "integer",
                        "description": "Target con pane index (from list_panes). Requires tmux native control on that pane."
                    },
                    "target": {
                        "type": ["string", "null"],
                        "description": "Optional tmux target such as %17, @3, or session:window.pane. When omitted, captures the current tmux pane of the anchor shell."
                    },
                    "lines": {
                        "type": "integer",
                        "description": "Number of recent lines to capture from the tmux pane (default: 120)."
                    }
                },
                "required": ["pane_index"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);
        self.pane_tx
            .send(PaneRequest {
                query: PaneQuery::TmuxCapture {
                    pane_index: args.pane_index,
                    target: args.target,
                    lines: args.lines,
                },
                response_tx,
            })
            .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

        let response = tokio::task::block_in_place(|| {
            response_rx
                .recv_timeout(std::time::Duration::from_secs(12))
                .map_err(|_| ToolError::CommandFailed("tmux capture timed out".into()))
        })?;

        match response {
            PaneResponse::TmuxCapture(capture) => {
                serde_json::to_value(&capture).map_err(|e| ToolError::CommandFailed(e.to_string()))
            }
            PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
            _ => Err(ToolError::CommandFailed("Unexpected response".into())),
        }
    }
}

// ── tmux_send_keys tool ───────────────────────────────────────────

#[derive(Deserialize)]
pub struct TmuxSendKeysArgs {
    pub pane_index: usize,
    pub target: String,
    pub literal_text: Option<String>,
    #[serde(default)]
    pub key_names: Vec<String>,
    #[serde(default)]
    pub append_enter: bool,
}

pub struct TmuxSendKeysTool {
    pane_tx: Sender<PaneRequest>,
}

impl TmuxSendKeysTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }
}

impl Tool for TmuxSendKeysTool {
    const NAME: &'static str = "tmux_send_keys";
    type Error = ToolError;
    type Args = TmuxSendKeysArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Send text or tmux key names to a specific tmux pane through a proven same-session tmux control anchor. This is the preferred interaction path for Codex CLI, Claude Code, OpenCode, or shell panes inside tmux. Use this instead of raw outer-pane input whenever tmux native control is available. Provide literal_text for typed text, key_names for tmux key tokens like Enter, Escape, C-c, Up, Down, or both.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pane_index": {
                        "type": "integer",
                        "description": "Target con pane index (from list_panes). Requires tmux native control on that pane."
                    },
                    "target": {
                        "type": "string",
                        "description": "tmux target such as %17, @3, or session:window.pane."
                    },
                    "literal_text": {
                        "type": ["string", "null"],
                        "description": "Optional literal text to send into the tmux target."
                    },
                    "key_names": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional tmux key names such as Enter, Escape, C-c, Up, Down."
                    },
                    "append_enter": {
                        "type": "boolean",
                        "description": "Whether to send Enter after literal_text and key_names. Defaults to false."
                    }
                },
                "required": ["pane_index", "target"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);
        self.pane_tx
            .send(PaneRequest {
                query: PaneQuery::TmuxSendKeys {
                    pane_index: args.pane_index,
                    target: args.target,
                    literal_text: args.literal_text,
                    key_names: args.key_names,
                    append_enter: args.append_enter,
                },
                response_tx,
            })
            .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

        let response = tokio::task::block_in_place(|| {
            response_rx
                .recv_timeout(std::time::Duration::from_secs(12))
                .map_err(|_| ToolError::CommandFailed("tmux send-keys timed out".into()))
        })?;

        match response {
            PaneResponse::Content(content) => Ok(content),
            PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
            _ => Err(ToolError::CommandFailed("Unexpected response".into())),
        }
    }
}

// ── tmux_run_command tool ─────────────────────────────────────────

#[derive(Deserialize)]
pub struct TmuxRunCommandArgs {
    pub pane_index: usize,
    pub location: TmuxExecLocation,
    pub command: String,
    pub target: Option<String>,
    pub window_name: Option<String>,
    pub cwd: Option<String>,
    #[serde(default = "default_true")]
    pub detached: bool,
}

fn default_true() -> bool {
    true
}

pub struct TmuxRunCommandTool {
    pane_tx: Sender<PaneRequest>,
}

impl TmuxRunCommandTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }
}

impl Tool for TmuxRunCommandTool {
    const NAME: &'static str = "tmux_run_command";
    type Error = ToolError;
    type Args = TmuxRunCommandArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Run a command through tmux itself by creating a new tmux window or split pane from a proven same-session tmux control anchor. This is the preferred way to launch a fresh shell, Codex CLI, Claude Code, OpenCode, or any long-running command inside tmux without typing through the currently visible app.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pane_index": {
                        "type": "integer",
                        "description": "Target con pane index (from list_panes). Requires tmux native control on that pane."
                    },
                    "location": {
                        "type": "string",
                        "enum": ["new_window", "split_horizontal", "split_vertical"],
                        "description": "Where to create the tmux target."
                    },
                    "command": {
                        "type": "string",
                        "description": "Command to run inside the new tmux target."
                    },
                    "target": {
                        "type": ["string", "null"],
                        "description": "Optional tmux target for placement, such as a session for new_window or an existing pane/window for split operations."
                    },
                    "window_name": {
                        "type": ["string", "null"],
                        "description": "Optional tmux window name. Only used for new_window."
                    },
                    "cwd": {
                        "type": ["string", "null"],
                        "description": "Optional working directory for the new tmux target."
                    },
                    "detached": {
                        "type": "boolean",
                        "description": "Whether the new tmux target should start detached. Defaults to true."
                    }
                },
                "required": ["pane_index", "location", "command"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);
        self.pane_tx
            .send(PaneRequest {
                query: PaneQuery::TmuxRunCommand {
                    pane_index: args.pane_index,
                    target: args.target,
                    location: args.location,
                    command: args.command,
                    window_name: args.window_name,
                    cwd: args.cwd,
                    detached: args.detached,
                },
                response_tx,
            })
            .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

        let response = tokio::task::block_in_place(|| {
            response_rx
                .recv_timeout(std::time::Duration::from_secs(12))
                .map_err(|_| ToolError::CommandFailed("tmux run-command timed out".into()))
        })?;

        match response {
            PaneResponse::TmuxExec(exec) => {
                serde_json::to_value(&exec).map_err(|e| ToolError::CommandFailed(e.to_string()))
            }
            PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
            _ => Err(ToolError::CommandFailed("Unexpected response".into())),
        }
    }
}

// ── tmux_find_targets tool ────────────────────────────────────────

#[derive(Deserialize)]
pub struct TmuxFindTargetsArgs {
    pub pane_index: usize,
    #[serde(default)]
    pub kind: Option<TmuxTargetKindFilter>,
    pub command_contains: Option<String>,
    pub path_contains: Option<String>,
    #[serde(default = "default_tmux_find_limit")]
    pub limit: usize,
}

fn default_tmux_find_limit() -> usize {
    8
}

pub struct TmuxFindTargetsTool {
    pane_tx: Sender<PaneRequest>,
}

impl TmuxFindTargetsTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }
}

impl Tool for TmuxFindTargetsTool {
    const NAME: &'static str = "tmux_find_targets";
    type Error = ToolError;
    type Args = TmuxFindTargetsArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Find useful tmux pane targets through a proven same-session tmux control anchor. Use this helper instead of hand-filtering tmux_list_targets when you need a shell pane, an agent CLI pane, or a command/path match inside tmux.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pane_index": {
                        "type": "integer",
                        "description": "Target con pane index (from list_panes). Requires tmux native control on that pane."
                    },
                    "kind": {
                        "type": ["string", "null"],
                        "enum": ["any", "shell", "agent_cli", null],
                        "description": "Optional target class filter."
                    },
                    "command_contains": {
                        "type": ["string", "null"],
                        "description": "Optional case-insensitive substring match against tmux pane_current_command."
                    },
                    "path_contains": {
                        "type": ["string", "null"],
                        "description": "Optional case-insensitive substring match against tmux pane_current_path."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of matches to return. Defaults to 8."
                    }
                },
                "required": ["pane_index"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let snapshot = pane_query_tmux_list(&self.pane_tx, args.pane_index)?;
        let kind = args.kind.unwrap_or(TmuxTargetKindFilter::Any);
        let command_contains = args
            .command_contains
            .as_ref()
            .map(|v| v.to_ascii_lowercase());
        let path_contains = args.path_contains.as_ref().map(|v| v.to_ascii_lowercase());

        let mut matches = snapshot
            .panes
            .into_iter()
            .filter(|pane| {
                let command = pane
                    .pane_current_command
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                let path = pane
                    .pane_current_path
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase();

                let kind_match = match kind {
                    TmuxTargetKindFilter::Any => true,
                    TmuxTargetKindFilter::Shell => pane
                        .pane_current_command
                        .as_deref()
                        .is_some_and(is_tmux_shell_command),
                    TmuxTargetKindFilter::AgentCli => pane
                        .pane_current_command
                        .as_deref()
                        .is_some_and(is_tmux_agent_cli_command),
                };

                let command_match = command_contains
                    .as_ref()
                    .is_none_or(|needle| command.contains(needle));
                let path_match = path_contains
                    .as_ref()
                    .is_none_or(|needle| path.contains(needle));

                kind_match && command_match && path_match
            })
            .collect::<Vec<_>>();

        sort_tmux_matches(&mut matches);
        matches.truncate(args.limit.max(1));

        let result = TmuxFindTargetsResult {
            kind,
            best_match: matches.first().cloned(),
            matches,
        };
        serde_json::to_value(&result).map_err(|e| ToolError::CommandFailed(e.to_string()))
    }
}

// ── tmux_ensure_shell_target tool ─────────────────────────────────

#[derive(Deserialize)]
pub struct TmuxEnsureShellTargetArgs {
    pub pane_index: usize,
    pub cwd: Option<String>,
    pub window_name: Option<String>,
    pub shell_command: Option<String>,
    #[serde(default = "default_tmux_shell_location")]
    pub location: TmuxExecLocation,
    #[serde(default = "default_true")]
    pub detached: bool,
}

fn default_tmux_shell_location() -> TmuxExecLocation {
    TmuxExecLocation::NewWindow
}

pub struct TmuxEnsureShellTargetTool {
    pane_tx: Sender<PaneRequest>,
}

impl TmuxEnsureShellTargetTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }
}

impl Tool for TmuxEnsureShellTargetTool {
    const NAME: &'static str = "tmux_ensure_shell_target";
    type Error = ToolError;
    type Args = TmuxEnsureShellTargetArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Return a tmux pane that is suitable for shell work. Reuses an existing shell pane when one already matches, otherwise creates a fresh tmux shell target through the native tmux control channel. Use this before writing files remotely, launching commands away from a visible TUI, or preparing a clean shell workspace in tmux.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pane_index": {
                        "type": "integer",
                        "description": "Target con pane index (from list_panes). Requires tmux native control on that pane."
                    },
                    "cwd": {
                        "type": ["string", "null"],
                        "description": "Optional path hint. Existing tmux shell panes whose pane_current_path contains this value are preferred, and new shell targets inherit it when created."
                    },
                    "window_name": {
                        "type": ["string", "null"],
                        "description": "Optional name for a newly created tmux window."
                    },
                    "shell_command": {
                        "type": ["string", "null"],
                        "description": "Optional command for a newly created shell target. Defaults to an interactive login shell based on $SHELL."
                    },
                    "location": {
                        "type": "string",
                        "enum": ["new_window", "split_horizontal", "split_vertical"],
                        "description": "Where to create a new shell target if none already exists."
                    },
                    "detached": {
                        "type": "boolean",
                        "description": "Whether a newly created target should start detached. Defaults to true."
                    }
                },
                "required": ["pane_index"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let snapshot = pane_query_tmux_list(&self.pane_tx, args.pane_index)?;
        let cwd_contains = args.cwd.as_ref().map(|v| v.to_ascii_lowercase());

        let mut shell_matches = snapshot
            .panes
            .into_iter()
            .filter(|pane| {
                pane.pane_current_command
                    .as_deref()
                    .is_some_and(is_tmux_shell_command)
                    && cwd_contains.as_ref().is_none_or(|needle| {
                        pane.pane_current_path
                            .as_deref()
                            .unwrap_or_default()
                            .to_ascii_lowercase()
                            .contains(needle)
                    })
            })
            .collect::<Vec<_>>();
        sort_tmux_matches(&mut shell_matches);

        if let Some(pane) = shell_matches.into_iter().next() {
            let result = TmuxEnsureShellTargetResult {
                created: false,
                pane,
                creation: None,
            };
            return serde_json::to_value(&result)
                .map_err(|e| ToolError::CommandFailed(e.to_string()));
        }

        let shell_command = args
            .shell_command
            .unwrap_or_else(|| "exec \"${SHELL:-/bin/sh}\" -il".to_string());
        let creation = pane_query_tmux_run_command(
            &self.pane_tx,
            args.pane_index,
            None,
            args.location,
            shell_command,
            args.window_name,
            args.cwd.clone(),
            args.detached,
        )?;

        let snapshot = pane_query_tmux_list(&self.pane_tx, args.pane_index)?;
        let pane = snapshot
            .panes
            .into_iter()
            .find(|pane| pane.target == creation.target || pane.pane_id == creation.pane_id)
            .unwrap_or_else(|| crate::tmux::TmuxPaneInfo {
                session_name: creation.session_name.clone(),
                window_id: creation.window_id.clone(),
                window_index: creation.window_index.clone(),
                window_name: creation.window_name.clone(),
                window_target: creation.window_target.clone(),
                pane_id: creation.pane_id.clone(),
                pane_index: creation.pane_index.clone(),
                target: creation.target.clone(),
                pane_active: !creation.detached,
                window_active: !creation.detached,
                pane_current_command: None,
                pane_current_path: args.cwd,
            });

        let result = TmuxEnsureShellTargetResult {
            created: true,
            pane,
            creation: Some(creation),
        };
        serde_json::to_value(&result).map_err(|e| ToolError::CommandFailed(e.to_string()))
    }
}

// ── resolve_work_target tool ─────────────────────────────────────

#[derive(Deserialize)]
pub struct ResolveWorkTargetArgs {
    pub intent: WorkTargetIntent,
    pub pane_index: Option<usize>,
    pub host_contains: Option<String>,
    pub cwd_contains: Option<String>,
    pub agent_name: Option<String>,
    #[serde(default = "default_tmux_find_limit")]
    pub limit: usize,
}

pub struct ResolveWorkTargetTool {
    pane_tx: Sender<PaneRequest>,
}

impl ResolveWorkTargetTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }
}

impl Tool for ResolveWorkTargetTool {
    const NAME: &'static str = "resolve_work_target";
    type Error = ToolError;
    type Args = ResolveWorkTargetArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Resolve the best pane or tmux target for a specific kind of work using con's typed control plane. Use this when multiple panes are open and you need to choose the right shell, the right tmux workspace, the best tmux shell pane, or a matching agent CLI target without re-deriving that logic from list_panes manually.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "intent": {
                        "type": "string",
                        "enum": ["visible_shell", "remote_shell", "tmux_workspace", "tmux_shell", "agent_cli"],
                        "description": "What kind of work target you need."
                    },
                    "pane_index": {
                        "type": ["integer", "null"],
                        "description": "Optional con pane index to constrain resolution to a single pane."
                    },
                    "host_contains": {
                        "type": ["string", "null"],
                        "description": "Optional case-insensitive filter for the effective host name."
                    },
                    "cwd_contains": {
                        "type": ["string", "null"],
                        "description": "Optional case-insensitive filter for working directory or tmux pane path."
                    },
                    "agent_name": {
                        "type": ["string", "null"],
                        "description": "Optional case-insensitive filter for an agent CLI name such as codex, claude, or opencode."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of candidates to return. Defaults to 8."
                    }
                },
                "required": ["intent"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let panes = pane_query_list(&self.pane_tx)?;
        let result = resolve_work_target_candidates(
            &self.pane_tx,
            panes,
            args.intent,
            args.pane_index,
            args.host_contains,
            args.cwd_contains,
            args.agent_name,
            args.limit,
        )?;
        serde_json::to_value(&result).map_err(|e| ToolError::CommandFailed(e.to_string()))
    }
}

// ── probe_shell_context tool ───────────────────────────────────────

#[derive(Deserialize)]
pub struct ProbeShellContextArgs {
    pub pane_index: usize,
}

pub struct ProbeShellContextTool {
    pane_tx: Sender<PaneRequest>,
}

impl ProbeShellContextTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }
}

impl Tool for ProbeShellContextTool {
    const NAME: &'static str = "probe_shell_context";
    type Error = ToolError;
    type Args = ProbeShellContextArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Run a read-only shell-scoped probe in a pane that has the probe_shell_context capability. Use this only when list_panes reports a proven fresh shell prompt. Returns authoritative shell facts from that shell frame such as hostname, pwd, SSH env, TMUX env, tmux session/window/pane ids, pane_current_command, pane_current_path, and NVIM_LISTEN_ADDRESS when available. This is shell-scope truth, not proof of the foreground app after control passes to tmux or another TUI.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pane_index": {
                        "type": "integer",
                        "description": "Target con pane index (from list_panes). Requires probe_shell_context capability."
                    }
                },
                "required": ["pane_index"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);
        self.pane_tx
            .send(PaneRequest {
                query: PaneQuery::ProbeShellContext {
                    pane_index: args.pane_index,
                },
                response_tx,
            })
            .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

        let response = tokio::task::block_in_place(|| {
            response_rx
                .recv_timeout(std::time::Duration::from_secs(12))
                .map_err(|_| ToolError::CommandFailed("Shell probe timed out".into()))
        })?;

        match response {
            PaneResponse::ShellProbe(info) => {
                serde_json::to_value(&info).map_err(|e| ToolError::CommandFailed(e.to_string()))
            }
            PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
            _ => Err(ToolError::CommandFailed("Unexpected response".into())),
        }
    }
}

// ── read_pane tool ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ReadPaneArgs {
    pub pane_index: usize,
    #[serde(default = "default_pane_lines")]
    pub lines: usize,
}

fn default_pane_lines() -> usize {
    50
}

pub struct ReadPaneTool {
    pane_tx: Sender<PaneRequest>,
}

impl ReadPaneTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }
}

impl Tool for ReadPaneTool {
    const NAME: &'static str = "read_pane";
    type Error = ToolError;
    type Args = ReadPaneArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read the recent visible output from a specific terminal pane. Use list_panes first to discover available panes and their indices.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pane_index": {
                        "type": "integer",
                        "description": "The pane index (from list_panes)"
                    },
                    "lines": {
                        "type": "integer",
                        "description": "Number of recent lines to read (default: 50)"
                    }
                },
                "required": ["pane_index"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);
        self.pane_tx
            .send(PaneRequest {
                query: PaneQuery::ReadContent {
                    pane_index: args.pane_index,
                    lines: args.lines,
                },
                response_tx,
            })
            .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

        let response = tokio::task::block_in_place(|| {
            response_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .map_err(|_| ToolError::CommandFailed("Pane query timed out".into()))
        })?;

        match response {
            PaneResponse::Content(content) => Ok(content),
            PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
            _ => Err(ToolError::CommandFailed("Unexpected response".into())),
        }
    }
}

// ── send_keys tool ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SendKeysArgs {
    pub pane_index: usize,
    pub keys: String,
}

/// Send raw keystrokes to any terminal pane. This is the low-level
/// primitive for interacting with TUIs, cancelling commands (Ctrl-C),
/// or sending input to any running process — not just shells.
pub struct SendKeysTool {
    pane_tx: Sender<PaneRequest>,
}

impl SendKeysTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }
}

impl Tool for SendKeysTool {
    const NAME: &'static str = "send_keys";
    type Error = ToolError;
    type Args = SendKeysArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Send raw keystrokes to a specific con terminal pane. Use this for direct TUI interaction, prompt-level shell input when exec_visible_shell is unavailable, and tmux prefix sequences only when tmux native control is unavailable. IMPORTANT: Always follow send_keys with read_pane to verify the action took effect. Common sequences: \\n (Enter), \\x1b (Escape), \\x03 (Ctrl-C), \\x02 (Ctrl-B, tmux prefix), \\x1b[A/B/C/D (arrow keys). For shell commands, prefer terminal_exec when exec_visible_shell is available. For tmux panes with query_tmux/send_tmux_keys, prefer tmux-native tools.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pane_index": {
                        "type": "integer",
                        "description": "The pane index (from list_panes)"
                    },
                    "keys": {
                        "type": "string",
                        "description": "Keystrokes to send. Supports escape sequences: \\n (Enter), \\t (Tab), \\x03 (Ctrl-C), \\x1b (Escape), \\x1b[A (Up arrow), etc."
                    }
                },
                "required": ["pane_index", "keys"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Decode escape sequences in the keys string
        let decoded = decode_key_escapes(&args.keys);
        log::info!(
            "[send_keys] pane={} raw={:?} decoded={:?} decoded_bytes={:?}",
            args.pane_index,
            args.keys,
            decoded,
            decoded.as_bytes()
        );

        // Split at standalone-ESC boundaries to avoid VT parser ambiguity.
        // When ESC (0x1B) is immediately followed by a printable char like 'g',
        // the terminal interprets "ESC g" as a 2-byte escape sequence instead of
        // a standalone ESC key followed by 'g'. We split the write at these points
        // and insert a small delay so the terminal processes ESC on its own.
        let segments = split_at_standalone_esc(&decoded);

        for (i, segment) in segments.iter().enumerate() {
            if i > 0 {
                // Brief delay so the terminal's VT parser commits the previous ESC
                // as a standalone key before receiving the next character.
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            let (response_tx, response_rx) = crossbeam_channel::bounded(1);
            self.pane_tx
                .send(PaneRequest {
                    query: PaneQuery::SendKeys {
                        pane_index: args.pane_index,
                        keys: segment.clone(),
                    },
                    response_tx,
                })
                .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

            let response = tokio::task::block_in_place(|| {
                response_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .map_err(|_| ToolError::CommandFailed("Pane query timed out".into()))
            })?;

            match response {
                PaneResponse::KeysSent => {}
                PaneResponse::Error(e) => return Err(ToolError::CommandFailed(e)),
                _ => return Err(ToolError::CommandFailed("Unexpected response".into())),
            }
        }

        Ok("Keys sent successfully".to_string())
    }
}

// ── batch_exec tool ──────────────────────────────────────────────
//
// Execute commands across multiple panes in parallel. This is critical
// for multi-pane workflows: "run uptime on all machines" executes
// concurrently instead of sequentially (which would take N * timeout).
//
// Works within Rig's sequential tool dispatch constraint by batching
// what would otherwise be N sequential terminal_exec calls into one.

#[derive(Deserialize)]
pub struct BatchExecArgs {
    pub commands: Vec<BatchCommand>,
}

#[derive(Deserialize, Debug)]
pub struct BatchCommand {
    pub command: String,
    pub pane_index: usize,
}

#[derive(Serialize)]
struct BatchResult {
    pane_index: usize,
    output: String,
    exit_code: Option<i32>,
    error: Option<String>,
}

pub struct BatchExecTool {
    request_tx: Sender<TerminalExecRequest>,
}

impl BatchExecTool {
    pub fn new(request_tx: Sender<TerminalExecRequest>) -> Self {
        Self { request_tx }
    }
}

impl Tool for BatchExecTool {
    const NAME: &'static str = "batch_exec";
    type Error = ToolError;
    type Args = BatchExecArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Execute commands across multiple con panes in PARALLEL, but only when each pane's control_capabilities include exec_visible_shell. pane_index values come from list_panes and do not refer to tmux panes/windows/editors.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "commands": {
                        "type": "array",
                        "description": "List of commands to execute, each targeting a specific pane",
                        "items": {
                            "type": "object",
                            "properties": {
                                "command": {
                                    "type": "string",
                                    "description": "The shell command to execute"
                                },
                                "pane_index": {
                                    "type": "integer",
                                    "description": "Target pane index (from list_panes)"
                                }
                            },
                            "required": ["command", "pane_index"]
                        }
                    }
                },
                "required": ["commands"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        if args.commands.is_empty() {
            return Ok(serde_json::Value::Array(vec![]));
        }

        log::info!(
            "[batch_exec] Executing {} commands in parallel",
            args.commands.len()
        );

        // Send all commands concurrently, collect response receivers
        let mut receivers = Vec::with_capacity(args.commands.len());
        for cmd in &args.commands {
            let (response_tx, response_rx) = crossbeam_channel::bounded(1);
            self.request_tx
                .send(TerminalExecRequest {
                    command: cmd.command.clone(),
                    working_dir: None,
                    pane_index: Some(cmd.pane_index),
                    response_tx,
                })
                .map_err(|_| ToolError::CommandFailed("Terminal exec channel closed".into()))?;
            receivers.push((cmd.pane_index, response_rx));
        }

        // Wait for all results concurrently with timeout
        let results: Vec<BatchResult> = tokio::task::block_in_place(|| {
            receivers
                .into_iter()
                .map(
                    |(pane_index, rx)| match rx.recv_timeout(std::time::Duration::from_secs(60)) {
                        Ok(resp) => BatchResult {
                            pane_index,
                            output: resp.output,
                            exit_code: resp.exit_code,
                            error: None,
                        },
                        Err(_) => BatchResult {
                            pane_index,
                            output: String::new(),
                            exit_code: None,
                            error: Some("Timed out (60s)".into()),
                        },
                    },
                )
                .collect()
        });

        serde_json::to_value(&results).map_err(|e| ToolError::CommandFailed(e.to_string()))
    }
}

// ── search_panes tool ─────────────────────────────────────────────
//
// Search scrollback + visible screen across one or all panes.
// Invaluable for finding previous command output, error messages,
// or any text the user has seen in any pane.

#[derive(Deserialize)]
pub struct SearchPanesArgs {
    pub pattern: String,
    #[serde(default)]
    pub pane_index: Option<usize>,
    #[serde(default = "default_max_matches")]
    pub max_matches: usize,
}

fn default_max_matches() -> usize {
    50
}

pub struct SearchPanesTool {
    pane_tx: Sender<PaneRequest>,
}

impl SearchPanesTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }
}

impl Tool for SearchPanesTool {
    const NAME: &'static str = "search_panes";
    type Error = ToolError;
    type Args = SearchPanesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search terminal scrollback and visible screen for text. Searches across all panes or a specific pane. Use this to find previous command output, error messages, or any text that appeared in any terminal pane.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {
                        "type": "string",
                        "description": "Text to search for (case-insensitive substring match)"
                    },
                    "pane_index": {
                        "type": "integer",
                        "description": "Optional: search only this pane (from list_panes). Omit to search all panes."
                    },
                    "max_matches": {
                        "type": "integer",
                        "description": "Maximum number of matching lines to return (default: 50)"
                    }
                },
                "required": ["pattern"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);
        self.pane_tx
            .send(PaneRequest {
                query: PaneQuery::SearchText {
                    pane_index: args.pane_index,
                    pattern: args.pattern.clone(),
                    max_matches: args.max_matches,
                },
                response_tx,
            })
            .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

        let response = tokio::task::block_in_place(|| {
            response_rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .map_err(|_| ToolError::CommandFailed("Search timed out".into()))
        })?;

        match response {
            PaneResponse::SearchResults(results) => {
                if results.is_empty() {
                    return Ok(format!("No matches found for '{}'", args.pattern));
                }
                let mut output = String::new();
                let mut current_pane = 0;
                for (pane_idx, line_num, text) in &results {
                    if *pane_idx != current_pane {
                        current_pane = *pane_idx;
                        output.push_str(&format!("\n── Pane {} ──\n", pane_idx));
                    }
                    output.push_str(&format!("{:>5}: {}\n", line_num, text));
                }
                Ok(output)
            }
            PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
            _ => Err(ToolError::CommandFailed("Unexpected response".into())),
        }
    }
}

// ── wait_for tool ──────────────────────────────────────────────────
//
// Polls a pane until it becomes idle (is_busy == false) or until a
// pattern appears in the pane's recent output. The polling loop lives
// entirely in the tool — the workspace just answers List and ReadContent
// queries as usual.

#[derive(Deserialize)]
pub struct WaitForArgs {
    pub pane_index: usize,
    pub timeout_secs: Option<u64>,
    pub pattern: Option<String>,
}

#[derive(Serialize)]
pub struct WaitForOutput {
    pub status: String,
    pub output: String,
}

pub struct WaitForTool {
    pane_tx: Sender<PaneRequest>,
    cancel_flag: Arc<AtomicBool>,
}

impl WaitForTool {
    pub fn new(pane_tx: Sender<PaneRequest>, cancel_flag: Arc<AtomicBool>) -> Self {
        Self {
            pane_tx,
            cancel_flag,
        }
    }
}

impl Tool for WaitForTool {
    const NAME: &'static str = "wait_for";
    type Error = ToolError;
    type Args = WaitForArgs;
    type Output = WaitForOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Wait for a terminal pane to become idle or for a specific pattern to appear. Use after launching a command to wait for it to finish. Without a pattern, waits for idle — works universally (shell integration or output quiescence). With a pattern, polls until the text appears. Prefer idle mode (no pattern). Returns status: idle, matched, or timeout. On timeout, read_pane to check progress and call wait_for again if needed.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pane_index": {
                        "type": "integer",
                        "description": "Target pane index (from list_panes)"
                    },
                    "pattern": {
                        "type": "string",
                        "description": "Text to wait for in pane output. If omitted, waits for the pane to become idle — preferred."
                    }
                },
                "required": ["pane_index"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let timeout_secs = args.timeout_secs.unwrap_or(30).min(120);

        log::info!(
            "[wait_for] pane={} timeout={}s pattern={:?}",
            args.pane_index,
            timeout_secs,
            args.pattern
        );

        // Delegate to workspace — it has direct terminal access and runs an
        // async GPUI task with 100ms/500ms polling, no channel roundtrips per tick.
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);
        self.pane_tx
            .send(PaneRequest {
                query: PaneQuery::WaitFor {
                    pane_index: args.pane_index,
                    timeout_secs: Some(timeout_secs),
                    pattern: args.pattern,
                },
                response_tx,
            })
            .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

        // Block until workspace responds, polling cancel_flag every 200ms
        // for clean shutdown (same pattern as ConHook approval polling).
        let cancel = self.cancel_flag.clone();
        let response = tokio::task::block_in_place(|| {
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs + 10);
            loop {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err("Cancelled");
                }
                match response_rx.recv_timeout(std::time::Duration::from_millis(200)) {
                    Ok(resp) => return Ok(resp),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                        if std::time::Instant::now() >= deadline {
                            return Err("Wait timed out (no response from workspace)");
                        }
                    }
                    Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                        return Err("Pane query channel closed");
                    }
                }
            }
        })
        .map_err(|e| ToolError::CommandFailed(e.into()))?;

        match response {
            PaneResponse::WaitComplete { status, output } => Ok(WaitForOutput { status, output }),
            PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
            _ => Err(ToolError::CommandFailed("Unexpected response".into())),
        }
    }
}

/// Decode common escape sequences in key strings.
/// Decode escape sequences in keys strings from LLMs.
///
/// Supports multiple notations that LLMs commonly produce:
/// - Standard: `\n`, `\t`, `\r`, `\\`
/// - Hex with backslash: `\x1b`, `\x03` (preferred notation)
/// - Hex without backslash: `x1b`, `x03` (weaker models omit the backslash)
///
/// The bare-hex fallback (`x1b`) is only triggered when the two hex digits
/// form a valid control character (0x00–0x1F) or DEL (0x7F), so it won't
/// corrupt normal text like "exit" or "maximum".
fn decode_key_escapes(input: &str) -> String {
    let mut result = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            match bytes[i + 1] {
                b'n' => {
                    result.push(b'\n');
                    i += 2;
                }
                b't' => {
                    result.push(b'\t');
                    i += 2;
                }
                b'r' => {
                    result.push(b'\r');
                    i += 2;
                }
                b'\\' => {
                    result.push(b'\\');
                    i += 2;
                }
                b'x' if i + 3 < bytes.len() => {
                    let hex = &input[i + 2..i + 4];
                    if let Ok(byte) = u8::from_str_radix(hex, 16) {
                        result.push(byte);
                        i += 4;
                    } else {
                        result.push(bytes[i]);
                        i += 1;
                    }
                }
                _ => {
                    result.push(bytes[i]);
                    i += 1;
                }
            }
        } else if bytes[i] == b'x' && i + 2 < bytes.len() {
            // Bare hex without backslash: "x1b", "x03", etc.
            // Weaker models (kimi-k2 via Groq) consistently omit the backslash.
            // Only match control characters (0x00-0x1F) and DEL (0x7F)
            // to minimize false positives in normal text.
            let hex = &input[i + 1..i + 3];
            if hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    if byte <= 0x1F || byte == 0x7F {
                        result.push(byte);
                        i += 3;
                        continue;
                    }
                }
            }
            result.push(bytes[i]);
            i += 1;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&result).into_owned()
}

/// Split a decoded key string at standalone-ESC boundaries.
///
/// In VT100 terminals, `ESC` followed by a byte in `0x40..=0x7E` forms a
/// 2-character escape sequence. When an LLM sends `\x1bggdG` intending
/// "ESC (exit insert) then gg then dG", the terminal's VT parser instead
/// consumes `ESC g` as one sequence, eating the ESC and the first `g`.
///
/// This function splits the input so that each standalone ESC (not followed
/// by `[`, `O`, or `P` which start valid multi-byte sequences like CSI, SS3,
/// DCS) becomes its own segment. The caller inserts a brief delay between
/// segments so the VT parser commits ESC as a standalone key.
///
/// Returns a vec of segments. If there are no standalone-ESC boundaries,
/// returns the original string as a single-element vec.
fn split_at_standalone_esc(input: &str) -> Vec<String> {
    let bytes = input.as_bytes();
    if bytes.is_empty() {
        return vec![input.to_string()];
    }

    let mut segments: Vec<String> = Vec::new();
    let mut start = 0;

    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1B {
            // Check what follows ESC
            let next = bytes.get(i + 1).copied();
            match next {
                // CSI (ESC [), SS3 (ESC O), DCS (ESC P) — valid multi-byte
                // sequence prefixes. Don't split these.
                Some(b'[') | Some(b'O') | Some(b'P') => {
                    i += 1;
                    continue;
                }
                // ESC at end of string — no split needed
                None => {
                    i += 1;
                    continue;
                }
                // Standalone ESC followed by a non-sequence char.
                // Split into: [start..i) as one segment, then ESC alone,
                // then [i+1..] starts the next segment.
                Some(_) => {
                    // Push any accumulated bytes before this ESC
                    if start < i {
                        let seg = String::from_utf8_lossy(&bytes[start..i]).into_owned();
                        segments.push(seg);
                    }
                    // Push ESC as its own segment
                    segments.push("\x1b".to_string());
                    start = i + 1;
                    i = start;
                    continue;
                }
            }
        }
        i += 1;
    }

    // Remaining bytes
    if start < bytes.len() {
        let seg = String::from_utf8_lossy(&bytes[start..]).into_owned();
        segments.push(seg);
    }

    if segments.is_empty() {
        vec![input.to_string()]
    } else {
        segments
    }
}

/// Simple glob matching for filename filtering.
/// Supports `*` (matches any sequence) and `?` (matches one char).
fn glob_match(pattern: &str, text: &str) -> bool {
    let mut p = pattern.chars().peekable();
    let mut t = text.chars().peekable();

    fn inner(
        p: &mut std::iter::Peekable<std::str::Chars>,
        t: &mut std::iter::Peekable<std::str::Chars>,
    ) -> bool {
        while let Some(&pc) = p.peek() {
            match pc {
                '*' => {
                    p.next();
                    while p.peek() == Some(&'*') {
                        p.next();
                    }
                    if p.peek().is_none() {
                        return true;
                    }
                    let mut t_clone = t.clone();
                    let p_save = p.clone();
                    loop {
                        let mut p_try = p_save.clone();
                        let mut t_try = t_clone.clone();
                        if inner(&mut p_try, &mut t_try) {
                            return true;
                        }
                        if t_clone.next().is_none() {
                            return false;
                        }
                    }
                }
                '?' => {
                    p.next();
                    if t.next().is_none() {
                        return false;
                    }
                }
                _ => {
                    p.next();
                    match t.next() {
                        Some(tc) if tc == pc => {}
                        _ => return false,
                    }
                }
            }
        }
        t.peek().is_none()
    }

    inner(&mut p, &mut t)
}

// ── create_pane tool ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreatePaneArgs {
    pub command: Option<String>,
}

#[derive(Serialize)]
pub struct CreatePaneOutput {
    pub pane_index: usize,
    pub command: Option<String>,
    /// Initial terminal output after pane creation and command execution.
    /// Gives the model immediate observability — no need for a follow-up read_pane.
    pub output: String,
}

pub struct CreatePaneTool {
    pane_tx: Sender<PaneRequest>,
}

impl CreatePaneTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }

    /// Wait for the new pane's command to produce meaningful output and settle.
    ///
    /// Two-phase detection:
    ///   Phase 1 (change): Wait for content to CHANGE from the initial command echo.
    ///     The local shell echoes the command immediately, but the actual program
    ///     (SSH handshake, server connection) takes 1-5s to produce output. If we
    ///     settle on the echo, we return before the program responds.
    ///   Phase 2 (stability): Once content changed, wait for it to stabilize.
    ///     The program's output (MOTD, prompt, error) streams in and eventually stops.
    ///
    /// Budget: 30 polls × 500ms = 15s total. Enough for SSH over slow networks.
    async fn wait_for_initial_output(&self, pane_index: usize) -> String {
        const POLL_MS: u64 = 500;
        const SETTLE_POLLS: u32 = 3; // 1.5s of stable output after change
        const MAX_POLLS: u32 = 30; // 15s total budget

        let mut initial_snapshot = String::new();
        let mut last_snapshot = String::new();
        let mut stable_count: u32 = 0;
        let mut phase_changed = false;

        for poll_num in 0..MAX_POLLS {
            tokio::time::sleep(std::time::Duration::from_millis(POLL_MS)).await;

            let (tx, rx) = crossbeam_channel::bounded(1);
            let sent = self.pane_tx.send(PaneRequest {
                query: PaneQuery::ReadContent {
                    pane_index,
                    lines: 50,
                },
                response_tx: tx,
            });
            if sent.is_err() {
                break;
            }

            let content = match rx.recv_timeout(std::time::Duration::from_secs(2)) {
                Ok(PaneResponse::Content(c)) => c,
                _ => continue,
            };

            // Normalize: trim trailing whitespace per line (cursor position artifacts)
            let normalized: String = content
                .lines()
                .map(|l| l.trim_end())
                .collect::<Vec<_>>()
                .join("\n");

            if normalized.is_empty() {
                continue;
            }

            if !phase_changed {
                // Phase 1: waiting for content to change from initial echo
                if initial_snapshot.is_empty() {
                    // First non-empty snapshot — likely command echo
                    initial_snapshot = normalized.clone();
                    last_snapshot = normalized;
                    log::info!(
                        "[create_pane] phase 1: initial echo captured ({} bytes)",
                        initial_snapshot.len()
                    );
                    continue;
                }

                if normalized != initial_snapshot {
                    // Content changed — the command produced actual output
                    phase_changed = true;
                    last_snapshot = normalized;
                    stable_count = 0;
                    log::info!(
                        "[create_pane] phase 2: content changed at poll {}",
                        poll_num
                    );
                    continue;
                }

                // Content unchanged from echo — command hasn't responded yet
                // (SSH handshake, network latency, etc.)
                continue;
            }

            // Phase 2: content has changed, wait for stability
            if normalized == last_snapshot {
                stable_count += 1;
                if stable_count >= SETTLE_POLLS {
                    log::info!(
                        "[create_pane] output settled at poll {} ({} bytes)",
                        poll_num,
                        normalized.len()
                    );
                    return normalized;
                }
            } else {
                last_snapshot = normalized;
                stable_count = 0;
            }
        }

        log::info!(
            "[create_pane] budget exhausted (phase_changed={}), returning ({} bytes)",
            phase_changed,
            last_snapshot.len()
        );
        last_snapshot
    }
}

impl Tool for CreatePaneTool {
    const NAME: &'static str = "create_pane";
    type Error = ToolError;
    type Args = CreatePaneArgs;
    type Output = CreatePaneOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Create a new terminal pane (split in current tab). Optionally run a startup command (e.g. \"ssh host\"). The command executes automatically — do NOT re-send it via send_keys. Returns the pane index and the initial terminal output (waits for output to settle). Check the output to see what happened — e.g. SSH connected with prompt, or asking for a password.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Optional startup command executed automatically in the new pane. Do NOT re-send this command — it already ran."
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        log::info!("[create_pane] Tool called, command: {:?}", args.command);
        let has_command = args.command.is_some();
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);
        let command_echo = args.command.clone();
        self.pane_tx
            .send(PaneRequest {
                query: PaneQuery::CreatePane {
                    command: args.command,
                },
                response_tx,
            })
            .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

        let response = tokio::task::block_in_place(|| {
            response_rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .map_err(|e| {
                    log::error!("[create_pane] Timed out waiting for response: {}", e);
                    ToolError::CommandFailed("Create pane request timed out".into())
                })
        })?;

        match response {
            PaneResponse::PaneCreated { pane_index } => {
                // If a command was provided (e.g. "ssh host"), wait for initial output
                // to settle so the model can observe the result immediately.
                // This eliminates the need for a follow-up read_pane/wait_for after SSH.
                let output = if has_command {
                    self.wait_for_initial_output(pane_index).await
                } else {
                    String::new()
                };

                Ok(CreatePaneOutput {
                    pane_index,
                    command: command_echo,
                    output,
                })
            }
            PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
            _ => Err(ToolError::CommandFailed("Unexpected response".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ResolveWorkTargetResult, TmuxTargetKindFilter, WorkTargetControlPath, WorkTargetIntent,
        decode_key_escapes, is_tmux_agent_cli_command, is_tmux_shell_command,
        resolve_work_target_candidates, split_at_standalone_esc,
    };
    use crate::context::{
        PaneConfidence, PaneEvidenceSource, PaneFrontState, PaneObservationSupport,
        PaneRuntimeScope, PaneScopeKind,
    };
    use crate::control::{
        PaneAddressSpace, PaneControlCapability, PaneVisibleTarget, PaneVisibleTargetKind,
        TmuxControlMode, TmuxControlState,
    };
    use crossbeam_channel::unbounded;

    #[test]
    fn decode_standard_escapes() {
        assert_eq!(decode_key_escapes(r"\n"), "\n");
        assert_eq!(decode_key_escapes(r"\t"), "\t");
        assert_eq!(decode_key_escapes(r"\r"), "\r");
        assert_eq!(decode_key_escapes(r"\\"), "\\");
    }

    #[test]
    fn decode_hex_with_backslash() {
        // \x1b → ESC (0x1B)
        let decoded = decode_key_escapes(r"\x1b");
        assert_eq!(decoded.as_bytes(), &[0x1B]);

        // \x03 → Ctrl-C (0x03)
        let decoded = decode_key_escapes(r"\x03");
        assert_eq!(decoded.as_bytes(), &[0x03]);

        // \x02 → Ctrl-B / tmux prefix (0x02)
        let decoded = decode_key_escapes(r"\x02");
        assert_eq!(decoded.as_bytes(), &[0x02]);
    }

    #[test]
    fn decode_bare_hex_control_chars() {
        // Weaker models (kimi-k2) omit the backslash: "x1b" → ESC
        let decoded = decode_key_escapes("x1b");
        assert_eq!(decoded.as_bytes(), &[0x1B], "bare x1b → ESC");

        let decoded = decode_key_escapes("x03");
        assert_eq!(decoded.as_bytes(), &[0x03], "bare x03 → Ctrl-C");

        // tmux prefix + window number
        let decoded = decode_key_escapes("x02c");
        assert_eq!(decoded.as_bytes(), &[0x02, b'c']);
    }

    #[test]
    fn bare_hex_safe_in_normal_text() {
        // Only control chars (≤0x1F, 0x7F) match — printable hex values don't
        assert_eq!(decode_key_escapes("exit"), "exit"); // 'x' + 'i' is not hex
        assert_eq!(decode_key_escapes("hexdump"), "hexdump"); // 'x' + 'd' + 'u' — 0xdu invalid
        assert_eq!(decode_key_escapes("x41"), "x41"); // 0x41 = 'A', not control
        assert_eq!(decode_key_escapes("maximum"), "maximum");
    }

    #[test]
    fn decode_vim_escape_save_sequence() {
        // LLM sends: \x1b:w\n (Escape, :w, Enter)
        let decoded = decode_key_escapes(r"\x1b:w\n");
        assert_eq!(decoded.as_bytes(), &[0x1B, b':', b'w', b'\n']);
    }

    #[test]
    fn decode_bare_hex_vim_sequence() {
        // kimi-k2 sends: x1b:wqx0a (Escape, :wq, newline)
        let decoded = decode_key_escapes("x1b:wqx0a");
        assert_eq!(decoded.as_bytes(), &[0x1B, b':', b'w', b'q', 0x0A]);
    }

    #[test]
    fn decode_mixed_content() {
        // "hello\nworld" → "hello" + newline + "world"
        let decoded = decode_key_escapes(r"hello\nworld");
        assert_eq!(decoded, "hello\nworld");

        // Normal text with no escapes
        assert_eq!(decode_key_escapes("ls -la"), "ls -la");
    }

    #[test]
    fn tmux_shell_command_detection_uses_basename() {
        assert!(is_tmux_shell_command("zsh"));
        assert!(is_tmux_shell_command("/bin/bash"));
        assert!(!is_tmux_shell_command("codex"));
    }

    #[test]
    fn tmux_agent_cli_detection_handles_known_names() {
        assert!(is_tmux_agent_cli_command("codex"));
        assert!(is_tmux_agent_cli_command("/usr/local/bin/opencode"));
        assert!(is_tmux_agent_cli_command("claude-code"));
        assert!(!is_tmux_agent_cli_command("zsh"));
    }

    #[test]
    fn tmux_target_kind_filter_serializes_snake_case() {
        let json = serde_json::to_string(&TmuxTargetKindFilter::AgentCli).expect("serialize");
        assert_eq!(json, "\"agent_cli\"");
    }

    fn test_pane(index: usize, title: &str) -> super::PaneInfo {
        super::PaneInfo {
            index,
            title: title.to_string(),
            cwd: None,
            is_focused: false,
            rows: 24,
            cols: 80,
            is_alive: true,
            hostname: None,
            hostname_confidence: None,
            hostname_source: None,
            front_state: PaneFrontState::Unknown,
            mode: super::PaneMode::Unknown,
            shell_metadata_fresh: false,
            shell_context_fresh: false,
            observation_support: PaneObservationSupport::default(),
            address_space: PaneAddressSpace::ConPane,
            visible_target: PaneVisibleTarget {
                kind: PaneVisibleTargetKind::Unknown,
                label: None,
                host: None,
            },
            target_stack: Vec::new(),
            tmux_control: None,
            control_attachments: Vec::new(),
            control_channels: Vec::new(),
            control_capabilities: Vec::new(),
            control_notes: Vec::new(),
            active_scope: None,
            agent_cli: None,
            evidence: Vec::new(),
            runtime_stack: Vec::new(),
            last_verified_runtime_stack: Vec::new(),
            runtime_warnings: Vec::new(),
            shell_context: None,
            recent_actions: Vec::new(),
            screen_hints: Vec::new(),
            tmux_session: None,
            has_shell_integration: false,
            last_command: None,
            last_exit_code: None,
            is_busy: false,
        }
    }

    #[test]
    fn resolve_work_target_prefers_remote_visible_shell() {
        let (pane_tx, _pane_rx) = unbounded();
        let mut local = test_pane(1, "local");
        local.control_capabilities = vec![PaneControlCapability::ExecVisibleShell];
        local.visible_target.kind = PaneVisibleTargetKind::ShellPrompt;
        local.front_state = PaneFrontState::ShellPrompt;
        local.mode = super::PaneMode::Shell;
        local.shell_metadata_fresh = true;

        let mut remote = test_pane(2, "remote");
        remote.control_capabilities = vec![PaneControlCapability::ExecVisibleShell];
        remote.visible_target.kind = PaneVisibleTargetKind::ShellPrompt;
        remote.front_state = PaneFrontState::ShellPrompt;
        remote.mode = super::PaneMode::Shell;
        remote.shell_metadata_fresh = true;
        remote.hostname = Some("haswell".to_string());
        remote.hostname_confidence = Some(PaneConfidence::Strong);
        remote.hostname_source = Some(PaneEvidenceSource::ShellProbe);
        remote.runtime_stack = vec![PaneRuntimeScope {
            kind: PaneScopeKind::RemoteShell,
            label: Some("haswell".to_string()),
            host: Some("haswell".to_string()),
            confidence: PaneConfidence::Strong,
            evidence_source: PaneEvidenceSource::ShellProbe,
        }];

        let result = resolve_work_target_candidates(
            &pane_tx,
            vec![local, remote],
            WorkTargetIntent::RemoteShell,
            None,
            None,
            None,
            None,
            8,
        )
        .expect("resolve");

        let best = result.best_match.expect("best");
        assert_eq!(best.pane_index, 2);
        assert_eq!(best.control_path, WorkTargetControlPath::VisibleShellExec);
    }

    #[test]
    fn resolve_work_target_prefers_native_tmux_workspace() {
        let (pane_tx, _pane_rx) = unbounded();
        let mut inspect_only = test_pane(1, "inspect-only tmux");
        inspect_only.tmux_control = Some(TmuxControlState {
            session_name: Some("work".to_string()),
            mode: TmuxControlMode::InspectOnly,
            front_target: None,
            reason: "inspect only".to_string(),
        });
        inspect_only.tmux_session = Some("work".to_string());

        let mut native = test_pane(2, "native tmux");
        native.tmux_control = Some(TmuxControlState {
            session_name: Some("ops".to_string()),
            mode: TmuxControlMode::Native,
            front_target: None,
            reason: "native".to_string(),
        });
        native.tmux_session = Some("ops".to_string());
        native.control_capabilities = vec![PaneControlCapability::QueryTmux];

        let result: ResolveWorkTargetResult = resolve_work_target_candidates(
            &pane_tx,
            vec![inspect_only, native],
            WorkTargetIntent::TmuxWorkspace,
            None,
            None,
            None,
            None,
            8,
        )
        .expect("resolve");

        let best = result.best_match.expect("best");
        assert_eq!(best.pane_index, 2);
        assert_eq!(best.control_path, WorkTargetControlPath::TmuxQuery);
        assert_eq!(best.suggested_tool, "tmux_list_targets");
    }

    #[test]
    fn resolve_work_target_detects_visible_agent_cli() {
        let (pane_tx, _pane_rx) = unbounded();
        let mut agent = test_pane(3, "codex");
        agent.visible_target.kind = PaneVisibleTargetKind::AgentCli;
        agent.agent_cli = Some("codex".to_string());

        let result = resolve_work_target_candidates(
            &pane_tx,
            vec![agent],
            WorkTargetIntent::AgentCli,
            None,
            None,
            None,
            Some("codex".to_string()),
            8,
        )
        .expect("resolve");

        let best = result.best_match.expect("best");
        assert_eq!(best.pane_index, 3);
        assert_eq!(best.control_path, WorkTargetControlPath::VisibleAgentUi);
        assert_eq!(best.suggested_tool, "send_keys");
    }

    // ── split_at_standalone_esc tests ────────────────────────────────

    #[test]
    fn split_esc_before_printable() {
        // ESC followed by 'g' — must split so ESC is processed alone
        let input = "\x1bggdG";
        let segments = split_at_standalone_esc(input);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0], "\x1b");
        assert_eq!(segments[1], "ggdG");
    }

    #[test]
    fn no_split_for_csi_sequence() {
        // ESC [ A is a CSI Up arrow — should NOT split
        let input = "\x1b[A";
        let segments = split_at_standalone_esc(input);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], "\x1b[A");
    }

    #[test]
    fn no_split_for_ss3_sequence() {
        // ESC O P is an SS3 F1 key — should NOT split
        let input = "\x1bOP";
        let segments = split_at_standalone_esc(input);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], "\x1bOP");
    }

    #[test]
    fn split_esc_colon_w() {
        // ESC :w\n — common vim "exit insert + save" sequence
        let input = "\x1b:w\n";
        let segments = split_at_standalone_esc(input);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0], "\x1b");
        assert_eq!(segments[1], ":w\n");
    }

    #[test]
    fn no_split_for_plain_text() {
        let segments = split_at_standalone_esc("hello world");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], "hello world");
    }

    #[test]
    fn esc_at_end_no_split() {
        // ESC at the very end — no following char, no need to split
        let input = "hello\x1b";
        let segments = split_at_standalone_esc(input);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0], "hello\x1b");
    }

    #[test]
    fn multiple_standalone_esc() {
        // ESC gg dG ESC :wq\n — two standalone ESCs
        let input = "\x1bggdG\x1b:wq\n";
        let segments = split_at_standalone_esc(input);
        assert_eq!(segments, vec!["\x1b", "ggdG", "\x1b", ":wq\n"]);
    }

    #[test]
    fn mixed_standalone_and_csi_esc() {
        // ESC (standalone) then gg then ESC[A (CSI Up arrow)
        let input = "\x1bgg\x1b[A";
        let segments = split_at_standalone_esc(input);
        assert_eq!(segments, vec!["\x1b", "gg\x1b[A"]);
    }
}
