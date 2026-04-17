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
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result, anyhow};
use parking_lot::Mutex;
use windows::Win32::Foundation::{CloseHandle, HANDLE, TRUE};
use windows::Win32::Security::SECURITY_ATTRIBUTES;
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
use windows::Win32::System::Console::{
    COORD, ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, InitializeProcThreadAttributeList,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
    STARTUPINFOEXW, STARTUPINFOW, UpdateProcThreadAttribute,
};
use windows::core::PWSTR;

/// Newtype-wrapped HPCON so we can implement Drop / Send safely.
struct PseudoConsole(HPCON);

impl Drop for PseudoConsole {
    fn drop(&mut self) {
        // SAFETY: HPCON is owned by us; closing on drop is the documented
        // teardown contract. Blocks until the child has finished draining
        // its output pipe.
        unsafe { ClosePseudoConsole(self.0) }
    }
}

unsafe impl Send for PseudoConsole {}
unsafe impl Sync for PseudoConsole {}

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
    pcon: PseudoConsole,
    /// Host end of the pipe the child reads from.
    input_write: Arc<Mutex<OwnedHandle>>,
    /// Process handle (kept so callers can `WaitForSingleObject` for exit).
    process: OwnedHandle,
    /// Child thread handle.
    _thread: OwnedHandle,
    /// Output reader thread; joined on drop.
    output_thread: Option<JoinHandle<()>>,
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

        let pcon = PseudoConsole(hpcon);

        let (startup_info, attribute_buffer) = build_startup_info(hpcon)?;

        let mut command_line_w: Vec<u16> = OsString::from(command_line)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut process_info = PROCESS_INFORMATION::default();

        // CREATE_NO_WINDOW: don't attach the child to the parent's
        // console, and don't let it pop a new one. ConPTY still works —
        // the pseudo-console is the child's stdio, independent of the
        // OS-level console window. Without this flag, every pane spawns
        // a visible cmd.exe / pwsh window alongside con-app.
        let creation_flags =
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW;

        // SAFETY: command_line_w is mutable + NUL-terminated;
        // `attribute_buffer` keeps the attribute list alive until after
        // CreateProcessW returns.
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

        Ok(Self {
            pcon,
            input_write,
            process,
            _thread: thread,
            output_thread: Some(output_thread),
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
        // SAFETY: pcon.0 is a valid HPCON owned by us.
        unsafe { ResizePseudoConsole(self.pcon.0, coord) }
            .map_err(|e| anyhow!("ResizePseudoConsole failed: {}", e.message()))
    }

    pub fn process_handle(&self) -> HANDLE {
        self.process.as_handle()
    }
}

impl Drop for ConPty {
    fn drop(&mut self) {
        // PseudoConsole's Drop closes the PCON, which EOFs the output
        // pipe. The reader thread exits and we join it so the FnMut
        // closure isn't dropped on a live thread.
        if let Some(handle) = self.output_thread.take() {
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
    Ok((OwnedHandle::from_handle(read), OwnedHandle::from_handle(write)))
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

    // SAFETY: passing the HPCON value as the attribute. The lifetime of
    // the HPCON exceeds CreateProcessW (caller holds `pcon` until after).
    unsafe {
        UpdateProcThreadAttribute(
            attribute_list,
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            Some(&hpcon as *const HPCON as *const _),
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
    if let Some(cmd) = std::env::var("CON_SHELL").ok().filter(|s| !s.trim().is_empty()) {
        return cmd;
    }
    for candidate in ["pwsh.exe", "powershell.exe"] {
        if path_lookup(candidate).is_some() {
            return candidate.to_string();
        }
    }
    if let Some(cmd) = std::env::var("COMSPEC").ok().filter(|s| !s.trim().is_empty()) {
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
