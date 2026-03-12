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

// ── shell_exec ──────────────────────────────────────────────────────

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
            description: "Execute a shell command. The command runs in a visible terminal pane — the user sees everything.".to_string(),
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
