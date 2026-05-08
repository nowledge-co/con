/// Test runner — executes steps from a parsed .test file against a live con-cli binary.
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::parser::{MatchMode, Step, parse_file};

pub struct RunConfig {
    pub con_cli: PathBuf,
    pub socket: Option<PathBuf>,
    pub rewrite: bool,
    pub verbose: bool,
}

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
    let mut rewrite_steps: Vec<(usize, String)> = Vec::new(); // (step index, new expected)

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

enum StepOutcome {
    Pass,
    Fail(StepFailure),
    Skip(String),
}

fn run_step(step: &Step, config: &RunConfig) -> Result<StepOutcome> {
    // Build the con-cli invocation
    let mut cmd = Command::new(&config.con_cli);
    if let Some(socket) = &config.socket {
        cmd.arg("--socket").arg(socket);
    }
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
            // Use a simple hand-rolled check to avoid adding a regex dep.
            // For now we treat the pattern as a literal substring with `.*` support.
            // TODO: add the `regex` crate if more power is needed.
            Ok(StepOutcome::Skip(
                "regex match mode not yet implemented — use contains or exact".into(),
            ))
        }
    }
}

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
        (Value::Array(exp_arr), Value::Array(act_arr)) => {
            exp_arr.iter().all(|ev| act_arr.iter().any(|av| json_is_subset(ev, av)))
        }
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
            (Some(e), Some(a)) if e == a => {
                out.push_str(&format!("  {e}\n"));
            }
            (Some(e), Some(a)) => {
                out.push_str(&format!("- {e}\n"));
                out.push_str(&format!("+ {a}\n"));
            }
            (Some(e), None) => {
                out.push_str(&format!("- {e}\n"));
            }
            (None, Some(a)) => {
                out.push_str(&format!("+ {a}\n"));
            }
            (None, None) => {}
        }
    }
    out
}

/// Rewrite a .test file, replacing expected blocks for the given step indices.
fn rewrite_file(path: &Path, _steps: &[Step], rewrites: &[(usize, String)]) -> Result<()> {
    let source = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = source.lines().collect();

    // Build a map from step index → new expected text
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
            // Emit the cmd line as-is
            out.push_str(lines[i]);
            out.push('\n');
            i += 1;

            // Emit optional match line
            if i < lines.len() && lines[i].trim().starts_with("match ") {
                out.push_str(lines[i]);
                out.push('\n');
                i += 1;
            }

            // Emit ----
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

            // Emit new expected if we have a rewrite for this step
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
    if s.chars().any(|c| matches!(c, ' ' | '\t' | '"' | '\'' | '\\' | '#')) {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}
