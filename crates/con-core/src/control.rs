use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::runtime::Runtime;
use tokio::sync::oneshot;

use con_agent::{PaneCreateLocation, TmuxExecLocation};

pub const DEFAULT_SOCKET_PATH: &str = "/tmp/con.sock";
pub const JSON_RPC_VERSION: &str = "2.0";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PaneTarget {
    pub pane_index: Option<usize>,
    pub pane_id: Option<usize>,
}

impl PaneTarget {
    pub const fn new(pane_index: Option<usize>, pane_id: Option<usize>) -> Self {
        Self {
            pane_index,
            pane_id,
        }
    }

    pub fn describe(self) -> String {
        match (self.pane_index, self.pane_id) {
            (Some(index), Some(id)) => format!("pane {index} (id {id})"),
            (Some(index), None) => format!("pane {index}"),
            (None, Some(id)) => format!("pane id {id}"),
            (None, None) => "focused pane".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ControlMethodInfo {
    pub method: &'static str,
    pub description: &'static str,
}

const CONTROL_METHODS: &[(&str, &str)] = &[
    (
        "system.identify",
        "Return app identity, active tab, socket path, and method inventory.",
    ),
    (
        "system.capabilities",
        "List supported control-plane methods.",
    ),
    (
        "tabs.list",
        "List tabs and their conversation/session metadata.",
    ),
    ("tabs.new", "Create a new active tab."),
    (
        "tabs.close",
        "Close a tab by index or close the active tab.",
    ),
    (
        "panes.list",
        "List panes for a tab with control/runtime metadata.",
    ),
    ("panes.read", "Read recent content from a pane."),
    (
        "panes.exec",
        "Execute a visible shell command in a pane when allowed.",
    ),
    ("panes.send_keys", "Send raw keys to a pane."),
    ("panes.create", "Create a new pane split in a tab."),
    (
        "panes.wait",
        "Wait for a pane to become idle or match a pattern.",
    ),
    (
        "panes.probe_shell",
        "Run a read-only shell probe against a proven shell prompt.",
    ),
    ("tmux.inspect", "Inspect tmux control state for a pane."),
    (
        "tmux.list",
        "List tmux panes/windows through a native tmux anchor.",
    ),
    ("tmux.capture", "Capture content from a tmux pane target."),
    ("tmux.send_keys", "Send keys to a tmux pane target."),
    ("tmux.run", "Launch a command via tmux."),
    (
        "agent.ask",
        "Send a prompt to a tab's built-in agent session and wait for the response.",
    ),
    (
        "agent.new_conversation",
        "Start a fresh built-in agent conversation for a tab.",
    ),
];

pub fn control_methods() -> Vec<ControlMethodInfo> {
    CONTROL_METHODS
        .iter()
        .map(|(method, description)| ControlMethodInfo {
            method,
            description,
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct SystemIdentifyResult {
    pub app: String,
    pub version: String,
    pub socket_path: String,
    pub active_tab_index: usize,
    pub tab_count: usize,
    pub methods: Vec<ControlMethodInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TabInfo {
    pub index: usize,
    pub title: String,
    pub is_active: bool,
    pub pane_count: usize,
    pub focused_pane_id: usize,
    pub needs_attention: bool,
    pub conversation_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentAskResult {
    pub tab_index: usize,
    pub conversation_id: String,
    pub prompt: String,
    pub message: con_agent::Message,
}

#[derive(Debug, Clone)]
pub enum ControlCommand {
    SystemIdentify,
    SystemCapabilities,
    TabsList,
    TabsNew,
    TabsClose {
        tab_index: Option<usize>,
    },
    PanesList {
        tab_index: Option<usize>,
    },
    PanesRead {
        tab_index: Option<usize>,
        target: PaneTarget,
        lines: usize,
    },
    PanesExec {
        tab_index: Option<usize>,
        target: PaneTarget,
        command: String,
    },
    PanesSendKeys {
        tab_index: Option<usize>,
        target: PaneTarget,
        keys: String,
    },
    PanesCreate {
        tab_index: Option<usize>,
        location: PaneCreateLocation,
        command: Option<String>,
    },
    PanesWait {
        tab_index: Option<usize>,
        target: PaneTarget,
        timeout_secs: Option<u64>,
        pattern: Option<String>,
    },
    PanesProbeShell {
        tab_index: Option<usize>,
        target: PaneTarget,
    },
    TmuxInspect {
        tab_index: Option<usize>,
        target: PaneTarget,
    },
    TmuxList {
        tab_index: Option<usize>,
        target: PaneTarget,
    },
    TmuxCapture {
        tab_index: Option<usize>,
        pane: PaneTarget,
        target: Option<String>,
        lines: usize,
    },
    TmuxSendKeys {
        tab_index: Option<usize>,
        pane: PaneTarget,
        target: String,
        literal_text: Option<String>,
        key_names: Vec<String>,
        append_enter: bool,
    },
    TmuxRun {
        tab_index: Option<usize>,
        pane: PaneTarget,
        target: Option<String>,
        location: TmuxExecLocation,
        command: String,
        window_name: Option<String>,
        cwd: Option<String>,
        detached: bool,
    },
    AgentAsk {
        tab_index: Option<usize>,
        prompt: String,
        auto_approve_tools: bool,
        timeout_secs: Option<u64>,
    },
    AgentNewConversation {
        tab_index: Option<usize>,
    },
}

impl ControlCommand {
    pub fn method_name(&self) -> &'static str {
        match self {
            Self::SystemIdentify => "system.identify",
            Self::SystemCapabilities => "system.capabilities",
            Self::TabsList => "tabs.list",
            Self::TabsNew => "tabs.new",
            Self::TabsClose { .. } => "tabs.close",
            Self::PanesList { .. } => "panes.list",
            Self::PanesRead { .. } => "panes.read",
            Self::PanesExec { .. } => "panes.exec",
            Self::PanesSendKeys { .. } => "panes.send_keys",
            Self::PanesCreate { .. } => "panes.create",
            Self::PanesWait { .. } => "panes.wait",
            Self::PanesProbeShell { .. } => "panes.probe_shell",
            Self::TmuxInspect { .. } => "tmux.inspect",
            Self::TmuxList { .. } => "tmux.list",
            Self::TmuxCapture { .. } => "tmux.capture",
            Self::TmuxSendKeys { .. } => "tmux.send_keys",
            Self::TmuxRun { .. } => "tmux.run",
            Self::AgentAsk { .. } => "agent.ask",
            Self::AgentNewConversation { .. } => "agent.new_conversation",
        }
    }

    pub fn params_json(&self) -> Value {
        match self {
            Self::SystemIdentify | Self::SystemCapabilities | Self::TabsList | Self::TabsNew => {
                json!({})
            }
            Self::TabsClose { tab_index } => json!({ "tab_index": tab_index }),
            Self::PanesList { tab_index } => json!({ "tab_index": tab_index }),
            Self::PanesRead {
                tab_index,
                target,
                lines,
            } => json!({
                "tab_index": tab_index,
                "pane_index": target.pane_index,
                "pane_id": target.pane_id,
                "lines": lines,
            }),
            Self::PanesExec {
                tab_index,
                target,
                command,
            } => json!({
                "tab_index": tab_index,
                "pane_index": target.pane_index,
                "pane_id": target.pane_id,
                "command": command,
            }),
            Self::PanesSendKeys {
                tab_index,
                target,
                keys,
            } => json!({
                "tab_index": tab_index,
                "pane_index": target.pane_index,
                "pane_id": target.pane_id,
                "keys": keys,
            }),
            Self::PanesCreate {
                tab_index,
                location,
                command,
            } => json!({
                "tab_index": tab_index,
                "location": location,
                "command": command,
            }),
            Self::PanesWait {
                tab_index,
                target,
                timeout_secs,
                pattern,
            } => json!({
                "tab_index": tab_index,
                "pane_index": target.pane_index,
                "pane_id": target.pane_id,
                "timeout_secs": timeout_secs,
                "pattern": pattern,
            }),
            Self::PanesProbeShell { tab_index, target }
            | Self::TmuxInspect { tab_index, target }
            | Self::TmuxList { tab_index, target } => json!({
                "tab_index": tab_index,
                "pane_index": target.pane_index,
                "pane_id": target.pane_id,
            }),
            Self::TmuxCapture {
                tab_index,
                pane,
                target,
                lines,
            } => json!({
                "tab_index": tab_index,
                "pane_index": pane.pane_index,
                "pane_id": pane.pane_id,
                "target": target,
                "lines": lines,
            }),
            Self::TmuxSendKeys {
                tab_index,
                pane,
                target,
                literal_text,
                key_names,
                append_enter,
            } => json!({
                "tab_index": tab_index,
                "pane_index": pane.pane_index,
                "pane_id": pane.pane_id,
                "target": target,
                "literal_text": literal_text,
                "key_names": key_names,
                "append_enter": append_enter,
            }),
            Self::TmuxRun {
                tab_index,
                pane,
                target,
                location,
                command,
                window_name,
                cwd,
                detached,
            } => json!({
                "tab_index": tab_index,
                "pane_index": pane.pane_index,
                "pane_id": pane.pane_id,
                "target": target,
                "location": location,
                "command": command,
                "window_name": window_name,
                "cwd": cwd,
                "detached": detached,
            }),
            Self::AgentAsk {
                tab_index,
                prompt,
                auto_approve_tools,
                timeout_secs,
            } => json!({
                "tab_index": tab_index,
                "prompt": prompt,
                "auto_approve_tools": auto_approve_tools,
                "timeout_secs": timeout_secs,
            }),
            Self::AgentNewConversation { tab_index } => json!({ "tab_index": tab_index }),
        }
    }

    pub fn from_rpc(method: &str, params: Value) -> Result<Self, ControlError> {
        match method {
            "system.identify" => Ok(Self::SystemIdentify),
            "system.capabilities" => Ok(Self::SystemCapabilities),
            "tabs.list" => Ok(Self::TabsList),
            "tabs.new" => Ok(Self::TabsNew),
            "tabs.close" => {
                let params: TabScopedParams = decode_params(params)?;
                Ok(Self::TabsClose {
                    tab_index: params.tab_index,
                })
            }
            "panes.list" => {
                let params: TabScopedParams = decode_params(params)?;
                Ok(Self::PanesList {
                    tab_index: params.tab_index,
                })
            }
            "panes.read" => {
                let params: PaneReadParams = decode_params(params)?;
                Ok(Self::PanesRead {
                    tab_index: params.tab_index,
                    target: params.target(),
                    lines: params.lines,
                })
            }
            "panes.exec" => {
                let params: PaneExecParams = decode_params(params)?;
                Ok(Self::PanesExec {
                    tab_index: params.tab_index,
                    target: params.target(),
                    command: params.command,
                })
            }
            "panes.send_keys" => {
                let params: PaneSendKeysParams = decode_params(params)?;
                Ok(Self::PanesSendKeys {
                    tab_index: params.tab_index,
                    target: params.target(),
                    keys: params.keys,
                })
            }
            "panes.create" => {
                let params: PaneCreateParams = decode_params(params)?;
                Ok(Self::PanesCreate {
                    tab_index: params.tab_index,
                    location: params.location,
                    command: params.command,
                })
            }
            "panes.wait" => {
                let params: PaneWaitParams = decode_params(params)?;
                Ok(Self::PanesWait {
                    tab_index: params.tab_index,
                    target: params.target(),
                    timeout_secs: params.timeout_secs,
                    pattern: params.pattern,
                })
            }
            "panes.probe_shell" => {
                let params: PaneTargetParams = decode_params(params)?;
                Ok(Self::PanesProbeShell {
                    tab_index: params.tab_index,
                    target: params.target(),
                })
            }
            "tmux.inspect" => {
                let params: PaneTargetParams = decode_params(params)?;
                Ok(Self::TmuxInspect {
                    tab_index: params.tab_index,
                    target: params.target(),
                })
            }
            "tmux.list" => {
                let params: PaneTargetParams = decode_params(params)?;
                Ok(Self::TmuxList {
                    tab_index: params.tab_index,
                    target: params.target(),
                })
            }
            "tmux.capture" => {
                let params: TmuxCaptureParams = decode_params(params)?;
                Ok(Self::TmuxCapture {
                    tab_index: params.tab_index,
                    pane: params.target(),
                    target: params.target,
                    lines: params.lines,
                })
            }
            "tmux.send_keys" => {
                let params: TmuxSendKeysParams = decode_params(params)?;
                Ok(Self::TmuxSendKeys {
                    tab_index: params.tab_index,
                    pane: params.target(),
                    target: params.target,
                    literal_text: params.literal_text,
                    key_names: params.key_names,
                    append_enter: params.append_enter,
                })
            }
            "tmux.run" => {
                let params: TmuxRunParams = decode_params(params)?;
                Ok(Self::TmuxRun {
                    tab_index: params.tab_index,
                    pane: params.target(),
                    target: params.target,
                    location: params.location,
                    command: params.command,
                    window_name: params.window_name,
                    cwd: params.cwd,
                    detached: params.detached,
                })
            }
            "agent.ask" => {
                let params: AgentAskParams = decode_params(params)?;
                Ok(Self::AgentAsk {
                    tab_index: params.tab_index,
                    prompt: params.prompt,
                    auto_approve_tools: params.auto_approve_tools,
                    timeout_secs: params.timeout_secs,
                })
            }
            "agent.new_conversation" => {
                let params: TabScopedParams = decode_params(params)?;
                Ok(Self::AgentNewConversation {
                    tab_index: params.tab_index,
                })
            }
            _ => Err(ControlError::method_not_found(method)),
        }
    }
}

#[derive(Debug)]
pub struct ControlRequestEnvelope {
    pub command: ControlCommand,
    pub response_tx: oneshot::Sender<ControlResult>,
}

pub type ControlResult = Result<Value, ControlError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone)]
pub struct ControlError {
    pub code: i32,
    pub message: String,
}

impl ControlError {
    pub fn parse(message: impl Into<String>) -> Self {
        Self {
            code: -32700,
            message: message.into(),
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: message.into(),
        }
    }

    pub fn method_not_found(method: impl AsRef<str>) -> Self {
        Self {
            code: -32601,
            message: format!("Unknown method: {}", method.as_ref()),
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: -32000,
            message: message.into(),
        }
    }

    fn into_json_rpc(self) -> JsonRpcError {
        JsonRpcError {
            code: self.code,
            message: self.message,
        }
    }
}

pub struct ControlSocketHandle {
    path: PathBuf,
}

impl ControlSocketHandle {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ControlSocketHandle {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn control_socket_path() -> PathBuf {
    match env::var("CON_SOCKET_PATH") {
        Ok(value) if !value.trim().is_empty() => PathBuf::from(value),
        _ => PathBuf::from(DEFAULT_SOCKET_PATH),
    }
}

pub fn spawn_control_socket_server(
    runtime: Arc<Runtime>,
    request_tx: Sender<ControlRequestEnvelope>,
) -> Result<ControlSocketHandle> {
    let path = control_socket_path();
    prepare_socket_path(&path)?;

    let listener = {
        let _guard = runtime.enter();
        UnixListener::bind(&path)
            .with_context(|| format!("failed to bind control socket at {}", path.display()))?
    };
    set_socket_permissions(&path)?;

    runtime.spawn(async move {
        if let Err(err) = accept_loop(listener, request_tx).await {
            log::error!("[control] socket accept loop failed: {err}");
        }
    });

    Ok(ControlSocketHandle { path })
}

async fn accept_loop(
    listener: UnixListener,
    request_tx: Sender<ControlRequestEnvelope>,
) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let request_tx = request_tx.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_client(stream, request_tx).await {
                log::warn!("[control] client connection failed: {err}");
            }
        });
    }
}

async fn handle_client(
    stream: UnixStream,
    request_tx: Sender<ControlRequestEnvelope>,
) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<JsonRpcRequest>(&line) {
            Ok(request) => handle_json_rpc_request(request, &request_tx).await,
            Err(err) => JsonRpcResponse {
                jsonrpc: JSON_RPC_VERSION.to_string(),
                id: Value::Null,
                result: None,
                error: Some(ControlError::parse(err.to_string()).into_json_rpc()),
            },
        };

        let encoded = serde_json::to_string(&response)?;
        write_half.write_all(encoded.as_bytes()).await?;
        write_half.write_all(b"\n").await?;
    }

    Ok(())
}

async fn handle_json_rpc_request(
    request: JsonRpcRequest,
    request_tx: &Sender<ControlRequestEnvelope>,
) -> JsonRpcResponse {
    let id = request.id.clone().unwrap_or(Value::Null);

    if request.jsonrpc != JSON_RPC_VERSION {
        return JsonRpcResponse {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(
                ControlError::invalid_request("Only JSON-RPC 2.0 requests are supported")
                    .into_json_rpc(),
            ),
        };
    }

    let command = match ControlCommand::from_rpc(&request.method, request.params) {
        Ok(command) => command,
        Err(err) => {
            return JsonRpcResponse {
                jsonrpc: JSON_RPC_VERSION.to_string(),
                id,
                result: None,
                error: Some(err.into_json_rpc()),
            };
        }
    };

    let (response_tx, response_rx) = oneshot::channel();
    if request_tx
        .send(ControlRequestEnvelope {
            command,
            response_tx,
        })
        .is_err()
    {
        return JsonRpcResponse {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(
                ControlError::internal("con control bridge is unavailable").into_json_rpc(),
            ),
        };
    }

    match response_rx.await {
        Ok(Ok(result)) => JsonRpcResponse {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id,
            result: Some(result),
            error: None,
        },
        Ok(Err(err)) => JsonRpcResponse {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(err.into_json_rpc()),
        },
        Err(_) => JsonRpcResponse {
            jsonrpc: JSON_RPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(ControlError::internal("con control request dropped").into_json_rpc()),
        },
    }
}

fn prepare_socket_path(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if path.exists() {
        match std::os::unix::net::UnixStream::connect(path) {
            Ok(_) => {
                anyhow::bail!(
                    "another con control socket is already listening at {}",
                    path.display()
                );
            }
            Err(_) => {
                std::fs::remove_file(path).with_context(|| {
                    format!("failed to remove stale control socket {}", path.display())
                })?;
            }
        }
    }

    Ok(())
}

#[cfg(unix)]
fn set_socket_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let permissions = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, permissions).with_context(|| {
        format!(
            "failed to set control socket permissions on {}",
            path.display()
        )
    })
}

#[cfg(not(unix))]
fn set_socket_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn default_read_lines() -> usize {
    80
}

fn default_capture_lines() -> usize {
    120
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct TabScopedParams {
    tab_index: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct PaneTargetParams {
    tab_index: Option<usize>,
    pane_index: Option<usize>,
    pane_id: Option<usize>,
}

impl PaneTargetParams {
    fn target(&self) -> PaneTarget {
        PaneTarget::new(self.pane_index, self.pane_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct PaneReadParams {
    tab_index: Option<usize>,
    pane_index: Option<usize>,
    pane_id: Option<usize>,
    #[serde(default = "default_read_lines")]
    lines: usize,
}

impl Default for PaneReadParams {
    fn default() -> Self {
        Self {
            tab_index: None,
            pane_index: None,
            pane_id: None,
            lines: default_read_lines(),
        }
    }
}

impl PaneReadParams {
    fn target(&self) -> PaneTarget {
        PaneTarget::new(self.pane_index, self.pane_id)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct PaneExecParams {
    tab_index: Option<usize>,
    pane_index: Option<usize>,
    pane_id: Option<usize>,
    command: String,
}

impl PaneExecParams {
    fn target(&self) -> PaneTarget {
        PaneTarget::new(self.pane_index, self.pane_id)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct PaneSendKeysParams {
    tab_index: Option<usize>,
    pane_index: Option<usize>,
    pane_id: Option<usize>,
    keys: String,
}

impl PaneSendKeysParams {
    fn target(&self) -> PaneTarget {
        PaneTarget::new(self.pane_index, self.pane_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct PaneCreateParams {
    tab_index: Option<usize>,
    location: PaneCreateLocation,
    command: Option<String>,
}

impl Default for PaneCreateParams {
    fn default() -> Self {
        Self {
            tab_index: None,
            location: PaneCreateLocation::Right,
            command: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct PaneWaitParams {
    tab_index: Option<usize>,
    pane_index: Option<usize>,
    pane_id: Option<usize>,
    timeout_secs: Option<u64>,
    pattern: Option<String>,
}

impl PaneWaitParams {
    fn target(&self) -> PaneTarget {
        PaneTarget::new(self.pane_index, self.pane_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct TmuxCaptureParams {
    tab_index: Option<usize>,
    pane_index: Option<usize>,
    pane_id: Option<usize>,
    target: Option<String>,
    #[serde(default = "default_capture_lines")]
    lines: usize,
}

impl Default for TmuxCaptureParams {
    fn default() -> Self {
        Self {
            tab_index: None,
            pane_index: None,
            pane_id: None,
            target: None,
            lines: default_capture_lines(),
        }
    }
}

impl TmuxCaptureParams {
    fn target(&self) -> PaneTarget {
        PaneTarget::new(self.pane_index, self.pane_id)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct TmuxSendKeysParams {
    tab_index: Option<usize>,
    pane_index: Option<usize>,
    pane_id: Option<usize>,
    target: String,
    literal_text: Option<String>,
    key_names: Vec<String>,
    append_enter: bool,
}

impl TmuxSendKeysParams {
    fn target(&self) -> PaneTarget {
        PaneTarget::new(self.pane_index, self.pane_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct TmuxRunParams {
    tab_index: Option<usize>,
    pane_index: Option<usize>,
    pane_id: Option<usize>,
    target: Option<String>,
    location: TmuxExecLocation,
    command: String,
    window_name: Option<String>,
    cwd: Option<String>,
    detached: bool,
}

impl Default for TmuxRunParams {
    fn default() -> Self {
        Self {
            tab_index: None,
            pane_index: None,
            pane_id: None,
            target: None,
            location: TmuxExecLocation::NewWindow,
            command: String::new(),
            window_name: None,
            cwd: None,
            detached: false,
        }
    }
}

impl TmuxRunParams {
    fn target(&self) -> PaneTarget {
        PaneTarget::new(self.pane_index, self.pane_id)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct AgentAskParams {
    tab_index: Option<usize>,
    prompt: String,
    auto_approve_tools: bool,
    timeout_secs: Option<u64>,
}

fn decode_params<T>(params: Value) -> Result<T, ControlError>
where
    T: for<'de> Deserialize<'de> + Default,
{
    let value = if params.is_null() { json!({}) } else { params };
    serde_json::from_value(value).map_err(|err| ControlError::invalid_params(err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_command_round_trips_from_rpc() {
        let command = ControlCommand::PanesExec {
            tab_index: Some(2),
            target: PaneTarget::new(None, Some(7)),
            command: "cargo test".to_string(),
        };

        let parsed = ControlCommand::from_rpc(command.method_name(), command.params_json())
            .expect("round-trip parse");

        match parsed {
            ControlCommand::PanesExec {
                tab_index,
                target,
                command,
            } => {
                assert_eq!(tab_index, Some(2));
                assert_eq!(target.pane_id, Some(7));
                assert_eq!(command, "cargo test");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn unknown_method_returns_json_rpc_error() {
        let err = ControlCommand::from_rpc("con.unknown", json!({})).expect_err("should fail");
        assert_eq!(err.code, -32601);
    }

    #[test]
    fn control_methods_are_non_empty() {
        assert!(!control_methods().is_empty());
    }
}
