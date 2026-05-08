/// Parser for .test files.
///
/// File format:
///
/// ```text
/// # This is a comment
///
/// # A step block:
/// cmd <con-cli arguments...>
/// match <mode>          # optional, default: contains
/// ----
/// expected output here
/// (blank line or next cmd ends the expected block)
/// ```
///
/// Match modes:
///   exact        — full string equality (after trimming trailing whitespace per line)
///   contains     — actual output contains the expected string (default)
///   json-subset  — every key/value in expected JSON exists in actual JSON
///   regex        — expected is a regex pattern matched against actual
///   ok           — only checks that con-cli exited 0; expected block is ignored
///   error        — checks that con-cli exited non-zero; expected block matched against stderr
///
/// A step may have an optional label comment on the same line as `cmd`:
///   cmd --json tabs list   # verify initial tab count
///
/// Blank lines and lines starting with `#` outside a step block are ignored.
use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, PartialEq)]
pub enum MatchMode {
    /// Full string equality (trailing whitespace trimmed per line)
    Exact,
    /// Actual output contains the expected string
    Contains,
    /// Expected is valid JSON; every key present in expected must match in actual
    JsonSubset,
    /// Expected is a regex matched against actual
    Regex,
    /// Only check exit code == 0; ignore expected block
    Ok,
    /// Check exit code != 0; match expected against stderr
    Error,
}

impl Default for MatchMode {
    fn default() -> Self {
        MatchMode::Contains
    }
}

impl MatchMode {
    fn parse(s: &str) -> Result<Self> {
        match s.trim() {
            "exact" => Ok(MatchMode::Exact),
            "contains" => Ok(MatchMode::Contains),
            "json-subset" => Ok(MatchMode::JsonSubset),
            "regex" => Ok(MatchMode::Regex),
            "ok" => Ok(MatchMode::Ok),
            "error" => Ok(MatchMode::Error),
            other => bail!("unknown match mode {:?}; valid: exact, contains, json-subset, regex, ok, error", other),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Step {
    /// Human-readable label (from inline comment on cmd line, or auto-generated)
    pub label: String,
    /// Arguments to pass to con-cli (already split on whitespace, respecting quotes)
    pub args: Vec<String>,
    pub match_mode: MatchMode,
    /// Expected output (empty for `ok` / `error` with no expected block)
    pub expected: String,
    /// 1-based line number of the `cmd` directive in the source file
    pub line: usize,
}

pub fn parse_file(path: &std::path::Path) -> Result<Vec<Step>> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read {}", path.display()))?;
    parse_source(&source, path.display().to_string())
}

pub fn parse_source(source: &str, origin: String) -> Result<Vec<Step>> {
    let mut steps = Vec::new();
    let lines: Vec<&str> = source.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let raw = lines[i];
        let trimmed = raw.trim();

        // Skip blank lines and standalone comments
        if trimmed.is_empty() || (trimmed.starts_with('#') && !trimmed.starts_with("cmd ")) {
            i += 1;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("cmd ").or_else(|| {
            if trimmed == "cmd" { Some("") } else { None }
        }) {
            let cmd_line = i + 1; // 1-based

            // Split inline label comment: `cmd foo bar  # my label`
            let (args_str, label) = split_inline_comment(rest);
            let args = shell_split(args_str.trim())
                .with_context(|| format!("{origin}:{cmd_line}: failed to parse cmd arguments"))?;

            let label = if label.is_empty() {
                format!("line {cmd_line}: {}", args_str.trim())
            } else {
                label.to_string()
            };

            i += 1;

            // Optional `match <mode>` line
            let mut match_mode = MatchMode::default();
            if i < lines.len() {
                let next = lines[i].trim();
                if let Some(mode_str) = next.strip_prefix("match ") {
                    match_mode = MatchMode::parse(mode_str)
                        .with_context(|| format!("{origin}:{}: invalid match mode", i + 1))?;
                    i += 1;
                }
            }

            // Expect `----` separator
            if i >= lines.len() || lines[i].trim() != "----" {
                bail!(
                    "{origin}:{}: expected `----` after cmd/match block, got {:?}",
                    i + 1,
                    lines.get(i).unwrap_or(&"<eof>")
                );
            }
            i += 1; // skip ----

            // Collect expected output until blank line or next `cmd` or EOF
            let mut expected_lines: Vec<&str> = Vec::new();
            while i < lines.len() {
                let line = lines[i];
                let t = line.trim();
                if t.starts_with("cmd ") || t == "cmd" {
                    break;
                }
                // A standalone blank line ends the expected block
                if t.is_empty() && expected_lines.last().map(|l: &&str| l.trim().is_empty()).unwrap_or(true) {
                    // Only break on the *first* blank line after content
                    if !expected_lines.is_empty() {
                        break;
                    }
                }
                expected_lines.push(line);
                i += 1;
            }

            // Trim trailing blank lines from expected block
            while expected_lines.last().map(|l: &&str| l.trim().is_empty()).unwrap_or(false) {
                expected_lines.pop();
            }

            let expected = expected_lines.join("\n");

            steps.push(Step {
                label,
                args,
                match_mode,
                expected,
                line: cmd_line,
            });
        } else {
            bail!(
                "{origin}:{}: unexpected line {:?} (expected `cmd`, `#`, or blank)",
                i + 1,
                trimmed
            );
        }
    }

    Ok(steps)
}

/// Split `foo bar  # comment` into (`foo bar`, `comment`).
/// Returns the original string and empty comment if no `#` found.
fn split_inline_comment(s: &str) -> (&str, &str) {
    // Find ` #` that is not inside a quoted string (simple heuristic: first ` #`)
    let mut in_single = false;
    let mut in_double = false;
    let bytes = s.as_bytes();
    for idx in 0..bytes.len() {
        match bytes[idx] {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'#' if !in_single && !in_double => {
                if idx == 0 || bytes[idx - 1] == b' ' {
                    let args = s[..idx].trim_end();
                    let comment = s[idx + 1..].trim();
                    return (args, comment);
                }
            }
            _ => {}
        }
    }
    (s, "")
}

/// Very small shell-word splitter: handles double-quoted strings and backslash escapes.
/// Does not support single-quoted strings or variable expansion (not needed here).
pub fn shell_split(s: &str) -> Result<Vec<String>> {
    let mut args: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_double = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' => in_double = !in_double,
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            ' ' | '\t' if !in_double => {
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(c),
        }
    }
    if in_double {
        bail!("unterminated double-quoted string in: {s:?}");
    }
    if !current.is_empty() {
        args.push(current);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_step() {
        let src = r#"
cmd --json identify
----
{"version":
"#;
        let steps = parse_source(src, "test".into()).unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].args, vec!["--json", "identify"]);
        assert_eq!(steps[0].match_mode, MatchMode::Contains);
        assert!(steps[0].expected.contains("{\"version\":"));
    }

    #[test]
    fn parse_match_mode_exact() {
        let src = "cmd --json tabs list\nmatch exact\n----\n{}\n";
        let steps = parse_source(src, "test".into()).unwrap();
        assert_eq!(steps[0].match_mode, MatchMode::Exact);
    }

    #[test]
    fn parse_multiple_steps() {
        let src = r#"
cmd identify
----
con

cmd --json tabs list
match json-subset
----
{"tabs":[]}
"#;
        let steps = parse_source(src, "test".into()).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].match_mode, MatchMode::JsonSubset);
    }

    #[test]
    fn parse_ok_mode_empty_expected() {
        let src = "cmd tabs new\nmatch ok\n----\n";
        let steps = parse_source(src, "test".into()).unwrap();
        assert_eq!(steps[0].match_mode, MatchMode::Ok);
        assert_eq!(steps[0].expected, "");
    }

    #[test]
    fn inline_label_comment() {
        let src = "cmd --json identify  # smoke test\n----\ncon\n";
        let steps = parse_source(src, "test".into()).unwrap();
        assert_eq!(steps[0].label, "smoke test");
    }

    #[test]
    fn shell_split_quoted() {
        assert_eq!(
            shell_split(r#"panes exec --tab 1 "echo hello world""#).unwrap(),
            vec!["panes", "exec", "--tab", "1", "echo hello world"]
        );
    }

    #[test]
    fn shell_split_backslash() {
        assert_eq!(
            shell_split(r"panes exec --tab 1 echo\ hello").unwrap(),
            vec!["panes", "exec", "--tab", "1", "echo hello"]
        );
    }
}
