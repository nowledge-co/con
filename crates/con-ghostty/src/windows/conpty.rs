//! ConPTY wrapper.
//!
//! ConPTY (Windows 10 1809+) is the modern pseudo-console API. The
//! host:
//!
//! 1. Creates two anonymous pipe pairs via `CreatePipe`.
//! 2. Calls `CreatePseudoConsole(size, hInputRead, hOutputWrite, 0,
//!    &hpcon)`. The host keeps `hInputWrite` and `hOutputRead`, drops
//!    the child-side ends.
//! 3. Spawns the shell via `CreateProcessW` with a `STARTUPINFOEXW`
//!    whose attribute list contains
//!    `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE = HPCON`.
//! 4. Reads/writes via the kept handles; resizes via
//!    `ResizePseudoConsole`; tears down via `ClosePseudoConsole`.
//!
//! Reference: https://learn.microsoft.com/en-us/windows/console/creating-a-pseudoconsole-session

use std::ffi::OsString;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::sync::Arc;
use std::sync::OnceLock;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use windows::Win32::Foundation::{
    CloseHandle, DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, TRUE,
};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::Console::{
    COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess, INFINITE, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION,
    STARTUPINFOEXW, STARTUPINFOW, TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows::core::PWSTR;

fn perf_trace_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("CON_GHOSTTY_PROFILE").is_some_and(|v| !v.is_empty() && v != "0")
    })
}

/// Close the pseudo-console if it hasn't been closed yet, clearing the
/// slot so a second caller becomes a no-op. Called from two places —
/// `ConPty::drop` (teardown triggered by the UI) and the exit-watcher
/// thread (teardown triggered by the child shell exiting on its own,
/// e.g. the user typing `exit`). Whichever runs first closes; the
/// other finds `None` and returns immediately.
fn close_hpcon_slot(slot: &Mutex<Option<HPCON>>) {
    if let Some(hpcon) = slot.lock().take() {
        // SAFETY: We own the HPCON; the `take` guarantees no one
        // else can close it. `ClosePseudoConsole` blocks until
        // conhost drains the output pipe, which is why the caller
        // force-kills the shell first when needed (see
        // `ConPty::drop`).
        unsafe { ClosePseudoConsole(hpcon) }
    }
}

/// Owned HANDLE that closes itself on drop.
///
/// We store the raw pointer as `usize` so the wrapper is unconditionally
/// `Send`/`Sync` — windows-rs 0.61's `HANDLE` is a tuple newtype around
/// `*mut c_void` whose auto-trait derivation propagates `!Send`. Rather
/// than rely on `unsafe impl Send for OwnedHandle` (which has flaky
/// inference inside generic closures), we keep the raw value in a
/// scalar and reconstruct the `HANDLE` inside `as_handle()`.
struct OwnedHandle(usize);

impl OwnedHandle {
    fn from_handle(h: HANDLE) -> Self {
        Self(h.0 as usize)
    }
    fn as_handle(&self) -> HANDLE {
        HANDLE(self.0 as *mut _)
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        let h = self.as_handle();
        if !h.is_invalid() {
            // SAFETY: handle ownership is unique (we never clone).
            unsafe {
                let _ = CloseHandle(h);
            }
        }
    }
}

/// A live ConPTY session.
pub struct ConPty {
    /// Pseudo-console handle, shared with the exit-watcher thread.
    /// Either `ConPty::drop` or the watcher takes the `Option<HPCON>`
    /// out and calls `ClosePseudoConsole`; the loser finds `None` and
    /// skips. See `close_hpcon_slot`.
    pcon: Arc<Mutex<Option<HPCON>>>,
    /// Host end of the pipe the child reads from.
    input_write: Arc<Mutex<OwnedHandle>>,
    /// Process handle (kept so callers can `WaitForSingleObject` for exit).
    process: OwnedHandle,
    /// Child thread handle.
    _thread: OwnedHandle,
    /// Output reader thread; joined on drop.
    output_thread: Option<JoinHandle<()>>,
    /// Background thread that waits on the child process handle and
    /// closes the pseudo-console when the shell exits on its own
    /// (e.g. user typed `exit`). Without this, conhost keeps the
    /// output pipe alive after the shell dies and the reader sits in
    /// `ReadFile` forever — the pane looks frozen and typing into it
    /// writes into a dead PTY. Joined on drop.
    exit_watcher: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone, Copy)]
pub struct PtySize {
    pub cols: u16,
    pub rows: u16,
}

impl ConPty {
    /// Spawn a child shell, wire up ConPTY, and start a background reader
    /// that calls `on_output` for each chunk of bytes the shell writes.
    pub fn spawn<F>(command_line: &str, size: PtySize, on_output: F) -> Result<Self>
    where
        F: FnMut(&[u8]) + Send + 'static,
    {
        let (input_read, input_write) = create_pipe().context("input pipe")?;
        let (output_read, output_write) = create_pipe().context("output pipe")?;

        let coord = COORD {
            X: size.cols as i16,
            Y: size.rows as i16,
        };

        // SAFETY: input_read and output_write are valid owned handles
        // captured by ConPTY. `0` flags = no special behavior.
        let hpcon: HPCON = unsafe {
            CreatePseudoConsole(coord, input_read.as_handle(), output_write.as_handle(), 0)
        }
        .context("CreatePseudoConsole failed")?;

        // Per Microsoft docs, after CreatePseudoConsole the captured
        // child-side ends should be closed by the host so only the
        // PCON owns them.
        drop(input_read);
        drop(output_write);

        let pcon = Arc::new(Mutex::new(Some(hpcon)));

        let (startup_info, attribute_buffer) = build_startup_info(hpcon)?;

        let mut command_line_w: Vec<u16> = OsString::from(command_line)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut process_info = PROCESS_INFORMATION::default();

        // Only two flags are safe with `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE`:
        // CREATE_NO_WINDOW, DETACHED_PROCESS, and CREATE_NEW_CONSOLE are
        // all INCOMPATIBLE with ConPTY — they prevent the conhost.exe
        // helper (which the pty attribute launches internally) from
        // starting, so the child never writes anything to the pipe.
        // Seen by the user: powershell spawned successfully but zero
        // output bytes ever reached the reader thread. The visible
        // console flash on startup is conhost briefly initializing;
        // the hide-window story is a later polish item.
        let creation_flags = EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT;

        // SAFETY: command_line_w is mutable + NUL-terminated;
        // `attribute_buffer` keeps the attribute list alive until after
        // CreateProcessW returns. The HPCON travels through the
        // PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE attribute, so the child
        // must NOT inherit the host process' unrelated stdout/stderr
        // handles. This matters when con itself is launched with
        // redirected logs (`*> con-profile.log`): inheriting those
        // handles lets PowerShell write its banner/prompt to the log
        // instead of through ConPTY, leaving the pane blank.
        log::info!("ConPTY CreateProcess: inherit_handles=false shell={command_line}");
        let create_result = unsafe {
            CreateProcessW(
                None,
                Some(PWSTR(command_line_w.as_mut_ptr())),
                None,
                None,
                false,
                creation_flags,
                None,
                None,
                &startup_info.StartupInfo as *const _,
                &mut process_info,
            )
        };

        // Free the attribute list buffer regardless of outcome.
        unsafe {
            DeleteProcThreadAttributeList(startup_info.lpAttributeList);
        }
        // Keep the buffer alive until after DeleteProcThreadAttributeList.
        drop(attribute_buffer);

        create_result.context("CreateProcessW failed for ConPTY child")?;

        let process = OwnedHandle::from_handle(process_info.hProcess);
        let thread = OwnedHandle::from_handle(process_info.hThread);

        let input_write = Arc::new(Mutex::new(input_write));
        let output_thread = spawn_output_reader(output_read, on_output);

        // Duplicate the process handle so the watcher thread can wait
        // on it independently of the `OwnedHandle` stored on `Self`.
        // The original is still needed for `process_handle()` and gets
        // closed by `OwnedHandle::drop` when `ConPty` is dropped; the
        // duplicate is closed by the watcher thread when it finishes.
        // SAFETY: GetCurrentProcess returns a pseudo-handle that does
        // not need closing; process_info.hProcess is a valid handle we
        // own; DUPLICATE_SAME_ACCESS copies the ACCESS mask verbatim.
        let mut dup_handle = HANDLE::default();
        let dup_ok = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                process_info.hProcess,
                GetCurrentProcess(),
                &mut dup_handle,
                0,
                false,
                DUPLICATE_SAME_ACCESS,
            )
        };
        let exit_watcher = if dup_ok.is_ok() {
            let dup_owned = OwnedHandle::from_handle(dup_handle);
            let pcon_for_watcher = Arc::clone(&pcon);
            let watcher = thread::Builder::new()
                .name("conpty-exit-watcher".into())
                .spawn(move || {
                    let h = dup_owned.as_handle();
                    // SAFETY: dup_owned keeps the handle alive for
                    // the duration of this thread; INFINITE blocks
                    // until the child process terminates.
                    let wait_result = unsafe { WaitForSingleObject(h, INFINITE) };
                    log::info!(
                        "conpty exit-watcher: child exited (wait_result={:?}), closing pseudo-console",
                        wait_result
                    );
                    // Close the pseudo-console so conhost releases
                    // the pipe write-end; the output reader's
                    // `ReadFile` will then see EOF and the reader
                    // thread exits. If `ConPty::drop` raced us and
                    // already closed the HPCON, `close_hpcon_slot`
                    // finds `None` and returns immediately.
                    close_hpcon_slot(&pcon_for_watcher);
                    // dup_owned drops here, closing the duplicate.
                })
                .expect("conpty exit-watcher spawn failed");
            Some(watcher)
        } else {
            log::warn!(
                "DuplicateHandle for exit-watcher failed; pane will not auto-close on `exit`"
            );
            None
        };

        Ok(Self {
            pcon,
            input_write,
            process,
            _thread: thread,
            output_thread: Some(output_thread),
            exit_watcher,
        })
    }

    pub fn write(&self, bytes: &[u8]) -> io::Result<usize> {
        let guard = self.input_write.lock();
        let mut written = 0u32;
        // SAFETY: guard handle is a valid pipe.
        unsafe {
            WriteFile(guard.as_handle(), Some(bytes), Some(&mut written), None)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.message()))?;
        }
        Ok(written as usize)
    }

    pub fn resize(&self, size: PtySize) -> Result<()> {
        let coord = COORD {
            X: size.cols as i16,
            Y: size.rows as i16,
        };
        // Read the HPCON through the shared slot. If the shell has
        // already exited (exit-watcher took the handle), the slot is
        // `None` and the resize is a no-op — there's nothing to resize.
        let guard = self.pcon.lock();
        let Some(hpcon) = *guard else {
            return Ok(());
        };
        // SAFETY: hpcon is a valid HPCON owned by this ConPty; the
        // lock guards against a concurrent close.
        unsafe { ResizePseudoConsole(hpcon, coord) }
            .map_err(|e| anyhow!("ResizePseudoConsole failed: {}", e.message()))
    }

    pub fn process_handle(&self) -> HANDLE {
        self.process.as_handle()
    }

    /// `true` while the pseudo-console is still open. Flips to `false`
    /// atomically when either `Drop` or the exit-watcher thread takes
    /// the HPCON out of the slot — i.e. when the child shell has
    /// exited and `ClosePseudoConsole` has been (or is being) called.
    pub fn is_alive(&self) -> bool {
        self.pcon.lock().is_some()
    }
}

impl Drop for ConPty {
    fn drop(&mut self) {
        // Teardown order matters here. The reader thread reads the
        // host side of the output pipe; the pipe's write-end lives
        // inside conhost (spawned by CreatePseudoConsole), NOT inside
        // the child shell. So terminating the child is not enough —
        // conhost keeps the pipe open and `ReadFile` never returns
        // EOF. We must close the pseudo-console to make conhost exit
        // and release the write-end, THEN join the reader.
        //
        // But `ClosePseudoConsole` itself blocks waiting for the child
        // to drain its output — a hung shell blocks forever. So the
        // correct sequence is:
        //   1. TerminateProcess(child)      — makes (2) return promptly
        //   2. ClosePseudoConsole(hpcon)    — releases the pipe write-end
        //   3. JoinHandle::join(reader)     — reader's ReadFile now sees EOF
        //   4. JoinHandle::join(watcher)    — watcher's WaitForSingleObject
        //                                     returns now that the child died
        //
        // SAFETY: process handle is owned by us; TerminateProcess with
        // exit code 0 is the Windows equivalent of SIGKILL. We don't
        // care about the returned bool — even if the process already
        // exited naturally, this is a no-op.
        let process = self.process.as_handle();
        if !process.is_invalid() {
            unsafe {
                let _ = TerminateProcess(process, 0);
            }
        }

        // Close the pseudo-console (idempotent with the watcher's
        // close via `close_hpcon_slot`).
        close_hpcon_slot(&self.pcon);

        if let Some(handle) = self.output_thread.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.exit_watcher.take() {
            let _ = handle.join();
        }
    }
}

fn create_pipe() -> Result<(OwnedHandle, OwnedHandle)> {
    let mut read = HANDLE::default();
    let mut write = HANDLE::default();
    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: TRUE,
    };
    // SAFETY: `read` and `write` are out parameters; `security` lives on
    // this stack frame for the duration of the call.
    unsafe { CreatePipe(&mut read, &mut write, Some(&security), 0) }
        .context("CreatePipe failed")?;
    Ok((
        OwnedHandle::from_handle(read),
        OwnedHandle::from_handle(write),
    ))
}

/// Build a `STARTUPINFOEXW` whose attribute list binds the pseudo-console
/// handle. The returned buffer backs the attribute list and must outlive
/// the `STARTUPINFOEXW`.
fn build_startup_info(hpcon: HPCON) -> Result<(STARTUPINFOEXW, Vec<u8>)> {
    let mut required_size: usize = 0;
    // First call to determine required buffer size. Documented to fail
    // with ERROR_INSUFFICIENT_BUFFER and write the size; we ignore the
    // error and just use the size.
    // SAFETY: NULL list + dwAttributeCount=1 means "tell me how big".
    unsafe {
        let _ = InitializeProcThreadAttributeList(
            Some(LPPROC_THREAD_ATTRIBUTE_LIST(ptr::null_mut())),
            1,
            None,
            &mut required_size,
        );
    }

    let mut buffer = vec![0u8; required_size];
    let attribute_list = LPPROC_THREAD_ATTRIBUTE_LIST(buffer.as_mut_ptr() as *mut _);

    // SAFETY: buffer sized correctly; attribute_list points into buffer.
    unsafe {
        InitializeProcThreadAttributeList(Some(attribute_list), 1, None, &mut required_size)
            .context("InitializeProcThreadAttributeList failed")?;
    }

    // SAFETY: for PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE the kernel
    // stores `lpValue` directly as the pseudo-console handle — NOT a
    // pointer to a value to copy. Match the canonical Microsoft
    // sample (github.com/microsoft/terminal ConPTY sample): pass the
    // HPCON (which is itself a pointer/void*) as lpValue.
    //
    // The previous `&hpcon` form had two bugs: (1) it pointed at a
    // local in `build_startup_info` that died when the function
    // returned, leaving a dangling stack pointer in the attribute
    // list for CreateProcessW to dereference; (2) even on a
    // longer-lived HPCON, this attribute expects the handle as
    // lpValue, not a pointer to it. Result seen by the user:
    // powershell spawned without PTY binding, opened its own console
    // window, wrote nothing to our pipe.
    unsafe {
        UpdateProcThreadAttribute(
            attribute_list,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            Some(hpcon.0 as *const _),
            size_of::<HPCON>(),
            None,
            None,
        )
        .context("UpdateProcThreadAttribute failed")?;
    }

    let startup_info = STARTUPINFOEXW {
        StartupInfo: STARTUPINFOW {
            cb: size_of::<STARTUPINFOEXW>() as u32,
            ..Default::default()
        },
        lpAttributeList: attribute_list,
    };

    Ok((startup_info, buffer))
}

fn spawn_output_reader<F>(read_handle: OwnedHandle, mut on_output: F) -> JoinHandle<()>
where
    F: FnMut(&[u8]) + Send + 'static,
{
    thread::Builder::new()
        .name("conpty-output-reader".into())
        .spawn(move || {
            let mut buf = [0u8; 4096];
            let mut total_bytes: u64 = 0;
            let mut chunk_index: u64 = 0;
            let started = perf_trace_enabled().then(Instant::now);
            let mut last_chunk_at: Option<Instant> = None;
            loop {
                let mut bytes_read: u32 = 0;
                let handle = read_handle.as_handle();
                // SAFETY: handle is valid for the lifetime of this
                // thread (OwnedHandle moved in via `read_handle`).
                let result = unsafe {
                    ReadFile(handle, Some(&mut buf), Some(&mut bytes_read), None)
                };
                if result.is_err() || bytes_read == 0 {
                    log::info!(
                        "conpty reader: EOF after {total_bytes} bytes, err={:?}",
                        result.err()
                    );
                    break; // EOF or error; child exited.
                }
                total_bytes += bytes_read as u64;
                if let Some(started) = started {
                    let now = Instant::now();
                    let since_prev_ms = last_chunk_at
                        .map(|last| now.duration_since(last).as_secs_f64() * 1000.0)
                        .unwrap_or(0.0);
                    let since_start_ms = now.duration_since(started).as_secs_f64() * 1000.0;
                    log::info!(
                        target: "con::perf",
                        "conpty_read chunk={} bytes={} total_bytes={} since_prev_ms={:.3} since_start_ms={:.3}",
                        chunk_index,
                        bytes_read,
                        total_bytes,
                        since_prev_ms,
                        since_start_ms,
                    );
                    last_chunk_at = Some(now);
                }
                chunk_index = chunk_index.wrapping_add(1);
                log::trace!(
                    "conpty reader: +{bytes_read} bytes (total {total_bytes})"
                );
                on_output(&buf[..bytes_read as usize]);
            }
            // OwnedHandle::Drop closes the handle.
        })
        .expect("conpty reader thread spawn failed")
}

/// Discover a sensible default shell. Matches Windows Terminal's
/// default-profile selection:
///   1. `pwsh.exe`   (PowerShell 7+) if on PATH.
///   2. `powershell.exe` (Windows PowerShell) if on PATH.
///   3. `$env:COMSPEC` if set (usually `cmd.exe`).
///   4. `cmd.exe` (hardcoded last resort — always present).
///
/// Users who want to force a different shell can point us at it via
/// `CON_SHELL` (future: a config-file field). We prefer pwsh over
/// cmd because Windows Terminal does, and because pwsh is the modern
/// default on Windows 11 / Server 2025.
pub fn default_shell_command() -> String {
    if let Some(cmd) = std::env::var("CON_SHELL")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return cmd;
    }
    for candidate in ["pwsh.exe", "powershell.exe"] {
        if path_lookup(candidate).is_some() {
            return candidate.to_string();
        }
    }
    if let Some(cmd) = std::env::var("COMSPEC")
        .ok()
        .filter(|s| !s.trim().is_empty())
    {
        return cmd;
    }
    "cmd.exe".to_string()
}

fn path_lookup(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    for entry in std::env::split_paths(&path) {
        let candidate = entry.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
