use anyhow::Result;
use crossbeam_channel::Receiver;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize as PtyPtySize, native_pty_system};
use std::io::{Read, Write};
use std::thread;

#[derive(Debug, Clone, Copy)]
pub struct PtySize {
    pub rows: u16,
    pub cols: u16,
}

impl Default for PtySize {
    fn default() -> Self {
        Self { rows: 24, cols: 80 }
    }
}

/// Events from the PTY
pub enum PtyEvent {
    /// Data read from the terminal
    Output(Vec<u8>),
    /// Child process exited
    Exit(Option<u32>),
}

/// Manages a pseudo-terminal
pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    event_rx: Receiver<PtyEvent>,
    _reader_thread: thread::JoinHandle<()>,
}

impl Pty {
    pub fn spawn(size: PtySize) -> Result<Self> {
        Self::spawn_in(size, None)
    }

    pub fn spawn_in(size: PtySize, cwd: Option<&str>) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtyPtySize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // Detect user's shell
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.arg("-l"); // login shell
        let working_dir = cwd
            .filter(|d| std::path::Path::new(d).is_dir())
            .unwrap_or_else(|| {
                // Cannot return a &str from env::var, so fall back to "/"
                // The HOME default is handled below
                "/"
            });
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        cmd.cwd(if working_dir == "/" {
            &home
        } else {
            working_dir
        });

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave); // Close slave side in parent

        let writer = pair.master.take_writer()?;
        let mut reader = pair.master.try_clone_reader()?;

        let (event_tx, event_rx) = crossbeam_channel::unbounded();

        // IO read thread
        let reader_thread = thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        let _ = event_tx.send(PtyEvent::Exit(None));
                        break;
                    }
                    Ok(n) => {
                        let _ = event_tx.send(PtyEvent::Output(buf[..n].to_vec()));
                    }
                    Err(e) => {
                        log::error!("PTY read error: {}", e);
                        let _ = event_tx.send(PtyEvent::Exit(None));
                        break;
                    }
                }
            }
        });

        Ok(Self {
            master: pair.master,
            child,
            writer,
            event_rx,
            _reader_thread: reader_thread,
        })
    }

    /// Write bytes to the PTY (keyboard input → shell)
    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        self.writer.write_all(data)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Write a string to the PTY
    pub fn write_str(&mut self, s: &str) -> Result<()> {
        self.write(s.as_bytes())
    }

    /// Get the event receiver for reading PTY output
    pub fn events(&self) -> &Receiver<PtyEvent> {
        &self.event_rx
    }

    /// Resize the PTY
    pub fn resize(&self, size: PtySize) -> Result<()> {
        self.master.resize(PtyPtySize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    /// Check if child is still running
    pub fn is_alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}
