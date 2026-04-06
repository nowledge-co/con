use crossbeam_channel::Sender;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::context::PaneMode;
use crate::control::{
    PaneAddressSpace, PaneControlCapability, PaneControlChannel, PaneVisibleTarget,
    TmuxControlState,
};

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
            description: "Execute a command visibly in a con terminal pane only when that pane's control_capabilities include exec_visible_shell. pane_index refers to a con pane from list_panes, not a tmux pane/window/editor target. For tmux, vim, nvim, Codex CLI, Claude Code, and other TUIs, inspect first and use send_keys only when intentional.".to_string(),
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
/// Includes connection state to prevent executing commands on the wrong
/// host (e.g., running a remote command locally after SSH disconnects).
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
    /// Effective hostname inferred from pane-local evidence.
    pub hostname: Option<String>,
    /// Confidence for the effective hostname, when detected.
    pub hostname_confidence: Option<crate::context::PaneConfidence>,
    /// Evidence source for the effective hostname, when detected.
    pub hostname_source: Option<crate::context::PaneEvidenceSource>,
    /// Current pane mode: shell, tmux-like multiplexer, or another TUI.
    pub mode: PaneMode,
    /// Whether shell metadata like cwd and last_command is likely fresh for the visible app.
    pub shell_metadata_fresh: bool,
    /// The only address space valid for pane_index today.
    pub address_space: PaneAddressSpace,
    /// The best-known visible target inside this con pane.
    pub visible_target: PaneVisibleTarget,
    /// Nested runtime/control targets from outer shell toward the front-most visible app.
    pub target_stack: Vec<PaneVisibleTarget>,
    /// tmux adapter state when a tmux layer is present in this pane.
    pub tmux_control: Option<TmuxControlState>,
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
    /// Warnings about stale or advisory runtime metadata.
    pub runtime_warnings: Vec<String>,
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
    /// Search results: Vec of (pane_index, line_number, line_text).
    SearchResults(Vec<(usize, usize, String)>),
    /// Response from a wait_for operation.
    WaitComplete { status: String, output: String },
    /// A new pane was created successfully.
    PaneCreated { pane_index: usize },
    Error(String),
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
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List all terminal panes currently open. Returns each pane's index, title, working directory, dimensions, runtime state, and control state: address space, visible target, nested target_stack, control channels, control capabilities, and notes. Use this before acting in tmux/TUI panes so you do not confuse a con pane with a tmux pane or over-trust stale shell metadata.".to_string(),
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
            PaneResponse::PaneList(panes) => serde_json::to_string_pretty(&panes)
                .map_err(|e| ToolError::CommandFailed(e.to_string())),
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
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Inspect the tmux adapter state for a specific con pane. Returns the detected tmux session, tmux control mode, the front-most target inside tmux, and the reason native tmux pane/window control is or is not available. Use this when a pane's target_stack includes tmux.".to_string(),
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
            PaneResponse::TmuxInfo(info) => serde_json::to_string_pretty(&info)
                .map_err(|e| ToolError::CommandFailed(e.to_string())),
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
            description: "Send raw keystrokes to a specific con terminal pane. This is THE primary tool for interacting with tmux, vim/nvim, and other TUIs. IMPORTANT: Always follow send_keys with read_pane to verify the action took effect. Common sequences: \\n (Enter), \\x1b (Escape), \\x03 (Ctrl-C), \\x02 (Ctrl-B, tmux prefix), \\x1b[A/B/C/D (arrow keys). For shell commands, prefer terminal_exec when exec_visible_shell is available.".to_string(),
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
    type Output = String;

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
            return Ok("[]".to_string());
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

        serde_json::to_string_pretty(&results).map_err(|e| ToolError::CommandFailed(e.to_string()))
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
}

impl WaitForTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }

    /// Send a PaneQuery::List and return the PaneInfo for the target pane.
    fn query_pane_info(&self, pane_index: usize) -> Result<PaneInfo, ToolError> {
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);
        self.pane_tx
            .send(PaneRequest {
                query: PaneQuery::List,
                response_tx,
            })
            .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

        let response = response_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| ToolError::CommandFailed("Pane query timed out".into()))?;

        match response {
            PaneResponse::PaneList(panes) => panes
                .into_iter()
                .find(|p| p.index == pane_index)
                .ok_or_else(|| {
                    ToolError::CommandFailed(format!("Pane {} not found", pane_index))
                }),
            PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
            _ => Err(ToolError::CommandFailed("Unexpected response".into())),
        }
    }

    /// Send a PaneQuery::ReadContent and return the text.
    fn query_pane_content(&self, pane_index: usize, lines: usize) -> Result<String, ToolError> {
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);
        self.pane_tx
            .send(PaneRequest {
                query: PaneQuery::ReadContent { pane_index, lines },
                response_tx,
            })
            .map_err(|_| ToolError::CommandFailed("Pane query channel closed".into()))?;

        let response = response_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|_| ToolError::CommandFailed("Pane query timed out".into()))?;

        match response {
            PaneResponse::Content(content) => Ok(content),
            PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
            _ => Err(ToolError::CommandFailed("Unexpected response".into())),
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
            description: "Wait for a terminal pane to become idle or for a specific pattern to appear in its output. Use after terminal_exec or send_keys when you need to wait for a long-running command to finish or for specific output to appear before proceeding. Without a pattern, waits for the shell to become idle (requires shell integration). With a pattern, polls the last 50 lines of output until the pattern is found.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pane_index": {
                        "type": "integer",
                        "description": "Target pane index (from list_panes)"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Maximum seconds to wait (default: 120, max: 600)"
                    },
                    "pattern": {
                        "type": "string",
                        "description": "Optional text pattern to wait for in the pane output. If omitted, waits for the pane to become idle (is_busy == false)."
                    }
                },
                "required": ["pane_index"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let timeout_secs = args.timeout_secs.unwrap_or(120).min(600);
        let poll_interval = std::time::Duration::from_millis(500);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

        log::info!(
            "[wait_for] pane={} timeout={}s pattern={:?}",
            args.pane_index,
            timeout_secs,
            args.pattern
        );

        loop {
            if std::time::Instant::now() >= deadline {
                // Timeout — grab final output and return
                let output = tokio::task::block_in_place(|| {
                    self.query_pane_content(args.pane_index, 50)
                })
                .unwrap_or_default();
                return Ok(WaitForOutput {
                    status: "timeout".to_string(),
                    output,
                });
            }

            let check_result: Result<Option<WaitForOutput>, ToolError> =
                tokio::task::block_in_place(|| {
                    if let Some(ref pattern) = args.pattern {
                        // Pattern mode: check last 50 lines for the pattern
                        let content = self.query_pane_content(args.pane_index, 50)?;
                        if content.contains(pattern.as_str()) {
                            return Ok(Some(WaitForOutput {
                                status: "matched".to_string(),
                                output: content,
                            }));
                        }
                        Ok(None)
                    } else {
                        // Idle mode: check is_busy flag
                        let info = self.query_pane_info(args.pane_index)?;
                        if !info.is_busy {
                            let content = self.query_pane_content(args.pane_index, 50)?;
                            return Ok(Some(WaitForOutput {
                                status: "idle".to_string(),
                                output: content,
                            }));
                        }
                        Ok(None)
                    }
                });
            let check_result = check_result?;

            if let Some(result) = check_result {
                return Ok(result);
            }

            tokio::time::sleep(poll_interval).await;
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
                        let seg =
                            String::from_utf8_lossy(&bytes[start..i]).into_owned();
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

pub struct CreatePaneTool {
    pane_tx: Sender<PaneRequest>,
}

impl CreatePaneTool {
    pub fn new(pane_tx: Sender<PaneRequest>) -> Self {
        Self { pane_tx }
    }
}

impl Tool for CreatePaneTool {
    const NAME: &'static str = "create_pane";
    type Error = ToolError;
    type Args = CreatePaneArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Create a new terminal pane (split in current tab). Optionally run a command in it (e.g. \"ssh cinnamon\"). Returns the new pane's index for use with other tools.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Optional command to run in the new pane (e.g. \"ssh cinnamon\", \"htop\")"
                    }
                }
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        log::info!("[create_pane] Tool called, command: {:?}", args.command);
        let (response_tx, response_rx) = crossbeam_channel::bounded(1);
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
                Ok(format!("Created new pane with index {}", pane_index))
            }
            PaneResponse::Error(e) => Err(ToolError::CommandFailed(e)),
            _ => Err(ToolError::CommandFailed("Unexpected response".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_key_escapes, split_at_standalone_esc};

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
        assert_eq!(decode_key_escapes("exit"), "exit");    // 'x' + 'i' is not hex
        assert_eq!(decode_key_escapes("hexdump"), "hexdump"); // 'x' + 'd' + 'u' — 0xdu invalid
        assert_eq!(decode_key_escapes("x41"), "x41");      // 0x41 = 'A', not control
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
