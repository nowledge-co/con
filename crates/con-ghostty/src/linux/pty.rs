use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::stub::{CommandFinishedSignal, SurfaceSize};

const DEFAULT_COLUMNS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
const MAX_TRANSCRIPT_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone)]
pub struct LinuxPtyOptions {
    pub cwd: Option<PathBuf>,
    pub program: Option<String>,
    pub size: SurfaceSize,
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
        }
    }
}

pub struct LinuxPtySession {
    master: Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    shared: Arc<SessionShared>,
    size: Mutex<SurfaceSize>,
    title: Option<String>,
    current_dir: Option<String>,
    input_generation: AtomicU64,
    started_at: Instant,
}

struct SessionShared {
    transcript: Mutex<TranscriptBuffer>,
    alive: AtomicBool,
    needs_render: AtomicBool,
    finished_signal: Mutex<Option<CommandFinishedSignal>>,
    last_exit_code: Mutex<Option<i32>>,
    last_duration: Mutex<Option<Duration>>,
}

impl SessionShared {
    fn new() -> Self {
        Self {
            transcript: Mutex::new(TranscriptBuffer::default()),
            alive: AtomicBool::new(true),
            needs_render: AtomicBool::new(false),
            finished_signal: Mutex::new(None),
            last_exit_code: Mutex::new(None),
            last_duration: Mutex::new(None),
        }
    }

    fn push_output(&self, chunk: &str) {
        self.transcript.lock().push(chunk);
        self.needs_render.store(true, Ordering::Release);
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

        let mut command = match options.program.as_ref() {
            Some(program) => CommandBuilder::new(program),
            None => CommandBuilder::new_default_prog(),
        };
        command.env("TERM", "xterm-256color");
        if let Some(cwd) = options.cwd.as_ref() {
            command.cwd(cwd);
        }

        let reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone linux pty reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to take linux pty writer")?;
        let child = pair
            .slave
            .spawn_command(command)
            .context("failed to spawn shell in linux pty")?;

        let shared = Arc::new(SessionShared::new());
        spawn_reader_thread(reader, shared.clone());

        Ok(Self {
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
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
        self.master
            .lock()
            .resize(pty_size_from_surface(&size))
            .context("failed to resize linux pty")
    }

    pub fn resize(&self, size: SurfaceSize) -> Result<()> {
        *self.size.lock() = size;
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
        self.read_recent_lines(max_lines)
    }

    pub fn read_recent_lines(&self, max_lines: usize) -> Vec<String> {
        self.shared.transcript.lock().recent_lines(max_lines)
    }

    pub fn search_text(&self, pattern: &str, limit: usize) -> Vec<(usize, String)> {
        self.shared.transcript.lock().search(pattern, limit)
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
    }
}

impl Drop for LinuxPtySession {
    fn drop(&mut self) {
        if let Err(err) = self.child.lock().kill() {
            log::debug!("failed to terminate linux pty child during drop: {err}");
        }
        self.shared.alive.store(false, Ordering::Release);
    }
}

fn spawn_reader_thread(mut reader: Box<dyn Read + Send>, shared: Arc<SessionShared>) {
    std::thread::Builder::new()
        .name("con-linux-pty-reader".into())
        .spawn(move || {
            let mut buffer = [0_u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        let chunk = String::from_utf8_lossy(&buffer[..read]);
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

fn pty_size_from_surface(size: &SurfaceSize) -> PtySize {
    PtySize {
        rows: size.rows.max(1),
        cols: size.columns.max(1),
        pixel_width: size.width_px.min(u32::from(u16::MAX)) as u16,
        pixel_height: size.height_px.min(u32::from(u16::MAX)) as u16,
    }
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
    use super::{TranscriptBuffer, sanitize_terminal_output};

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
}
