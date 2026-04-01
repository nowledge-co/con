use crossbeam_channel::Sender;
use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Error type for agent tool execution
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("command failed: {0}")]
    CommandFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

// ── terminal_exec (visible) ─────────────────────────────────────────

/// Request to execute a command in the visible terminal.
/// The workspace polls for these and writes the command to the focused PTY.
#[derive(Debug)]
pub struct TerminalExecRequest {
    pub command: String,
    pub working_dir: Option<String>,
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
            description: "Execute a command in the user's visible terminal. The user sees the command run in real time. Prefer this over shell_exec for transparency.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute in the visible terminal"
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
            description: "Execute a shell command in a background process. Output is captured but not shown in the terminal. Use terminal_exec instead for visible execution.".to_string(),
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

pub struct FileReadTool;

impl Tool for FileReadTool {
    const NAME: &'static str = "file_read";
    type Error = ToolError;
    type Args = FileReadArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Read the contents of a file.".to_string(),
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
        let content = std::fs::read_to_string(&args.path)?;
        let lines: Vec<&str> = content.lines().collect();
        let start = args.start_line.unwrap_or(1).saturating_sub(1);
        let end = args.end_line.unwrap_or(lines.len()).min(lines.len());
        Ok(lines[start..end].join("\n"))
    }
}

// ── file_write ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct FileWriteArgs {
    pub path: String,
    pub content: String,
}

pub struct FileWriteTool;

impl Tool for FileWriteTool {
    const NAME: &'static str = "file_write";
    type Error = ToolError;
    type Args = FileWriteArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Write content to a file. Creates the file if it doesn't exist.".to_string(),
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
        if let Some(parent) = Path::new(&args.path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&args.path, &args.content)?;
        Ok(format!("Wrote {} bytes to {}", args.content.len(), args.path))
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
pub struct EditFileTool;

impl Tool for EditFileTool {
    const NAME: &'static str = "edit_file";
    type Error = ToolError;
    type Args = EditFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Edit a file by replacing a specific text snippet. Safer than file_write — only changes the targeted section.".to_string(),
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
        let content = std::fs::read_to_string(&args.path)?;

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
        std::fs::write(&args.path, &new_content)?;

        // Generate a simple diff summary
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
pub struct ListFilesTool;

impl Tool for ListFilesTool {
    const NAME: &'static str = "list_files";
    type Error = ToolError;
    type Args = ListFilesArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "List files and directories. Returns a tree-like listing.".to_string(),
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
        let dir = args.path.as_deref().unwrap_or(".");
        let max_depth = args.max_depth.unwrap_or(3);

        // Try git ls-files first (respects .gitignore, fast)
        let git_listing = std::process::Command::new("git")
            .args(["ls-files", "--cached", "--others", "--exclude-standard"])
            .current_dir(dir)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
            .filter(|s| !s.trim().is_empty());

        // Fall back to find for non-git directories
        let stdout = match git_listing {
            Some(listing) => {
                // Apply pattern and max_depth filters that git ls-files doesn't handle
                let filtered: Vec<&str> = listing
                    .lines()
                    .filter(|path| {
                        // Respect max_depth: count path separators
                        let depth = path.chars().filter(|c| *c == '/').count();
                        if depth >= max_depth {
                            return false;
                        }
                        // Respect glob pattern: match against filename
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
                cmd.arg(dir);
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

pub struct SearchTool;

impl Tool for SearchTool {
    const NAME: &'static str = "search";
    type Error = ToolError;
    type Args = SearchArgs;
    type Output = SearchOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Search for text in files using grep. Returns matching lines with file paths and line numbers.".to_string(),
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
        let mut cmd = std::process::Command::new("grep");
        cmd.args(["-rn", "--max-count=100"]);
        if let Some(ref fp) = args.file_pattern {
            cmd.args(["--include", fp]);
        }
        cmd.arg(&args.pattern);
        cmd.arg(args.path.as_deref().unwrap_or("."));

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
                    // Skip consecutive stars
                    while p.peek() == Some(&'*') {
                        p.next();
                    }
                    if p.peek().is_none() {
                        return true; // trailing * matches everything
                    }
                    // Try matching * against 0, 1, 2, ... chars
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
