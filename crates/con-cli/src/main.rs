use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use con_agent::{PaneCreateLocation, TmuxExecLocation};
use con_core::{
    ControlCommand, JSON_RPC_VERSION, JsonRpcRequest, JsonRpcResponse, PaneTarget,
    control_socket_path,
};
use serde_json::{Value, json};

#[derive(Parser)]
#[command(name = "con-cli", about = "CLI control surface for a running con app")]
struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    socket: Option<PathBuf>,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Identify,
    Capabilities,
    Tabs {
        #[command(subcommand)]
        command: TabsCommand,
    },
    Panes {
        #[command(subcommand)]
        command: PanesCommand,
    },
    Tmux {
        #[command(subcommand)]
        command: TmuxCommand,
    },
    Agent {
        #[command(subcommand)]
        command: AgentCommand,
    },
}

#[derive(Subcommand)]
enum TabsCommand {
    List,
}

#[derive(Subcommand)]
enum PanesCommand {
    List(TabArgs),
    Read(PaneReadArgs),
    Exec(PaneExecArgs),
    SendKeys(PaneSendKeysArgs),
    Create(PaneCreateArgs),
    Wait(PaneWaitArgs),
    ProbeShell(PaneTargetArgs),
}

#[derive(Subcommand)]
enum TmuxCommand {
    Inspect(PaneTargetArgs),
    List(PaneTargetArgs),
    Capture(TmuxCaptureArgs),
    SendKeys(TmuxSendKeysArgs),
    Run(TmuxRunArgs),
}

#[derive(Subcommand)]
enum AgentCommand {
    Ask(AgentAskArgs),
    NewConversation(TabArgs),
}

#[derive(Args, Clone, Default)]
struct TabArgs {
    #[arg(long, value_name = "INDEX")]
    tab: Option<usize>,
}

#[derive(Args, Clone, Default)]
struct PaneTargetArgs {
    #[command(flatten)]
    tab: TabArgs,
    #[arg(long, value_name = "INDEX")]
    pane_index: Option<usize>,
    #[arg(long, value_name = "ID")]
    pane_id: Option<usize>,
}

impl PaneTargetArgs {
    fn target(&self) -> PaneTarget {
        PaneTarget::new(self.pane_index, self.pane_id)
    }
}

#[derive(Args, Clone)]
struct PaneReadArgs {
    #[command(flatten)]
    target: PaneTargetArgs,
    #[arg(long, default_value_t = 80)]
    lines: usize,
}

#[derive(Args, Clone)]
struct PaneExecArgs {
    #[command(flatten)]
    target: PaneTargetArgs,
    #[arg(required = true, trailing_var_arg = true)]
    command: Vec<String>,
}

#[derive(Args, Clone)]
struct PaneSendKeysArgs {
    #[command(flatten)]
    target: PaneTargetArgs,
    #[arg(required = true)]
    keys: String,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum PaneCreateLocationArg {
    Right,
    Down,
}

impl From<PaneCreateLocationArg> for PaneCreateLocation {
    fn from(value: PaneCreateLocationArg) -> Self {
        match value {
            PaneCreateLocationArg::Right => PaneCreateLocation::Right,
            PaneCreateLocationArg::Down => PaneCreateLocation::Down,
        }
    }
}

#[derive(Args, Clone)]
struct PaneCreateArgs {
    #[command(flatten)]
    tab: TabArgs,
    #[arg(long, value_enum, default_value_t = PaneCreateLocationArg::Right)]
    location: PaneCreateLocationArg,
    #[arg(long)]
    command: Option<String>,
}

#[derive(Args, Clone)]
struct PaneWaitArgs {
    #[command(flatten)]
    target: PaneTargetArgs,
    #[arg(long)]
    timeout: Option<u64>,
    #[arg(long)]
    pattern: Option<String>,
}

#[derive(Args, Clone)]
struct TmuxCaptureArgs {
    #[command(flatten)]
    pane: PaneTargetArgs,
    #[arg(long)]
    target: Option<String>,
    #[arg(long, default_value_t = 120)]
    lines: usize,
}

#[derive(Args, Clone)]
struct TmuxSendKeysArgs {
    #[command(flatten)]
    pane: PaneTargetArgs,
    #[arg(long)]
    target: String,
    #[arg(long)]
    text: Option<String>,
    #[arg(long = "key")]
    key_names: Vec<String>,
    #[arg(long)]
    enter: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
enum TmuxRunLocationArg {
    NewWindow,
    SplitHorizontal,
    SplitVertical,
}

impl From<TmuxRunLocationArg> for TmuxExecLocation {
    fn from(value: TmuxRunLocationArg) -> Self {
        match value {
            TmuxRunLocationArg::NewWindow => TmuxExecLocation::NewWindow,
            TmuxRunLocationArg::SplitHorizontal => TmuxExecLocation::SplitHorizontal,
            TmuxRunLocationArg::SplitVertical => TmuxExecLocation::SplitVertical,
        }
    }
}

#[derive(Args, Clone)]
struct TmuxRunArgs {
    #[command(flatten)]
    pane: PaneTargetArgs,
    #[arg(long)]
    target: Option<String>,
    #[arg(long, value_enum, default_value_t = TmuxRunLocationArg::NewWindow)]
    location: TmuxRunLocationArg,
    #[arg(long)]
    window_name: Option<String>,
    #[arg(long)]
    cwd: Option<String>,
    #[arg(long)]
    detached: bool,
    #[arg(required = true, trailing_var_arg = true)]
    command: Vec<String>,
}

#[derive(Args, Clone)]
struct AgentAskArgs {
    #[command(flatten)]
    tab: TabArgs,
    #[arg(long)]
    auto_approve_tools: bool,
    #[arg(required = true, trailing_var_arg = true)]
    prompt: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let socket_path = cli.socket.clone().unwrap_or_else(control_socket_path);

    match cli.command {
        Command::Identify => {
            let result = send_command(&socket_path, ControlCommand::SystemIdentify)?;
            print_result(&result, cli.json, render_identify)?;
        }
        Command::Capabilities => {
            let result = send_command(&socket_path, ControlCommand::SystemCapabilities)?;
            print_result(&result, cli.json, render_capabilities)?;
        }
        Command::Tabs { command } => match command {
            TabsCommand::List => {
                let result = send_command(&socket_path, ControlCommand::TabsList)?;
                print_result(&result, cli.json, render_tabs_list)?;
            }
        },
        Command::Panes { command } => match command {
            PanesCommand::List(args) => {
                let result = send_command(
                    &socket_path,
                    ControlCommand::PanesList {
                        tab_index: args.tab,
                    },
                )?;
                print_result(&result, cli.json, render_panes_list)?;
            }
            PanesCommand::Read(args) => {
                let result = send_command(
                    &socket_path,
                    ControlCommand::PanesRead {
                        tab_index: args.target.tab.tab,
                        target: args.target.target(),
                        lines: args.lines,
                    },
                )?;
                print_result(&result, cli.json, render_content_only)?;
            }
            PanesCommand::Exec(args) => {
                let result = send_command(
                    &socket_path,
                    ControlCommand::PanesExec {
                        tab_index: args.target.tab.tab,
                        target: args.target.target(),
                        command: join_words(args.command),
                    },
                )?;
                print_result(&result, cli.json, render_exec_result)?;
            }
            PanesCommand::SendKeys(args) => {
                let result = send_command(
                    &socket_path,
                    ControlCommand::PanesSendKeys {
                        tab_index: args.target.tab.tab,
                        target: args.target.target(),
                        keys: args.keys,
                    },
                )?;
                print_result(&result, cli.json, render_status_only)?;
            }
            PanesCommand::Create(args) => {
                let result = send_command(
                    &socket_path,
                    ControlCommand::PanesCreate {
                        tab_index: args.tab.tab,
                        location: args.location.into(),
                        command: args.command,
                    },
                )?;
                print_result(&result, cli.json, render_create_result)?;
            }
            PanesCommand::Wait(args) => {
                let result = send_command(
                    &socket_path,
                    ControlCommand::PanesWait {
                        tab_index: args.target.tab.tab,
                        target: args.target.target(),
                        timeout_secs: args.timeout,
                        pattern: args.pattern,
                    },
                )?;
                print_result(&result, cli.json, render_wait_result)?;
            }
            PanesCommand::ProbeShell(args) => {
                let result = send_command(
                    &socket_path,
                    ControlCommand::PanesProbeShell {
                        tab_index: args.tab.tab,
                        target: args.target(),
                    },
                )?;
                print_result(&result, cli.json, render_pretty_json)?;
            }
        },
        Command::Tmux { command } => match command {
            TmuxCommand::Inspect(args) => {
                let result = send_command(
                    &socket_path,
                    ControlCommand::TmuxInspect {
                        tab_index: args.tab.tab,
                        target: args.target(),
                    },
                )?;
                print_result(&result, cli.json, render_pretty_json)?;
            }
            TmuxCommand::List(args) => {
                let result = send_command(
                    &socket_path,
                    ControlCommand::TmuxList {
                        tab_index: args.tab.tab,
                        target: args.target(),
                    },
                )?;
                print_result(&result, cli.json, render_tmux_list)?;
            }
            TmuxCommand::Capture(args) => {
                let result = send_command(
                    &socket_path,
                    ControlCommand::TmuxCapture {
                        tab_index: args.pane.tab.tab,
                        pane: args.pane.target(),
                        target: args.target,
                        lines: args.lines,
                    },
                )?;
                print_result(&result, cli.json, render_tmux_capture)?;
            }
            TmuxCommand::SendKeys(args) => {
                let result = send_command(
                    &socket_path,
                    ControlCommand::TmuxSendKeys {
                        tab_index: args.pane.tab.tab,
                        pane: args.pane.target(),
                        target: args.target,
                        literal_text: args.text,
                        key_names: args.key_names,
                        append_enter: args.enter,
                    },
                )?;
                print_result(&result, cli.json, render_status_only)?;
            }
            TmuxCommand::Run(args) => {
                let result = send_command(
                    &socket_path,
                    ControlCommand::TmuxRun {
                        tab_index: args.pane.tab.tab,
                        pane: args.pane.target(),
                        target: args.target,
                        location: args.location.into(),
                        command: join_words(args.command),
                        window_name: args.window_name,
                        cwd: args.cwd,
                        detached: args.detached,
                    },
                )?;
                print_result(&result, cli.json, render_pretty_json)?;
            }
        },
        Command::Agent { command } => match command {
            AgentCommand::Ask(args) => {
                let result = send_command(
                    &socket_path,
                    ControlCommand::AgentAsk {
                        tab_index: args.tab.tab,
                        prompt: join_words(args.prompt),
                        auto_approve_tools: args.auto_approve_tools,
                    },
                )?;
                print_result(&result, cli.json, render_agent_ask)?;
            }
            AgentCommand::NewConversation(args) => {
                let result = send_command(
                    &socket_path,
                    ControlCommand::AgentNewConversation {
                        tab_index: args.tab,
                    },
                )?;
                print_result(&result, cli.json, render_new_conversation)?;
            }
        },
    }

    Ok(())
}

fn join_words(words: Vec<String>) -> String {
    words.join(" ")
}

fn send_command(socket_path: &Path, command: ControlCommand) -> Result<Value> {
    let mut stream = UnixStream::connect(socket_path).with_context(|| {
        format!(
            "failed to connect to con at {}. Launch con first or pass --socket/CON_SOCKET_PATH.",
            socket_path.display()
        )
    })?;
    let request = JsonRpcRequest {
        jsonrpc: JSON_RPC_VERSION.to_string(),
        id: Some(json!(request_id())),
        method: command.method_name().to_string(),
        params: command.params_json(),
    };

    let encoded = serde_json::to_string(&request)?;
    stream.write_all(encoded.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    let read = reader.read_line(&mut line)?;
    if read == 0 {
        bail!("con closed the control connection without returning a response");
    }

    let response: JsonRpcResponse =
        serde_json::from_str(&line).context("failed to decode con control response")?;
    if let Some(error) = response.error {
        bail!("{} (code {})", error.message, error.code);
    }

    response
        .result
        .ok_or_else(|| anyhow::anyhow!("con returned an empty control response"))
}

fn request_id() -> String {
    format!(
        "concli-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    )
}

fn print_result(
    value: &Value,
    json_mode: bool,
    plain_renderer: fn(&Value) -> Result<()>,
) -> Result<()> {
    if json_mode {
        println!("{}", serde_json::to_string(value)?);
        return Ok(());
    }
    plain_renderer(value)
}

fn render_identify(value: &Value) -> Result<()> {
    println!(
        "con {}",
        string_field(value, "version").unwrap_or("unknown-version")
    );
    println!(
        "socket: {}",
        string_field(value, "socket_path").unwrap_or("<unknown>")
    );
    println!(
        "active tab: {} of {}",
        value["active_tab_index"].as_u64().unwrap_or(0),
        value["tab_count"].as_u64().unwrap_or(0)
    );
    println!();
    render_capabilities(value)
}

fn render_capabilities(value: &Value) -> Result<()> {
    let methods = value
        .get("methods")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("missing methods array"))?;
    for method in methods {
        let name = method["method"].as_str().unwrap_or("<unknown>");
        let description = method["description"].as_str().unwrap_or("");
        println!("{name:<24} {description}");
    }
    Ok(())
}

fn render_tabs_list(value: &Value) -> Result<()> {
    let tabs = value
        .get("tabs")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("missing tabs array"))?;
    for tab in tabs {
        let marker = if tab["is_active"].as_bool().unwrap_or(false) {
            "*"
        } else {
            " "
        };
        let index = tab["index"].as_u64().unwrap_or(0);
        let pane_count = tab["pane_count"].as_u64().unwrap_or(0);
        let conversation_id = tab["conversation_id"].as_str().unwrap_or("");
        let title = tab["title"].as_str().unwrap_or("Untitled");
        println!(
            "{marker} tab {index:<3} panes={pane_count:<2} conversation={conversation_id}  {title}"
        );
    }
    Ok(())
}

fn render_panes_list(value: &Value) -> Result<()> {
    let panes = value
        .get("panes")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("missing panes array"))?;
    for pane in panes {
        let marker = if pane["is_focused"].as_bool().unwrap_or(false) {
            "*"
        } else {
            " "
        };
        let index = pane["index"].as_u64().unwrap_or(0);
        let pane_id = pane["pane_id"].as_u64().unwrap_or(0);
        let mode = pane["mode"].as_str().unwrap_or("unknown");
        let target = pane
            .get("visible_target")
            .and_then(|v| v.get("kind"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let cwd = pane["cwd"].as_str().unwrap_or("");
        let title = pane["title"].as_str().unwrap_or("Pane");
        let caps = pane
            .get("control_capabilities")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        println!(
            "{marker} pane {index:<3} id={pane_id:<3} mode={mode:<14} target={target:<16} cwd={cwd}"
        );
        if !title.is_empty() {
            println!("    title: {title}");
        }
        if !caps.is_empty() {
            println!("    capabilities: {caps}");
        }
    }
    Ok(())
}

fn render_content_only(value: &Value) -> Result<()> {
    print!("{}", value["content"].as_str().unwrap_or(""));
    Ok(())
}

fn render_exec_result(value: &Value) -> Result<()> {
    let output = value["output"].as_str().unwrap_or("");
    print!("{output}");
    if let Some(code) = value.get("exit_code").and_then(Value::as_i64) {
        if !output.ends_with('\n') && !output.is_empty() {
            println!();
        }
        println!("[exit_code={code}]");
    }
    Ok(())
}

fn render_status_only(value: &Value) -> Result<()> {
    if let Some(status) = value["status"].as_str() {
        println!("{status}");
    } else {
        println!("{}", serde_json::to_string_pretty(value)?);
    }
    Ok(())
}

fn render_create_result(value: &Value) -> Result<()> {
    println!(
        "Created pane {} (id {}) in tab {}",
        value["pane_index"].as_u64().unwrap_or(0),
        value["pane_id"].as_u64().unwrap_or(0),
        value["tab_index"].as_u64().unwrap_or(0)
    );
    Ok(())
}

fn render_wait_result(value: &Value) -> Result<()> {
    let status = value["status"].as_str().unwrap_or("unknown");
    println!("status: {status}");
    let output = value["output"].as_str().unwrap_or("");
    if !output.is_empty() {
        println!();
        print!("{output}");
    }
    Ok(())
}

fn render_tmux_list(value: &Value) -> Result<()> {
    let Some(panes) = value
        .get("snapshot")
        .and_then(|snapshot| snapshot.get("panes"))
        .and_then(Value::as_array)
    else {
        return render_pretty_json(value);
    };

    for pane in panes {
        let session = pane["session_name"].as_str().unwrap_or("?");
        let window = pane["window_index"].as_u64().unwrap_or(0);
        let window_name = pane["window_name"].as_str().unwrap_or("");
        let pane_index = pane["pane_index"].as_u64().unwrap_or(0);
        let target = pane["target"].as_str().unwrap_or("");
        let command = pane["current_command"].as_str().unwrap_or("");
        let cwd = pane["current_path"].as_str().unwrap_or("");
        println!(
            "{session}:{window}.{pane_index}  target={target:<12} command={command:<16} cwd={cwd}"
        );
        if !window_name.is_empty() {
            println!("    window: {window_name}");
        }
    }
    Ok(())
}

fn render_tmux_capture(value: &Value) -> Result<()> {
    let content = value
        .get("capture")
        .and_then(|capture| capture.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("");
    print!("{content}");
    Ok(())
}

fn render_agent_ask(value: &Value) -> Result<()> {
    let content = value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or("");
    print!("{content}");
    Ok(())
}

fn render_new_conversation(value: &Value) -> Result<()> {
    println!(
        "Started new conversation {} on tab {}",
        value["conversation_id"].as_str().unwrap_or(""),
        value["tab_index"].as_u64().unwrap_or(0)
    );
    Ok(())
}

fn render_pretty_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn string_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}
