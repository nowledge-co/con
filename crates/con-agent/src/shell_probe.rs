use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

const PREFIX: &str = "__CON_SHELL_PROBE__";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ShellProbeTmuxContext {
    pub session_name: Option<String>,
    pub window_id: Option<String>,
    pub window_name: Option<String>,
    pub pane_id: Option<String>,
    pub pane_current_command: Option<String>,
    pub pane_current_path: Option<String>,
    pub client_tty: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ShellProbeResult {
    pub host: Option<String>,
    pub pwd: Option<String>,
    pub term: Option<String>,
    pub term_program: Option<String>,
    pub ssh_connection: Option<String>,
    pub ssh_tty: Option<String>,
    pub tmux_env: Option<String>,
    pub nvim_listen_address: Option<String>,
    pub tmux: Option<ShellProbeTmuxContext>,
    pub facts: BTreeMap<String, String>,
}

pub fn build_shell_probe_command(nonce: &str) -> String {
    format!(
        r##"sh -lc '
con_probe_emit() {{ printf "__CON_SHELL_PROBE__\t%s\t%s\n" "$1" "$2"; }}
printf "%s\n" "__CON_SHELL_PROBE_BEGIN_{nonce}__"
con_probe_emit host "$(hostname 2>/dev/null || uname -n 2>/dev/null || printf "")"
con_probe_emit pwd "$(pwd 2>/dev/null || printf "")"
con_probe_emit term "${{TERM-}}"
con_probe_emit term_program "${{TERM_PROGRAM-}}"
con_probe_emit ssh_connection "${{SSH_CONNECTION-}}"
con_probe_emit ssh_tty "${{SSH_TTY-}}"
con_probe_emit tmux_env "${{TMUX-}}"
con_probe_emit nvim_listen_address "${{NVIM_LISTEN_ADDRESS-}}"
if [ -n "${{TMUX-}}" ] && command -v tmux >/dev/null 2>&1; then
  con_probe_emit tmux_available "1"
  con_probe_emit tmux_session_name "$(tmux display-message -p "#{{session_name}}" 2>/dev/null || printf "")"
  con_probe_emit tmux_window_id "$(tmux display-message -p "#{{window_id}}" 2>/dev/null || printf "")"
  con_probe_emit tmux_window_name "$(tmux display-message -p "#{{window_name}}" 2>/dev/null || printf "")"
  con_probe_emit tmux_pane_id "$(tmux display-message -p "#{{pane_id}}" 2>/dev/null || printf "")"
  con_probe_emit tmux_pane_current_command "$(tmux display-message -p "#{{pane_current_command}}" 2>/dev/null || printf "")"
  con_probe_emit tmux_pane_current_path "$(tmux display-message -p "#{{pane_current_path}}" 2>/dev/null || printf "")"
  con_probe_emit tmux_client_tty "$(tmux display-message -p "#{{client_tty}}" 2>/dev/null || printf "")"
else
  con_probe_emit tmux_available "0"
fi
printf "%s\n" "__CON_SHELL_PROBE_END_{nonce}__"
'"##,
    )
}

pub fn parse_shell_probe_lines(lines: &[String], nonce: &str) -> Result<ShellProbeResult, String> {
    let begin = format!("__CON_SHELL_PROBE_BEGIN_{nonce}__");
    let end = format!("__CON_SHELL_PROBE_END_{nonce}__");

    let end_idx = lines
        .iter()
        .rposition(|line| line.trim_end() == end)
        .ok_or_else(|| "shell probe end marker not found in pane output".to_string())?;
    let begin_idx = lines[..end_idx]
        .iter()
        .rposition(|line| line.trim_end() == begin)
        .ok_or_else(|| "shell probe begin marker not found in pane output".to_string())?;

    let mut facts = BTreeMap::new();
    for line in &lines[begin_idx + 1..end_idx] {
        let trimmed = line.trim_end_matches('\r');
        let mut parts = trimmed.splitn(3, '\t');
        let prefix = parts.next();
        let key = parts.next();
        let value = parts.next();
        if prefix != Some(PREFIX) {
            continue;
        }
        if let (Some(key), Some(value)) = (key, value) {
            facts.insert(key.to_string(), value.to_string());
        }
    }

    if facts.is_empty() {
        return Err("shell probe markers were present but no probe facts were parsed".to_string());
    }

    let tmux = if facts
        .get("tmux_available")
        .is_some_and(|value| value == "1")
    {
        Some(ShellProbeTmuxContext {
            session_name: optional_fact(&facts, "tmux_session_name"),
            window_id: optional_fact(&facts, "tmux_window_id"),
            window_name: optional_fact(&facts, "tmux_window_name"),
            pane_id: optional_fact(&facts, "tmux_pane_id"),
            pane_current_command: optional_fact(&facts, "tmux_pane_current_command"),
            pane_current_path: optional_fact(&facts, "tmux_pane_current_path"),
            client_tty: optional_fact(&facts, "tmux_client_tty"),
        })
    } else {
        None
    };

    Ok(ShellProbeResult {
        host: optional_fact(&facts, "host"),
        pwd: optional_fact(&facts, "pwd"),
        term: optional_fact(&facts, "term"),
        term_program: optional_fact(&facts, "term_program"),
        ssh_connection: optional_fact(&facts, "ssh_connection"),
        ssh_tty: optional_fact(&facts, "ssh_tty"),
        tmux_env: optional_fact(&facts, "tmux_env"),
        nvim_listen_address: optional_fact(&facts, "nvim_listen_address"),
        tmux,
        facts,
    })
}

fn optional_fact(facts: &BTreeMap<String, String>, key: &str) -> Option<String> {
    facts.get(key).and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{build_shell_probe_command, parse_shell_probe_lines};

    #[test]
    fn build_command_uses_nonce_markers() {
        let cmd = build_shell_probe_command("abc123");
        assert!(cmd.contains("__CON_SHELL_PROBE_BEGIN_abc123__"));
        assert!(cmd.contains("__CON_SHELL_PROBE_END_abc123__"));
        assert!(cmd.contains("tmux display-message -p"));
    }

    #[test]
    fn parse_probe_block_extracts_tmux_context() {
        let lines = vec![
            "prompt$ sh -lc '...'".to_string(),
            "__CON_SHELL_PROBE_BEGIN_42__".to_string(),
            "__CON_SHELL_PROBE__\thost\thaswell".to_string(),
            "__CON_SHELL_PROBE__\tpwd\t/home/weyl/project".to_string(),
            "__CON_SHELL_PROBE__\tssh_connection\t1.2.3.4 1111 5.6.7.8 22".to_string(),
            "__CON_SHELL_PROBE__\ttmux_env\t/tmp/tmux-1000/default,123,0".to_string(),
            "__CON_SHELL_PROBE__\ttmux_available\t1".to_string(),
            "__CON_SHELL_PROBE__\ttmux_session_name\twork".to_string(),
            "__CON_SHELL_PROBE__\ttmux_window_id\t@3".to_string(),
            "__CON_SHELL_PROBE__\ttmux_pane_id\t%17".to_string(),
            "__CON_SHELL_PROBE__\ttmux_pane_current_command\tnvim".to_string(),
            "__CON_SHELL_PROBE_END_42__".to_string(),
        ];

        let result = parse_shell_probe_lines(&lines, "42").expect("probe parses");
        assert_eq!(result.host.as_deref(), Some("haswell"));
        assert_eq!(result.pwd.as_deref(), Some("/home/weyl/project"));
        assert_eq!(
            result.ssh_connection.as_deref(),
            Some("1.2.3.4 1111 5.6.7.8 22")
        );
        let tmux = result.tmux.expect("tmux context present");
        assert_eq!(tmux.session_name.as_deref(), Some("work"));
        assert_eq!(tmux.window_id.as_deref(), Some("@3"));
        assert_eq!(tmux.pane_id.as_deref(), Some("%17"));
        assert_eq!(tmux.pane_current_command.as_deref(), Some("nvim"));
    }

    #[test]
    fn parse_probe_requires_markers() {
        let lines = vec!["no markers".to_string()];
        let err = parse_shell_probe_lines(&lines, "x").expect_err("missing markers");
        assert!(err.contains("marker"));
    }
}
