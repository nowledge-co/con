/// Test runner — launches a con process, resets state between files, executes steps.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::parser::{MatchMode, Step, parse_file};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

pub struct RunConfig {
    pub con_cli: PathBuf,
    pub socket: PathBuf,
    pub rewrite: bool,
    pub verbose: bool,
}

// ---------------------------------------------------------------------------
// Con process lifecycle
// ---------------------------------------------------------------------------

/// RAII guard for a running con process.
/// Kills the process and removes the socket file on drop.
pub struct ConProcess {
    child: std::process::Child,
    socket: PathBuf,
}

impl ConProcess {
    /// Launch con with a dedicated socket path and wait until the socket is ready.
    pub fn launch(con_bin: &Path, socket: &Path, startup_timeout: Duration) -> Result<Self> {
        // Remove any stale socket from a previous run.
        let _ = std::fs::remove_file(socket);

        let child = Command::new(con_bin)
            .env("CON_SOCKET_PATH", socket)
            // Suppress con's own log output so it doesn't pollute test output.
            // Tests that need to inspect logs can override this.
            .env("RUST_LOG", "error")
            .spawn()
            .with_context(|| format!("failed to launch con from {}", con_bin.display()))?;

        let process = ConProcess {
            child,
            socket: socket.to_path_buf(),
        };

        process.wait_for_socket(startup_timeout)?;
        Ok(process)
    }

    /// Block until the socket file appears or the timeout expires.
    fn wait_for_socket(&self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.socket.exists() {
                // Give con a brief moment to finish binding before we connect.
                std::thread::sleep(Duration::from_millis(100));
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        bail!(
            "con did not create socket at {} within {:?}",
            self.socket.display(),
            timeout
        )
    }

}

impl Drop for ConProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait(); // reap the zombie
        let _ = std::fs::remove_file(&self.socket);
    }
}

// ---------------------------------------------------------------------------
// State reset
// ---------------------------------------------------------------------------

/// Reset con to a known baseline state before each test file:
///   - Close all tabs except the first one (con requires at least one tab).
///   - The surviving tab is tab index 1.
///
/// We re-query tabs after each close because indices shift on removal.
pub fn reset_state(con_cli: &Path, socket: &Path) -> Result<()> {
    loop {
        let output = Command::new(con_cli)
            .args(["--socket", &socket.to_string_lossy(), "--json", "tabs", "list"])
            .output()
            .context("reset_state: failed to run tabs list")?;

        if !output.status.success() {
            bail!(
                "reset_state: tabs list failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let json: Value = serde_json::from_slice(&output.stdout)
            .context("reset_state: tabs list returned invalid JSON")?;

        let tabs = json["tabs"]
            .as_array()
            .context("reset_state: missing tabs array")?;

        // Find any tab with index > 1 and close it.
        // We close one at a time and re-query so indices stay accurate.
        let extra = tabs
            .iter()
            .filter_map(|t| t["index"].as_u64())
            .find(|&idx| idx > 1);

        match extra {
            Some(idx) => {
                let output = Command::new(con_cli)
                    .args([
                        "--socket",
                        &socket.to_string_lossy(),
                        "tabs",
                        "close",
                        "--tab",
                        &idx.to_string(),
                    ])
                    .output()
                    .context("reset_state: failed to run tabs close")?;

                if !output.status.success() {
                    bail!(
                        "reset_state: tabs close --tab {} failed: {}",
                        idx,
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                // Small pause to let con process the close before we re-query.
                std::thread::sleep(Duration::from_millis(50));
            }
            None => break, // only tab 1 remains
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// File runner
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
pub struct FileResult {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub failures: Vec<StepFailure>,
}

#[derive(Debug)]
pub struct StepFailure {
    pub step_label: String,
    pub cmd: String,
    pub expected: String,
    pub actual: String,
    pub diff: String,
}

pub fn run_file(path: &Path, config: &RunConfig) -> Result<FileResult> {
    let steps = parse_file(path)?;
    let mut result = FileResult::default();
    let mut rewrite_steps: Vec<(usize, String)> = Vec::new();

    for (idx, step) in steps.iter().enumerate() {
        let outcome = run_step(step, config)?;
        match outcome {
            StepOutcome::Pass => {
                result.passed += 1;
                if config.verbose {
                    println!("    pass: {}", step.label);
                }
            }
            StepOutcome::Fail(failure) => {
                if config.rewrite {
                    rewrite_steps.push((idx, failure.actual.clone()));
                    result.passed += 1;
                } else {
                    result.failures.push(failure);
                    result.failed += 1;
                }
            }
            StepOutcome::Skip(reason) => {
                result.skipped += 1;
                if config.verbose {
                    println!("    skip: {} — {reason}", step.label);
                }
            }
        }
    }

    if config.rewrite && !rewrite_steps.is_empty() {
        rewrite_file(path, &steps, &rewrite_steps)
            .with_context(|| format!("failed to rewrite {}", path.display()))?;
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Step execution
// ---------------------------------------------------------------------------

enum StepOutcome {
    Pass,
    Fail(StepFailure),
    Skip(String),
}

fn run_step(step: &Step, config: &RunConfig) -> Result<StepOutcome> {
    let mut cmd = Command::new(&config.con_cli);
    cmd.arg("--socket").arg(&config.socket);
    cmd.args(&step.args);

    let output = cmd
        .output()
        .with_context(|| format!("failed to run con-cli for step {:?}", step.label))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let exit_ok = output.status.success();

    let cmd_display = format!(
        "con-cli {}",
        step.args
            .iter()
            .map(|a| shell_quote(a))
            .collect::<Vec<_>>()
            .join(" ")
    );

    match &step.match_mode {
        MatchMode::Ok => {
            if exit_ok {
                Ok(StepOutcome::Pass)
            } else {
                Ok(StepOutcome::Fail(StepFailure {
                    step_label: format!("{} (line {})", step.label, step.line),
                    cmd: cmd_display,
                    expected: "exit code 0".into(),
                    actual: format!(
                        "exit code {}\nstderr: {stderr}",
                        output.status.code().unwrap_or(-1)
                    ),
                    diff: String::new(),
                }))
            }
        }

        MatchMode::Error => {
            if !exit_ok {
                let actual = stderr.trim().to_string();
                check_text_match(&step.expected, &actual, step, &cmd_display, &stderr)
            } else {
                Ok(StepOutcome::Fail(StepFailure {
                    step_label: format!("{} (line {})", step.label, step.line),
                    cmd: cmd_display,
                    expected: "non-zero exit code".into(),
                    actual: format!("exit code 0\nstdout: {stdout}"),
                    diff: String::new(),
                }))
            }
        }

        MatchMode::Exact => {
            if !exit_ok {
                return Ok(StepOutcome::Fail(exit_failure(step, &cmd_display, &stderr)));
            }
            let actual = normalize_lines(&stdout);
            let expected = normalize_lines(&step.expected);
            if actual == expected {
                Ok(StepOutcome::Pass)
            } else {
                Ok(StepOutcome::Fail(StepFailure {
                    step_label: format!("{} (line {})", step.label, step.line),
                    cmd: cmd_display,
                    expected: step.expected.clone(),
                    actual: stdout.clone(),
                    diff: make_diff(&expected, &actual),
                }))
            }
        }

        MatchMode::Contains => {
            if !exit_ok {
                return Ok(StepOutcome::Fail(exit_failure(step, &cmd_display, &stderr)));
            }
            check_text_match(&step.expected, &stdout, step, &cmd_display, &stderr)
        }

        MatchMode::JsonSubset => {
            if !exit_ok {
                return Ok(StepOutcome::Fail(exit_failure(step, &cmd_display, &stderr)));
            }
            let actual_val: Value = serde_json::from_str(stdout.trim()).with_context(|| {
                format!(
                    "step {:?}: actual output is not valid JSON:\n{stdout}",
                    step.label
                )
            })?;
            let expected_val: Value =
                serde_json::from_str(step.expected.trim()).with_context(|| {
                    format!(
                        "step {:?}: expected block is not valid JSON:\n{}",
                        step.label, step.expected
                    )
                })?;
            if json_is_subset(&expected_val, &actual_val) {
                Ok(StepOutcome::Pass)
            } else {
                Ok(StepOutcome::Fail(StepFailure {
                    step_label: format!("{} (line {})", step.label, step.line),
                    cmd: cmd_display,
                    expected: serde_json::to_string_pretty(&expected_val).unwrap_or_default(),
                    actual: serde_json::to_string_pretty(&actual_val).unwrap_or_default(),
                    diff: format!(
                        "expected JSON is not a subset of actual JSON\nexpected: {}\nactual:   {}",
                        step.expected.trim(),
                        stdout.trim()
                    ),
                }))
            }
        }

        MatchMode::Regex => {
            if !exit_ok {
                return Ok(StepOutcome::Fail(exit_failure(step, &cmd_display, &stderr)));
            }
            Ok(StepOutcome::Skip(
                "regex match mode not yet implemented — use contains or exact".into(),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn check_text_match(
    expected: &str,
    actual: &str,
    step: &Step,
    cmd_display: &str,
    stderr: &str,
) -> Result<StepOutcome> {
    if expected.is_empty() || actual.contains(expected.trim()) {
        Ok(StepOutcome::Pass)
    } else {
        Ok(StepOutcome::Fail(StepFailure {
            step_label: format!("{} (line {})", step.label, step.line),
            cmd: cmd_display.to_string(),
            expected: expected.to_string(),
            actual: actual.to_string(),
            diff: if !stderr.is_empty() {
                format!("stderr: {stderr}")
            } else {
                String::new()
            },
        }))
    }
}

fn exit_failure(step: &Step, cmd_display: &str, stderr: &str) -> StepFailure {
    StepFailure {
        step_label: format!("{} (line {})", step.label, step.line),
        cmd: cmd_display.to_string(),
        expected: step.expected.clone(),
        actual: format!("con-cli exited with error\nstderr: {stderr}"),
        diff: String::new(),
    }
}

/// Recursively check that every key/value in `expected` exists in `actual`.
/// Arrays: every element in expected must appear somewhere in actual (order-independent).
fn json_is_subset(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Object(exp_map), Value::Object(act_map)) => exp_map.iter().all(|(k, ev)| {
            act_map
                .get(k)
                .map(|av| json_is_subset(ev, av))
                .unwrap_or(false)
        }),
        (Value::Array(exp_arr), Value::Array(act_arr)) => exp_arr
            .iter()
            .all(|ev| act_arr.iter().any(|av| json_is_subset(ev, av))),
        _ => expected == actual,
    }
}

/// Normalize line endings and trim trailing whitespace per line.
fn normalize_lines(s: &str) -> String {
    s.lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

/// Produce a simple unified-style diff between two strings.
fn make_diff(expected: &str, actual: &str) -> String {
    let exp_lines: Vec<&str> = expected.lines().collect();
    let act_lines: Vec<&str> = actual.lines().collect();
    let mut out = String::new();
    let max = exp_lines.len().max(act_lines.len());
    for i in 0..max {
        match (exp_lines.get(i), act_lines.get(i)) {
            (Some(e), Some(a)) if e == a => out.push_str(&format!("  {e}\n")),
            (Some(e), Some(a)) => {
                out.push_str(&format!("- {e}\n"));
                out.push_str(&format!("+ {a}\n"));
            }
            (Some(e), None) => out.push_str(&format!("- {e}\n")),
            (None, Some(a)) => out.push_str(&format!("+ {a}\n")),
            (None, None) => {}
        }
    }
    out
}

/// Rewrite a .test file, replacing expected blocks for the given step indices.
fn rewrite_file(path: &Path, _steps: &[Step], rewrites: &[(usize, String)]) -> Result<()> {
    let source = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = source.lines().collect();

    let rewrite_map: std::collections::HashMap<usize, &str> = rewrites
        .iter()
        .map(|(idx, new)| (*idx, new.as_str()))
        .collect();

    let mut out = String::new();
    let mut step_idx = 0usize;
    let mut i = 0usize;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if trimmed.starts_with("cmd ") || trimmed == "cmd" {
            out.push_str(lines[i]);
            out.push('\n');
            i += 1;

            if i < lines.len() && lines[i].trim().starts_with("match ") {
                out.push_str(lines[i]);
                out.push('\n');
                i += 1;
            }

            if i < lines.len() && lines[i].trim() == "----" {
                out.push_str(lines[i]);
                out.push('\n');
                i += 1;
            }

            // Skip old expected block
            while i < lines.len() {
                let t = lines[i].trim();
                if t.starts_with("cmd ") || t == "cmd" {
                    break;
                }
                if t.is_empty() {
                    i += 1;
                    break;
                }
                i += 1;
            }

            if let Some(new_expected) = rewrite_map.get(&step_idx) {
                for line in new_expected.lines() {
                    out.push_str(line);
                    out.push('\n');
                }
                out.push('\n');
            }

            step_idx += 1;
        } else {
            out.push_str(lines[i]);
            out.push('\n');
            i += 1;
        }
    }

    std::fs::write(path, out)?;
    Ok(())
}

/// Quote a shell argument if it contains spaces or special characters.
fn shell_quote(s: &str) -> String {
    if s.chars()
        .any(|c| matches!(c, ' ' | '\t' | '"' | '\'' | '\\' | '#'))
    {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}
