use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::stub::{CommandFinishedSignal, SurfaceSize};
use crate::vt::{ScreenSnapshot, VtScreen};

const DEFAULT_COLUMNS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
const MAX_TRANSCRIPT_BYTES: usize = 256 * 1024;

#[derive(Clone)]
pub struct LinuxPtyOptions {
    pub cwd: Option<PathBuf>,
    pub program: Option<String>,
    pub size: SurfaceSize,
    pub wake_generation: Option<Arc<AtomicU64>>,
}

impl Default for LinuxPtyOptions {
    fn default() -> Self {
        Self {
            cwd: None,
            program: None,
            size: SurfaceSize {
                columns: DEFAULT_COLUMNS,
                rows: DEFAULT_ROWS,
                width_px: 0,
                height_px: 0,
                cell_width_px: 0,
                cell_height_px: 0,
            },
            wake_generation: None,
        }
    }
}

pub struct LinuxPtySession {
    master: Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    shared: Arc<SessionShared>,
    size: Mutex<SurfaceSize>,
    title: Option<String>,
    current_dir: Option<String>,
    input_generation: AtomicU64,
    started_at: Instant,
}

struct SessionShared {
    screen: Arc<VtScreen>,
    transcript: Mutex<TranscriptBuffer>,
    alive: AtomicBool,
    needs_render: AtomicBool,
    wake_generation: Option<Arc<AtomicU64>>,
    finished_signal: Mutex<Option<CommandFinishedSignal>>,
    last_exit_code: Mutex<Option<i32>>,
    last_duration: Mutex<Option<Duration>>,
}

impl SessionShared {
    fn new(screen: Arc<VtScreen>, wake_generation: Option<Arc<AtomicU64>>) -> Self {
        Self {
            screen,
            transcript: Mutex::new(TranscriptBuffer::default()),
            alive: AtomicBool::new(true),
            needs_render: AtomicBool::new(false),
            wake_generation,
            finished_signal: Mutex::new(None),
            last_exit_code: Mutex::new(None),
            last_duration: Mutex::new(None),
        }
    }

    fn wake(&self) {
        if let Some(wake_generation) = self.wake_generation.as_ref() {
            wake_generation.fetch_add(1, Ordering::AcqRel);
        }
    }

    fn push_output(&self, chunk: &str) {
        self.transcript.lock().push(chunk);
        self.needs_render.store(true, Ordering::Release);
        self.wake();
    }
}

#[derive(Default)]
struct TranscriptBuffer {
    text: String,
}

impl TranscriptBuffer {
    fn push(&mut self, chunk: &str) {
        self.text.push_str(chunk);
        if self.text.len() <= MAX_TRANSCRIPT_BYTES {
            return;
        }

        let mut keep_from = self.text.len().saturating_sub(MAX_TRANSCRIPT_BYTES);
        while keep_from < self.text.len() && !self.text.is_char_boundary(keep_from) {
            keep_from += 1;
        }
        if keep_from > 0 {
            self.text.drain(..keep_from);
        }
    }

    fn recent_lines(&self, max_lines: usize) -> Vec<String> {
        if max_lines == 0 {
            return Vec::new();
        }
        let sanitized = sanitize_terminal_output(&self.text);
        let mut lines: Vec<String> = sanitized
            .lines()
            .rev()
            .take(max_lines)
            .map(ToOwned::to_owned)
            .collect();
        lines.reverse();
        lines
    }

    fn search(&self, pattern: &str, limit: usize) -> Vec<(usize, String)> {
        if pattern.is_empty() || limit == 0 {
            return Vec::new();
        }
        sanitize_terminal_output(&self.text)
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains(pattern))
            .take(limit)
            .map(|(idx, line)| (idx, line.to_string()))
            .collect()
    }
}

impl LinuxPtySession {
    pub fn spawn(options: LinuxPtyOptions) -> Result<Self> {
        let pty_system = native_pty_system();
        let pty_size = pty_size_from_surface(&options.size);
        let pair = pty_system
            .openpty(pty_size)
            .context("failed to open linux pty")?;

        let shell_program = options.program.clone();
        let spawn_cwd = options.cwd.clone();

        let mut command = match options.program.as_ref() {
            Some(program) => {
                let mut command = CommandBuilder::new(program);
                configure_shell_startup(program, &mut command);
                command
            }
            None => CommandBuilder::new_default_prog(),
        };
        command.env("TERM", "xterm-256color");
        if let Some(cwd) = options.cwd.as_ref() {
            command.cwd(cwd);
        }

        log::info!(
            "linux pty spawning shell program={:?} cwd={:?} cols={} rows={}",
            shell_program.as_deref().unwrap_or("<default>"),
            spawn_cwd.as_ref().map(|path| path.display().to_string()),
            options.size.columns,
            options.size.rows
        );

        let reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone linux pty reader")?;
        let writer = Arc::new(Mutex::new(
            pair.master
                .take_writer()
                .context("failed to take linux pty writer")?,
        ));
        let screen = Arc::new(
            VtScreen::new_with_write_pty(
                options.size.columns.max(1),
                options.size.rows.max(1),
                None,
                Some({
                    let writer = writer.clone();
                    Arc::new(move |data: &[u8]| {
                        if let Err(err) = writer.lock().write_all(data) {
                            log::debug!("linux vt write_pty failed: {err:#}");
                        }
                    })
                }),
            )
            .context("failed to create linux vt screen")?,
        );
        let child = pair
            .slave
            .spawn_command(command)
            .context("failed to spawn shell in linux pty")?;

        let shared = Arc::new(SessionShared::new(screen, options.wake_generation));
        spawn_reader_thread(reader, shared.clone());

        Ok(Self {
            master: Mutex::new(pair.master),
            writer,
            child: Mutex::new(child),
            shared,
            size: Mutex::new(options.size),
            title: Some(default_title(
                options.cwd.as_deref(),
                options.program.as_deref(),
            )),
            current_dir: options.cwd.map(|cwd| cwd.to_string_lossy().to_string()),
            input_generation: AtomicU64::new(0),
            started_at: Instant::now(),
        })
    }

    pub fn size(&self) -> SurfaceSize {
        *self.size.lock()
    }

    pub fn set_pixel_size(&self, width_px: u32, height_px: u32) -> Result<()> {
        let mut size = self.size.lock();
        size.width_px = width_px;
        size.height_px = height_px;
        self.shared
            .screen
            .resize(
                size.columns.max(1),
                size.rows.max(1),
                size.cell_width_px.max(1),
                size.cell_height_px.max(1),
            )
            .context("failed to resize linux vt screen")?;
        self.master
            .lock()
            .resize(pty_size_from_surface(&size))
            .context("failed to resize linux pty")
    }

    pub fn resize(&self, size: SurfaceSize) -> Result<()> {
        *self.size.lock() = size;
        self.shared
            .screen
            .resize(
                size.columns.max(1),
                size.rows.max(1),
                size.cell_width_px.max(1),
                size.cell_height_px.max(1),
            )
            .context("failed to resize linux vt screen")?;
        self.master
            .lock()
            .resize(pty_size_from_surface(&size))
            .context("failed to resize linux pty")
    }

    pub fn write_input(&self, data: &[u8]) -> Result<()> {
        self.writer
            .lock()
            .write_all(data)
            .context("failed to write to linux pty")?;
        self.input_generation.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn title(&self) -> Option<String> {
        self.title.clone()
    }

    pub fn current_dir(&self) -> Option<String> {
        self.current_dir.clone()
    }

    pub fn is_alive(&self) -> bool {
        self.poll_child_status();
        self.shared.alive.load(Ordering::Acquire)
    }

    pub fn take_command_finished(&self) -> Option<CommandFinishedSignal> {
        self.poll_child_status();
        self.shared.finished_signal.lock().take()
    }

    pub fn last_exit_code(&self) -> Option<i32> {
        self.poll_child_status();
        *self.shared.last_exit_code.lock()
    }

    pub fn last_command_duration(&self) -> Option<Duration> {
        self.poll_child_status();
        *self.shared.last_duration.lock()
    }

    pub fn input_generation(&self) -> u64 {
        self.input_generation.load(Ordering::Relaxed)
    }

    pub fn take_needs_render(&self) -> bool {
        self.shared.needs_render.swap(false, Ordering::AcqRel)
    }

    pub fn read_screen_text(&self, max_lines: usize) -> Vec<String> {
        snapshot_to_lines(&self.shared.screen.snapshot(), max_lines)
    }

    pub fn read_recent_lines(&self, max_lines: usize) -> Vec<String> {
        self.shared.transcript.lock().recent_lines(max_lines)
    }

    pub fn search_text(&self, pattern: &str, limit: usize) -> Vec<(usize, String)> {
        self.shared.transcript.lock().search(pattern, limit)
    }

    pub fn is_bracketed_paste(&self) -> bool {
        self.shared.screen.is_bracketed_paste()
    }

    pub fn is_decckm(&self) -> bool {
        self.shared.screen.is_decckm()
    }

    pub fn set_dark_mode(&self, dark: bool) {
        self.shared.screen.set_dark_mode(dark);
    }

    fn poll_child_status(&self) {
        if !self.shared.alive.load(Ordering::Acquire) {
            return;
        }

        let Ok(Some(status)) = self.child.lock().try_wait() else {
            return;
        };

        self.shared.alive.store(false, Ordering::Release);
        let exit_code = i32::try_from(status.exit_code()).unwrap_or(i32::MAX);
        let duration = self.started_at.elapsed();
        *self.shared.last_exit_code.lock() = Some(exit_code);
        *self.shared.last_duration.lock() = Some(duration);
        *self.shared.finished_signal.lock() = Some(CommandFinishedSignal {
            exit_code: Some(exit_code),
            duration,
        });
        self.shared.needs_render.store(true, Ordering::Release);
        self.shared.wake();
    }
}

impl Drop for LinuxPtySession {
    fn drop(&mut self) {
        if let Err(err) = self.child.lock().kill() {
            log::debug!("failed to terminate linux pty child during drop: {err}");
        }
        self.shared.alive.store(false, Ordering::Release);
        self.shared.needs_render.store(true, Ordering::Release);
        self.shared.wake();
    }
}

fn spawn_reader_thread(mut reader: Box<dyn Read + Send>, shared: Arc<SessionShared>) {
    std::thread::Builder::new()
        .name("con-linux-pty-reader".into())
        .spawn(move || {
            let mut buffer = [0_u8; 8192];
            let mut logged_first_chunk = false;
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        shared.screen.feed(&buffer[..read]);
                        let chunk = String::from_utf8_lossy(&buffer[..read]);
                        if !logged_first_chunk {
                            logged_first_chunk = true;
                            let preview = sanitize_terminal_output(chunk.as_ref());
                            let preview = preview.chars().take(160).collect::<String>();
                            log::info!(
                                "linux pty received first output chunk bytes={} preview={:?}",
                                read,
                                preview
                            );
                        }
                        shared.push_output(chunk.as_ref());
                    }
                    Err(err) if err.kind() == ErrorKind::Interrupted => continue,
                    Err(err) => {
                        log::debug!("linux pty reader terminated: {err}");
                        break;
                    }
                }
            }
        })
        .expect("failed to spawn linux pty reader thread");
}

fn default_title(cwd: Option<&Path>, program: Option<&str>) -> String {
    if let Some(name) = cwd
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
    {
        return name.to_string();
    }

    if let Some(name) = program
        .and_then(|program| Path::new(program).file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
    {
        return name.to_string();
    }

    "shell".to_string()
}

fn configure_shell_startup(program: &str, command: &mut CommandBuilder) {
    let Some(shell) = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return;
    };

    match shell {
        "fish" => {
            command.arg("--interactive");
        }
        "pwsh" => {
            command.arg("-NoLogo");
        }
        "sh" | "dash" | "ksh" | "mksh" | "xonsh" | "nu" => {
            if let Some(flag) = interactive_shell_flag(program) {
                command.arg(flag);
            }
        }
        _ => {
            if let Some(flag) = interactive_shell_flag(program) {
                command.arg(flag);
            }
        }
    }
}

fn interactive_shell_flag(program: &str) -> Option<&'static str> {
    let shell = Path::new(program).file_name()?.to_str()?;
    match shell {
        "bash" | "sh" | "zsh" | "dash" | "ksh" | "mksh" | "nu" | "xonsh" => Some("-i"),
        _ => None,
    }
}

fn pty_size_from_surface(size: &SurfaceSize) -> PtySize {
    PtySize {
        rows: size.rows.max(1),
        cols: size.columns.max(1),
        pixel_width: size.width_px.min(u32::from(u16::MAX)) as u16,
        pixel_height: size.height_px.min(u32::from(u16::MAX)) as u16,
    }
}

fn snapshot_to_lines(snapshot: &ScreenSnapshot, max_lines: usize) -> Vec<String> {
    if max_lines == 0 || snapshot.cols == 0 || snapshot.rows == 0 {
        return Vec::new();
    }

    let cols = usize::from(snapshot.cols);
    let mut lines = Vec::with_capacity(usize::from(snapshot.rows));

    for row in 0..usize::from(snapshot.rows) {
        let row_start = row * cols;
        let row_end = row_start + cols;
        let Some(cells) = snapshot.cells.get(row_start..row_end) else {
            break;
        };

        let mut line = String::with_capacity(cols);
        for cell in cells {
            let ch = match cell.codepoint {
                0 => ' ',
                codepoint => char::from_u32(codepoint).unwrap_or('\u{FFFD}'),
            };
            line.push(ch);
        }

        let trimmed = line.trim_end_matches(' ');
        lines.push(trimmed.to_string());
    }

    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }

    if lines.len() > max_lines {
        lines.drain(..lines.len() - max_lines);
    }

    lines
}

fn sanitize_terminal_output(raw: &str) -> String {
    #[derive(Clone, Copy)]
    enum EscapeState {
        None,
        Esc,
        Csi,
        Osc,
        OscEsc,
        Ss3,
        Charset,
        Dcs,
        DcsEsc,
    }

    let mut output = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut index = 0;
    let mut state = EscapeState::None;

    while index < bytes.len() {
        let byte = bytes[index];
        match state {
            EscapeState::None => match byte {
                b'\x1b' => {
                    state = EscapeState::Esc;
                    index += 1;
                }
                b'\r' => {
                    if bytes.get(index + 1) != Some(&b'\n') {
                        clear_current_line(&mut output);
                    }
                    index += 1;
                }
                b'\x08' => {
                    if !output.ends_with('\n') {
                        output.pop();
                    }
                    index += 1;
                }
                b'\t' => {
                    output.push_str("    ");
                    index += 1;
                }
                b'\n' => {
                    output.push('\n');
                    index += 1;
                }
                0x00..=0x1f | 0x7f => {
                    index += 1;
                }
                _ => {
                    let ch = raw[index..]
                        .chars()
                        .next()
                        .expect("valid utf-8 chunk while sanitizing pty output");
                    output.push(ch);
                    index += ch.len_utf8();
                }
            },
            EscapeState::Esc => {
                state = match byte {
                    b'[' => EscapeState::Csi,
                    b']' => EscapeState::Osc,
                    b'O' => EscapeState::Ss3,
                    b'(' | b')' | b'*' | b'+' => EscapeState::Charset,
                    b'P' | b'X' | b'^' | b'_' => EscapeState::Dcs,
                    _ => EscapeState::None,
                };
                index += 1;
            }
            EscapeState::Csi => {
                if (0x40..=0x7e).contains(&byte) {
                    state = EscapeState::None;
                }
                index += 1;
            }
            EscapeState::Osc => {
                match byte {
                    b'\x07' => state = EscapeState::None,
                    b'\x1b' => state = EscapeState::OscEsc,
                    _ => {}
                }
                index += 1;
            }
            EscapeState::OscEsc => {
                state = if byte == b'\\' {
                    EscapeState::None
                } else {
                    EscapeState::Osc
                };
                index += 1;
            }
            EscapeState::Ss3 => {
                if (0x40..=0x7e).contains(&byte) {
                    state = EscapeState::None;
                }
                index += 1;
            }
            EscapeState::Charset => {
                state = EscapeState::None;
                index += 1;
            }
            EscapeState::Dcs => {
                match byte {
                    b'\x07' => state = EscapeState::None,
                    b'\x1b' => state = EscapeState::DcsEsc,
                    _ => {}
                }
                index += 1;
            }
            EscapeState::DcsEsc => {
                state = if byte == b'\\' {
                    EscapeState::None
                } else {
                    EscapeState::Dcs
                };
                index += 1;
            }
        }
    }

    output
}

fn clear_current_line(output: &mut String) {
    while output.chars().last().is_some_and(|ch| ch != '\n') {
        output.pop();
    }
}

#[cfg(test)]
mod tests {
    use crate::vt::{Cell, Cursor, ScreenSnapshot};

    use super::{TranscriptBuffer, sanitize_terminal_output, snapshot_to_lines};

    #[test]
    fn transcript_buffer_returns_recent_lines_in_order() {
        let mut transcript = TranscriptBuffer::default();
        transcript.push("one\ntwo\nthree\nfour\n");

        assert_eq!(
            transcript.recent_lines(2),
            vec!["three".to_string(), "four".to_string()]
        );
    }

    #[test]
    fn transcript_buffer_search_is_bounded() {
        let mut transcript = TranscriptBuffer::default();
        transcript.push("alpha\nbeta\nalphabet\n");

        assert_eq!(
            transcript.search("alpha", 1),
            vec![(0, "alpha".to_string())]
        );
    }

    #[test]
    fn sanitize_terminal_output_strips_ansi_sequences() {
        assert_eq!(
            sanitize_terminal_output("\x1b]0;title\x07\x1b[31mhello\x1b[0m"),
            "hello"
        );
    }

    #[test]
    fn sanitize_terminal_output_honors_carriage_return_rewrites() {
        assert_eq!(sanitize_terminal_output("loading\rready"), "ready");
    }

    #[test]
    fn snapshot_to_lines_trims_trailing_blank_rows() {
        let mut cells = vec![Cell::default(); 6];
        cells[0].codepoint = 'p' as u32;
        cells[1].codepoint = 's' as u32;
        cells[2].codepoint = '1' as u32;

        let snapshot = ScreenSnapshot {
            cols: 3,
            rows: 2,
            cells,
            dirty_rows: vec![0, 1],
            cursor: Cursor::default(),
            title: None,
            generation: 1,
        };

        assert_eq!(snapshot_to_lines(&snapshot, 10), vec!["ps1".to_string()]);
    }
}
