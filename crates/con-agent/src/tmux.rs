use serde::{Deserialize, Serialize};

const PREFIX: &str = "__CON_TMUX__";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TmuxPaneInfo {
    pub session_name: String,
    pub window_id: String,
    pub window_name: String,
    pub pane_id: String,
    pub pane_index: String,
    pub pane_active: bool,
    pub pane_current_command: Option<String>,
    pub pane_current_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TmuxSnapshot {
    pub panes: Vec<TmuxPaneInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TmuxCapture {
    pub target: String,
    pub content: String,
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub fn build_tmux_list_command(nonce: &str) -> String {
    format!(
        r#"sh -lc '
printf "%s\n" "__CON_TMUX_BEGIN_{nonce}__"
tmux list-panes -a -F "__CON_TMUX__\t#{{session_name}}\t#{{window_id}}\t#{{window_name}}\t#{{pane_id}}\t#{{pane_index}}\t#{{pane_active}}\t#{{pane_current_command}}\t#{{pane_current_path}}" 2>/dev/null
printf "%s\n" "__CON_TMUX_END_{nonce}__"
'"#,
    )
}

pub fn parse_tmux_list_lines(lines: &[String], nonce: &str) -> Result<TmuxSnapshot, String> {
    let begin = format!("__CON_TMUX_BEGIN_{nonce}__");
    let end = format!("__CON_TMUX_END_{nonce}__");

    let end_idx = lines
        .iter()
        .rposition(|line| line.trim_end() == end)
        .ok_or_else(|| "tmux list end marker not found in pane output".to_string())?;
    let begin_idx = lines[..end_idx]
        .iter()
        .rposition(|line| line.trim_end() == begin)
        .ok_or_else(|| "tmux list begin marker not found in pane output".to_string())?;

    let mut panes = Vec::new();
    for line in &lines[begin_idx + 1..end_idx] {
        let trimmed = line.trim_end_matches('\r');
        let mut parts = trimmed.splitn(9, '\t');
        if parts.next() != Some(PREFIX) {
            continue;
        }
        let fields: Vec<_> = parts.collect();
        if fields.len() != 8 {
            continue;
        }
        panes.push(TmuxPaneInfo {
            session_name: fields[0].to_string(),
            window_id: fields[1].to_string(),
            window_name: fields[2].to_string(),
            pane_id: fields[3].to_string(),
            pane_index: fields[4].to_string(),
            pane_active: fields[5] == "1",
            pane_current_command: optional_field(fields[6]),
            pane_current_path: optional_field(fields[7]),
        });
    }

    if panes.is_empty() {
        return Err("tmux list markers were present but no pane rows were parsed".to_string());
    }

    Ok(TmuxSnapshot { panes })
}

pub fn build_tmux_capture_command(nonce: &str, target: Option<&str>, lines: usize) -> String {
    let target_flag = target
        .map(|value| format!("-t {} ", shell_quote(value)))
        .unwrap_or_default();
    format!(
        r#"sh -lc '
printf "%s\n" "__CON_TMUX_CAPTURE_BEGIN_{nonce}__"
tmux capture-pane -p -J {target_flag}-S -{lines} 2>/dev/null
printf "%s\n" "__CON_TMUX_CAPTURE_END_{nonce}__"
'"#,
    )
}

pub fn parse_tmux_capture_lines(
    lines: &[String],
    nonce: &str,
    target: Option<&str>,
) -> Result<TmuxCapture, String> {
    let begin = format!("__CON_TMUX_CAPTURE_BEGIN_{nonce}__");
    let end = format!("__CON_TMUX_CAPTURE_END_{nonce}__");

    let end_idx = lines
        .iter()
        .rposition(|line| line.trim_end() == end)
        .ok_or_else(|| "tmux capture end marker not found in pane output".to_string())?;
    let begin_idx = lines[..end_idx]
        .iter()
        .rposition(|line| line.trim_end() == begin)
        .ok_or_else(|| "tmux capture begin marker not found in pane output".to_string())?;

    let content = lines[begin_idx + 1..end_idx].join("\n");
    Ok(TmuxCapture {
        target: target.unwrap_or("current").to_string(),
        content,
    })
}

pub fn build_tmux_send_keys_command(
    target: &str,
    literal_text: Option<&str>,
    key_names: &[String],
    append_enter: bool,
) -> Result<String, String> {
    if literal_text.is_none() && key_names.is_empty() {
        return Err("tmux_send_keys requires literal_text or key_names".to_string());
    }

    let mut commands = Vec::new();
    if let Some(text) = literal_text {
        commands.push(format!(
            "tmux send-keys -t {} -l -- {}",
            shell_quote(target),
            shell_quote(text)
        ));
    }
    if !key_names.is_empty() {
        let joined = key_names
            .iter()
            .map(|key| shell_quote(key))
            .collect::<Vec<_>>()
            .join(" ");
        commands.push(format!(
            "tmux send-keys -t {} {}",
            shell_quote(target),
            joined
        ));
    }
    if append_enter {
        commands.push(format!("tmux send-keys -t {} Enter", shell_quote(target)));
    }

    Ok(format!("sh -lc {}", shell_quote(&commands.join(" && "))))
}

fn optional_field(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_tmux_capture_command, build_tmux_list_command, build_tmux_send_keys_command,
        parse_tmux_capture_lines, parse_tmux_list_lines,
    };

    #[test]
    fn list_parser_extracts_tmux_panes() {
        let lines = vec![
            "__CON_TMUX_BEGIN_x__".to_string(),
            "__CON_TMUX__\twork\t@1\tshell\t%3\t0\t1\tzsh\t/home/w".to_string(),
            "__CON_TMUX__\twork\t@2\tcodex\t%7\t1\t0\tcodex\t/home/w/repo".to_string(),
            "__CON_TMUX_END_x__".to_string(),
        ];
        let snapshot = parse_tmux_list_lines(&lines, "x").expect("parse");
        assert_eq!(snapshot.panes.len(), 2);
        assert_eq!(snapshot.panes[1].pane_id, "%7");
        assert_eq!(
            snapshot.panes[1].pane_current_command.as_deref(),
            Some("codex")
        );
    }

    #[test]
    fn capture_parser_extracts_content() {
        let lines = vec![
            "prompt".to_string(),
            "__CON_TMUX_CAPTURE_BEGIN_x__".to_string(),
            "hello".to_string(),
            "world".to_string(),
            "__CON_TMUX_CAPTURE_END_x__".to_string(),
        ];
        let capture = parse_tmux_capture_lines(&lines, "x", Some("%7")).expect("capture");
        assert_eq!(capture.target, "%7");
        assert_eq!(capture.content, "hello\nworld");
    }

    #[test]
    fn builders_include_markers() {
        assert!(build_tmux_list_command("n").contains("__CON_TMUX_BEGIN_n__"));
        assert!(
            build_tmux_capture_command("n", Some("%3"), 80)
                .contains("__CON_TMUX_CAPTURE_BEGIN_n__")
        );
        let cmd = build_tmux_send_keys_command("%3", Some("ls"), &[], true).expect("cmd");
        assert!(cmd.contains("tmux send-keys"));
        assert!(cmd.contains("Enter"));
    }
}
