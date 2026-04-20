//! WS_CHILD HWND host for the terminal renderer.
//!
//! Each [`HostView`] owns a child window parented to GPUI's main HWND.
//! Lifetime sequence:
//!
//! 1. [`HostView::new`] registers a window class (idempotent), creates
//!    the child HWND, parents it via `SetParent`, and constructs a
//!    [`Renderer`] that draws into the HWND's swapchain.
//! 2. The window proc forwards `WM_*` messages back into our state:
//!    - `WM_SIZE` -> `Renderer::resize` + `ConPty::resize`
//!    - `WM_DPICHANGED` -> rebuild glyph atlas at new DPI
//!    - `WM_CHAR` / `WM_KEYDOWN` -> encode + write to ConPTY
//!    - `WM_LBUTTONDOWN` / `WM_MOUSEWHEEL` -> mouse forwarding
//!    - `WM_PAINT` -> trigger render
//! 3. [`HostView::reposition`] is called by the GPUI element on each
//!    layout to move the HWND inside the parent's coordinate space.
//! 4. Drop destroys the HWND.
//!
//! Thread model: HWND is created on the main thread (GPUI's). The
//! renderer state (`Renderer`) is owned by this struct and accessed
//! only from the main thread; the ConPTY reader runs on its own
//! thread and feeds bytes to the VT parser. The parser's `Mutex` is
//! the only synchronization point.

use std::cell::RefCell;
use std::ptr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use parking_lot::Mutex;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{HBRUSH, ScreenToClient};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, VK_C, VK_CONTROL, VK_DOWN, VK_END, VK_HOME, VK_LEFT,
    VK_RIGHT, VK_SHIFT, VK_UP, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW,
    HCURSOR, HICON, MA_NOACTIVATE, MoveWindow, RegisterClassExW, SW_SHOW, SetParent,
    SetWindowLongPtrW, ShowWindow, WINDOW_EX_STYLE, WM_CHAR, WM_DESTROY, WM_DPICHANGED,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_PAINT, WM_SIZE, WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
};

use super::clipboard;
use super::conpty::{ConPty, PtySize};
use super::render::{Renderer, RendererConfig, Selection};
use super::vt::{ScreenSnapshot, VtScreen};

const WINDOW_CLASS_NAME: windows::core::PCWSTR =
    windows::core::w!("ConWindowsTerminalHostView");

/// Per-HWND state, pointed to by `GWLP_USERDATA`. Boxed so the address
/// is stable across the WM_NCCREATE handoff.
struct HostState {
    renderer: Mutex<Renderer>,
    vt: Arc<VtScreen>,
    conpty: Arc<ConPty>,
    config: RendererConfig,
    /// Logical (96-dpi) font size; the effective physical size is
    /// `base_font_size_px * current_dpi / 96`. Preserved across
    /// `WM_DPICHANGED` so we rebuild the atlas from the same base.
    base_font_size_px: f32,
    current_dpi: u32,
    /// Cell under the cursor when LMB went down. `Some` only while
    /// the button is held. Cleared on WM_LBUTTONUP / focus loss.
    drag_anchor: Mutex<Option<(u16, u16)>>,
    /// Set by the WM_LBUTTONDOWN handler; drained by the GPUI side
    /// (`WindowsGhosttyTerminal::take_click_pending`) on each paint so
    /// it can move GPUI's focus to this pane when the user clicks
    /// inside the child HWND. Without this, focus stays wherever it
    /// was before (typically the input bar) and the terminal never
    /// becomes the "active" pane from the workspace's perspective.
    click_pending: AtomicBool,
}

/// A live terminal pane: child HWND + renderer + PTY + parser.
pub struct HostView {
    hwnd: HWND,
    state: *mut HostState, // Boxed; freed in WM_DESTROY.
}

unsafe impl Send for HostView {}

impl HostView {
    /// Create a child HWND inside `parent`, spawn a shell, and start
    /// rendering. `bounds` is the initial position+size in the parent's
    /// client coordinates.
    pub fn new(parent: HWND, bounds: RECT, config: RendererConfig) -> Result<Self> {
        log::info!(
            "HostView::new parent={:?} rect={}x{}",
            parent.0,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top
        );
        ensure_window_class()?;

        let width = (bounds.right - bounds.left).max(1) as u32;
        let height = (bounds.bottom - bounds.top).max(1) as u32;

        // SAFETY: this is the standard CreateWindowExW idiom. `parent`
        // must be valid; the new window is parented + WS_CHILD. The
        // renderer is constructed *after* CreateWindowExW returns,
        // since it needs the HWND.
        let hwnd: HWND = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                WINDOW_CLASS_NAME,
                windows::core::w!(""),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                Some(parent),
                None,
                None,
                Some(ptr::null()),
            )
        }
        .context("CreateWindowExW failed for terminal host view")?;

        // DPI: read from the child HWND (inherits from parent monitor).
        // Scale the logical font size to physical px so text appears at
        // the same on-screen size Windows Terminal shows on this
        // monitor. Fall back to 96 if GetDpiForWindow returns 0 (happens
        // on pre-Win10 or under screen-reader shims).
        // SAFETY: hwnd just returned from CreateWindowExW; still alive.
        let current_dpi = {
            let dpi = unsafe { GetDpiForWindow(hwnd) };
            if dpi == 0 { 96 } else { dpi }
        };
        let base_font_size_px = config.font_size_px;
        log::info!(
            "HostView::new dpi={current_dpi} base_font_size={base_font_size_px}"
        );

        // Build the renderer now that we have an HWND.
        let mut renderer_config = config.clone();
        renderer_config.initial_width = width;
        renderer_config.initial_height = height;
        renderer_config.font_size_px = scale_font_size(base_font_size_px, current_dpi);
        log::info!(
            "HostView: creating Renderer (physical font_size_px={})",
            renderer_config.font_size_px
        );
        let renderer = Renderer::new(hwnd, &renderer_config)?;
        log::info!("HostView: Renderer created");

        let (cols, rows) = renderer.grid_for_dimensions(&renderer_config);
        log::info!("HostView: grid {cols}x{rows}");

        log::info!("HostView: creating VtScreen");
        let vt = Arc::new(VtScreen::new(cols, rows)?);
        log::info!("HostView: VtScreen created");

        let vt_for_pty = vt.clone();
        let shell = super::conpty::default_shell_command();
        log::info!("HostView: spawning ConPTY shell={shell}");
        let conpty = ConPty::spawn(
            &shell,
            PtySize { cols, rows },
            move |bytes| vt_for_pty.feed(bytes),
        )?;
        let conpty = Arc::new(conpty);
        log::info!("HostView: ConPTY spawned");

        let state = Box::new(HostState {
            renderer: Mutex::new(renderer),
            vt,
            conpty,
            config: renderer_config,
            base_font_size_px,
            current_dpi,
            drag_anchor: Mutex::new(None),
            click_pending: AtomicBool::new(false),
        });
        let state_ptr = Box::into_raw(state);

        // SAFETY: we just allocated state_ptr; the window's USERDATA
        // takes ownership until WM_DESTROY frees it.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
            let _ = ShowWindow(hwnd, SW_SHOW);
        }

        Ok(Self {
            hwnd,
            state: state_ptr,
        })
    }

    /// Move + resize within the parent. Called from GPUI layout.
    pub fn reposition(&self, bounds: RECT) {
        // SAFETY: hwnd is valid until Drop.
        unsafe {
            let _ = MoveWindow(
                self.hwnd,
                bounds.left,
                bounds.top,
                bounds.right - bounds.left,
                bounds.bottom - bounds.top,
                true,
            );
        }
    }

    /// Render one frame right now. Called from GPUI's paint pass so
    /// the terminal surface doesn't rely on Windows-driven `WM_PAINT`
    /// (which only fires when the window is invalidated — something
    /// that doesn't happen for a child HWND whose class has no
    /// auto-paint).
    pub fn paint_frame(&self) {
        // SAFETY: state_ptr is valid for the lifetime of the HWND (freed
        // only in WM_DESTROY). We access it from the main thread just
        // like the WM_PAINT handler does.
        if self.state.is_null() {
            return;
        }
        unsafe {
            let state = &*self.state;
            let snapshot = state.vt.snapshot();
            let renderer = state.renderer.lock();
            if let Err(err) = renderer.render(&snapshot, &state.config) {
                log::warn!("renderer.render failed: {err:#}");
            }
        }
    }

    /// Re-parent to a different GPUI HWND (rare; happens on tab move).
    pub fn set_parent(&self, new_parent: HWND) {
        // SAFETY: hwnd is valid; SetParent may fire WM_PARENTNOTIFY.
        unsafe {
            let _ = SetParent(self.hwnd, Some(new_parent));
        }
    }

    /// HWND for callers that need raw access (focus assertions, debugging).
    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    /// `true` while the underlying ConPTY session is still live. Flips
    /// to `false` after the exit-watcher closes the pseudo-console
    /// (user typed `exit`, shell crashed, etc.) — consumed by the
    /// GPUI-side `GhosttyView` to emit `GhosttyProcessExited` and let
    /// the workspace auto-close the pane.
    pub fn is_alive(&self) -> bool {
        // SAFETY: state is valid until WM_DESTROY.
        let state = unsafe { &*self.state };
        state.conpty.is_alive()
    }

    /// Drain the "user clicked inside my HWND since last poll" flag.
    /// GPUI-side code calls this each frame and, when it returns `true`,
    /// moves GPUI's keyboard focus to this pane's `FocusHandle`. Needed
    /// because WM_LBUTTONDOWN is delivered directly to the child HWND —
    /// GPUI's parent HWND never sees the click and can't update focus
    /// on its own.
    pub fn take_click_pending(&self) -> bool {
        // SAFETY: state is valid until WM_DESTROY.
        let state = unsafe { &*self.state };
        state.click_pending.swap(false, Ordering::AcqRel)
    }

    /// Send a UTF-8 string to the child shell.
    pub fn write_input(&self, text: &str) {
        // SAFETY: state is valid until WM_DESTROY.
        let state = unsafe { &*self.state };
        // ConPTY simulates a keyboard: Enter on Windows keyboards
        // produces CR (\r), not LF (\n). The app-level input paths (e.g.
        // the command input bar in workspace.rs) append \n per Unix
        // convention; translate here so pwsh / cmd actually execute the
        // line. Without this, pwsh echoes the command but never runs it
        // because its line editor is still waiting for CR.
        let bytes: std::borrow::Cow<[u8]> = if text.as_bytes().contains(&b'\n') {
            std::borrow::Cow::Owned(text.replace('\n', "\r").into_bytes())
        } else {
            std::borrow::Cow::Borrowed(text.as_bytes())
        };
        let _ = state.conpty.write(&bytes);
    }
}

impl Drop for HostView {
    fn drop(&mut self) {
        // SAFETY: hwnd was created by us; DestroyWindow triggers
        // WM_DESTROY which frees the state Box.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

/// DPI-scale a logical (96-dpi) font size into physical pixels. Float
/// math: DirectWrite's CreateTextFormat takes a float size, so we don't
/// round to an integer — rounding to the nearest whole pixel causes
/// visible banding across the jump from 100%→125%→150% scaling.
fn scale_font_size(logical_px: f32, dpi: u32) -> f32 {
    logical_px * (dpi as f32) / 96.0
}

/// Single resize chain: `ResizeBuffers` → derive cell grid → forward
/// to the VT parser and the PTY. Each step returns a `Result`; `?`
/// bubbles the first failure so WM_SIZE can log precisely which link
/// broke. Returning `Err` from here doesn't abort the program — the
/// caller just logs and lets the next WM_SIZE try again.
fn apply_resize(state: &HostState, width: u32, height: u32) -> Result<()> {
    {
        let mut renderer = state.renderer.lock();
        renderer
            .resize(width, height)
            .context("Renderer::resize (ResizeBuffers)")?;
    }
    let (cols, rows) = {
        let renderer = state.renderer.lock();
        renderer.grid_for_dimensions(&state.config)
    };
    let cell_w = (width / cols.max(1) as u32).max(1);
    let cell_h = (height / rows.max(1) as u32).max(1);
    state
        .vt
        .resize(cols, rows, cell_w, cell_h)
        .context("VtScreen::resize")?;
    state
        .conpty
        .resize(PtySize { cols, rows })
        .context("ConPty::resize")?;
    Ok(())
}

/// Turn an `lparam` carrying client-space (x, y) coords into a `(col,
/// row)` cell address using the renderer's current cell metrics. Out-
/// of-window negative coords clamp to zero. Returns `None` if cell
/// metrics aren't known yet.
fn client_cell(state: &HostState, lparam: LPARAM) -> Option<(u16, u16)> {
    let x = (lparam.0 & 0xFFFF) as i16 as i32;
    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
    let renderer = state.renderer.lock();
    let m = renderer.metrics();
    drop(renderer);
    let cell_w = m.cell_width_px.max(1) as i32;
    let cell_h = m.cell_height_px.max(1) as i32;
    let col = (x.max(0) / cell_w).min(u16::MAX as i32) as u16;
    let row = (y.max(0) / cell_h).min(u16::MAX as i32) as u16;
    Some((col, row))
}

/// `GetKeyState` returns a SHORT: high bit = physically down, low bit
/// = toggled (e.g. caps lock). We care about the high bit for modifier
/// keys during WM_KEYDOWN handling.
fn is_key_down(vk: u16) -> bool {
    // SAFETY: GetKeyState is a pure read of the thread-local key state
    // table; safe for any VK.
    let state = unsafe { GetKeyState(vk as i32) };
    (state as u16 & 0x8000) != 0
}

/// Walk the current VT snapshot and emit the codepoints that fall
/// inside the current selection as a `String`. Row breaks are rendered
/// as '\n'. Returns `None` if there's no selection.
fn copy_selection(state: &HostState) -> Option<String> {
    let renderer = state.renderer.lock();
    let selection = renderer.selection()?;
    drop(renderer);
    let snapshot = state.vt.snapshot();
    Some(extract_selection_text(&snapshot, selection))
}

fn extract_selection_text(snapshot: &ScreenSnapshot, sel: Selection) -> String {
    let cols = snapshot.cols;
    if cols == 0 || snapshot.cells.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    let rows = snapshot.rows;
    for row in 0..rows {
        let mut row_buf = String::new();
        let mut row_has_cell = false;
        // Trailing empty cells are trimmed per-row so we don't emit
        // long runs of whitespace for a drag that ends at the right
        // margin. We detect "empty" via codepoint == 0 (blank cell).
        let mut last_non_blank: i32 = -1;
        for col in 0..cols {
            if !sel.contains(col, row, cols) {
                continue;
            }
            row_has_cell = true;
            let idx = row as usize * cols as usize + col as usize;
            let cell = snapshot.cells.get(idx).copied().unwrap_or_default();
            let ch = if cell.codepoint == 0 {
                ' '
            } else {
                char::from_u32(cell.codepoint).unwrap_or(' ')
            };
            row_buf.push(ch);
            if cell.codepoint != 0 && cell.codepoint != 0x20 {
                last_non_blank = row_buf.chars().count() as i32 - 1;
            }
        }
        if !row_has_cell {
            continue;
        }
        // Trim trailing whitespace on the row (xterm/Terminal behavior).
        if last_non_blank >= 0 {
            let trimmed: String = row_buf.chars().take(last_non_blank as usize + 1).collect();
            out.push_str(&trimmed);
        }
        out.push('\n');
    }
    // Drop the final '\n' — selections that end mid-line shouldn't get
    // a trailing newline tacked on. The caller can paste verbatim.
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Encode arrow / Home / End as an xterm escape sequence, respecting
/// DECCKM (application-cursor vs. cursor mode). Returns the byte
/// payload to write to the PTY, or `None` if `vk` isn't a cursor key.
fn encode_cursor_key(vk: u16, app_mode: bool) -> Option<&'static [u8]> {
    // Application-cursor uses the `ESC O X` form; default (cursor mode)
    // uses `ESC [ X`. Home / End don't move with DECCKM in xterm, but
    // we route them through the same helper for symmetry — both forms
    // are accepted by readline / vim.
    let (csi_form, ss3_form): (&[u8], &[u8]) = match vk {
        v if v == VK_UP.0 => (b"\x1b[A", b"\x1bOA"),
        v if v == VK_DOWN.0 => (b"\x1b[B", b"\x1bOB"),
        v if v == VK_RIGHT.0 => (b"\x1b[C", b"\x1bOC"),
        v if v == VK_LEFT.0 => (b"\x1b[D", b"\x1bOD"),
        v if v == VK_HOME.0 => (b"\x1b[H", b"\x1bOH"),
        v if v == VK_END.0 => (b"\x1b[F", b"\x1bOF"),
        _ => return None,
    };
    Some(if app_mode { ss3_form } else { csi_form })
}

/// Encode a mouse-wheel tick as an xterm SGR mouse report and write it
/// to the PTY. Button codes per xterm: 64 = wheel up, 65 = wheel down.
/// The report is `ESC [ < Button ; Col ; Row M` (M because wheel is a
/// press; there is no release). Col/row are 1-based cell coordinates
/// derived from the cursor client-area position.
fn forward_wheel(hwnd: HWND, state: &HostState, wparam: WPARAM, lparam: LPARAM) {
    // HIWORD(wparam) is a signed 16-bit wheel delta. One notch on a
    // standard mouse is `WHEEL_DELTA` (120); hi-res / trackpad sends
    // smaller fractional ticks. We only care about sign to pick the
    // button — coalescing the delta is left for a future refinement.
    let delta = ((wparam.0 >> 16) & 0xFFFF) as i16;
    if delta == 0 {
        return;
    }
    let button: u8 = if delta > 0 { 64 } else { 65 };

    // lparam is screen-space (x, y) for mouse wheel messages (unlike
    // WM_MOUSEMOVE / WM_LBUTTONDOWN which are client-space). Convert.
    let x_screen = (lparam.0 & 0xFFFF) as i16 as i32;
    let y_screen = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
    let mut pt = POINT { x: x_screen, y: y_screen };
    // SAFETY: hwnd is valid for the duration of the message pump.
    let ok = unsafe { ScreenToClient(hwnd, &mut pt) };
    if !ok.as_bool() {
        return;
    }

    let renderer = state.renderer.lock();
    let metrics = renderer.metrics();
    drop(renderer);
    let cell_w = metrics.cell_width_px.max(1) as i32;
    let cell_h = metrics.cell_height_px.max(1) as i32;
    // 1-based cell coords per SGR mouse spec; clamp to 1 so the
    // terminal app never sees a 0 (some apps treat 0 as "unknown").
    let col = (pt.x.max(0) / cell_w + 1).min(u16::MAX as i32) as u16;
    let row = (pt.y.max(0) / cell_h + 1).min(u16::MAX as i32) as u16;

    let seq = format!("\x1b[<{button};{col};{row}M");
    let _ = state.conpty.write(seq.as_bytes());
}

fn ensure_window_class() -> Result<()> {
    thread_local! {
        static REGISTERED: RefCell<bool> = const { RefCell::new(false) };
    }
    let mut already = false;
    REGISTERED.with(|cell| {
        if *cell.borrow() {
            already = true;
        } else {
            *cell.borrow_mut() = true;
        }
    });
    if already {
        return Ok(());
    }

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_DBLCLKS,
        lpfnWndProc: Some(window_proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: HINSTANCE::default(),
        hIcon: HICON::default(),
        hCursor: HCURSOR::default(),
        hbrBackground: HBRUSH::default(),
        lpszMenuName: windows::core::PCWSTR(ptr::null()),
        lpszClassName: WINDOW_CLASS_NAME,
        hIconSm: HICON::default(),
    };
    // SAFETY: wc is a valid stack-local; RegisterClassExW returns 0 only
    // if the class is already registered for this hInstance — benign,
    // CreateWindowExW will succeed anyway.
    unsafe {
        RegisterClassExW(&wc);
    }
    Ok(())
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Pull the boxed state pointer back out of GWLP_USERDATA.
    // SAFETY: pointer was set in HostView::new and remains valid until
    // WM_DESTROY frees it.
    let state_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut HostState;

    match msg {
        WM_DESTROY => {
            if !state_ptr.is_null() {
                // SAFETY: we set USERDATA to a Box::into_raw pointer and
                // the window is being destroyed.
                unsafe {
                    drop(Box::from_raw(state_ptr));
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
            }
            LRESULT(0)
        }
        WM_PAINT => {
            if !state_ptr.is_null() {
                // SAFETY: state_ptr is valid; renderer mutex is fine to
                // lock from the message thread.
                let state = unsafe { &*state_ptr };
                let snapshot = state.vt.snapshot();
                let renderer = state.renderer.lock();
                let _ = renderer.render(&snapshot, &state.config);
            }
            // Mark the client area validated — without this, the OS
            // considers the HWND perpetually dirty and keeps calling
            // WM_PAINT in a tight loop (and can refuse to composite
            // the backbuffer until the dirty region is cleared).
            // SAFETY: hwnd is valid for the duration of the message.
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::ValidateRect(Some(hwnd), None);
            }
            LRESULT(0)
        }
        WM_SIZE => {
            if !state_ptr.is_null() {
                let width = (lparam.0 & 0xFFFF) as u32;
                let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
                // SAFETY: state_ptr is valid for the lifetime of the
                // window; the resize chain uses interior-mutex access
                // on the renderer and owned Arcs for vt / conpty.
                let state = unsafe { &*state_ptr };
                if let Err(err) = apply_resize(state, width, height) {
                    log::warn!(
                        "WM_SIZE resize chain failed ({width}x{height}): {err:#}"
                    );
                }
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            if !state_ptr.is_null() {
                let new_dpi = ((wparam.0 >> 16) & 0xFFFF) as u32;
                // lparam points at a RECT* in screen coords that the
                // system suggests we move/resize to (so the window
                // preserves its on-screen size under the new scale).
                // SAFETY: Windows guarantees lparam is a valid RECT*
                // for WM_DPICHANGED.
                if lparam.0 != 0 {
                    let suggested = unsafe { &*(lparam.0 as *const RECT) };
                    unsafe {
                        let _ = MoveWindow(
                            hwnd,
                            suggested.left,
                            suggested.top,
                            suggested.right - suggested.left,
                            suggested.bottom - suggested.top,
                            true,
                        );
                    }
                }
                // SAFETY: state_ptr valid.
                let state = unsafe { &mut *state_ptr };
                state.current_dpi = new_dpi.max(1);
                let new_font = scale_font_size(state.base_font_size_px, state.current_dpi);
                state.config.font_size_px = new_font;
                {
                    let renderer = state.renderer.lock();
                    if let Err(err) = renderer.rebuild_atlas(new_font) {
                        log::warn!("rebuild_atlas on DPI change failed: {err:#}");
                    }
                }
                // Re-derive the grid from the new metrics; WM_SIZE will
                // also fire but MoveWindow dispatch order isn't
                // guaranteed, so do it here too to be robust.
                let renderer = state.renderer.lock();
                let (w, h) = renderer.dimensions_px();
                let (cols, rows) = renderer.grid_for_dimensions(&state.config);
                let (cell_w, cell_h) = (
                    (w / cols as u32).max(1),
                    (h / rows as u32).max(1),
                );
                drop(renderer);
                let _ = state.vt.resize(cols, rows, cell_w, cell_h);
                let _ = state.conpty.resize(PtySize { cols, rows });
                log::info!(
                    "WM_DPICHANGED: dpi={new_dpi} font_px={new_font:.2} grid={cols}x{rows}"
                );
            }
            LRESULT(0)
        }
        WM_CHAR => {
            // wparam is a UTF-16 code unit. Surrogate pairs arrive as
            // two consecutive WM_CHAR messages; the renderer cares
            // about codepoints but ConPTY accepts UTF-8 raw, so we
            // re-encode here.
            if !state_ptr.is_null() {
                let unit = (wparam.0 & 0xFFFF) as u16;
                if let Some(ch) = char::decode_utf16(std::iter::once(unit))
                    .next()
                    .and_then(|r| r.ok())
                {
                    let mut buf = [0u8; 4];
                    let s = ch.encode_utf8(&mut buf);
                    // SAFETY: state_ptr is valid.
                    let state = unsafe { &*state_ptr };
                    let _ = state.conpty.write(s.as_bytes());
                }
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            // Ctrl+Shift+C / Ctrl+Shift+V: copy-selection / paste. We
            // need to intercept here (not in WM_CHAR) because Ctrl-
            // modified keys don't produce a WM_CHAR for plain letters,
            // and even if they did we'd want Ctrl+Shift+C to NOT send
            // the Ctrl+C (^C / SIGINT) byte. VK_C = 0x43, VK_V = 0x56.
            if !state_ptr.is_null() {
                let vk = (wparam.0 & 0xFFFF) as u16;
                let ctrl = is_key_down(VK_CONTROL.0);
                let shift = is_key_down(VK_SHIFT.0);
                if ctrl && shift {
                    // SAFETY: state_ptr valid.
                    let state = unsafe { &*state_ptr };
                    if vk == VK_C.0 {
                        if let Some(text) = copy_selection(state) {
                            if let Err(err) = clipboard::set_text(hwnd, &text) {
                                log::warn!("clipboard::set_text failed: {err:#}");
                            }
                        }
                        return LRESULT(0);
                    } else if vk == VK_V.0 {
                        match clipboard::get_text(hwnd) {
                            Ok(Some(text)) if !text.is_empty() => {
                                // ConPTY is a virtual keyboard: convert
                                // \r\n (Windows) and bare \n (Unix-origin
                                // clipboard payloads) into \r so the
                                // shell actually executes multi-line
                                // paste line-by-line.
                                let normalized =
                                    text.replace("\r\n", "\r").replace('\n', "\r");
                                // Bracketed paste: when the shell has
                                // asked for it (DEC 2004), wrap the
                                // payload in ESC[200~ … ESC[201~ so
                                // it can disable line-editing hooks
                                // for the duration. Safe to interleave
                                // with the normalized CR translation —
                                // the markers are plain bytes.
                                if state.vt.is_bracketed_paste() {
                                    let _ = state.conpty.write(b"\x1b[200~");
                                    let _ = state.conpty.write(normalized.as_bytes());
                                    let _ = state.conpty.write(b"\x1b[201~");
                                } else {
                                    let _ = state.conpty.write(normalized.as_bytes());
                                }
                            }
                            Ok(_) => {}
                            Err(err) => log::warn!("clipboard::get_text failed: {err:#}"),
                        }
                        return LRESULT(0);
                    }
                }
                // Arrow / Home / End. These never produce a WM_CHAR,
                // so WM_KEYDOWN is the only place we see them. DECCKM
                // toggles between CSI (ESC[X) and SS3 (ESCOx) forms —
                // readline / vim / less flip it on entry.
                let vk = (wparam.0 & 0xFFFF) as u16;
                // SAFETY: state_ptr valid.
                let state = unsafe { &*state_ptr };
                let app_mode = state.vt.is_decckm();
                if let Some(bytes) = encode_cursor_key(vk, app_mode) {
                    let _ = state.conpty.write(bytes);
                    return LRESULT(0);
                }
            }
            // TODO(phase-3-keys): translate F-keys and Page Up / Down
            // (VK_F1..F24, VK_PRIOR, VK_NEXT) to xterm escape sequences.
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => {
            // Default behavior for WM_MOUSEACTIVATE on a child HWND is
            // MA_ACTIVATE, which has Windows SetFocus() the child —
            // stealing keyboard focus from GPUI's parent HWND. Once
            // focus is on the child, WM_KEYDOWN / WM_CHAR go here
            // instead of to GPUI, so GPUI-level action bindings
            // (Ctrl+L / Toggle Agent Panel, Ctrl+T / New Tab, etc.)
            // silently stop firing. Returning MA_NOACTIVATE tells
            // Windows to process the click *without* moving focus, so
            // GPUI stays in the focus chain and keeps dispatching keys.
            // We still get WM_LBUTTONDOWN for selection / click-to-
            // focus tracking.
            LRESULT(MA_NOACTIVATE as isize)
        }
        WM_LBUTTONDOWN => {
            if !state_ptr.is_null() {
                // SAFETY: state_ptr valid.
                let state = unsafe { &*state_ptr };
                // Signal the GPUI-side view that this pane was clicked
                // so it can move focus to this pane's FocusHandle on
                // the next paint. MA_NOACTIVATE kept Windows focus on
                // GPUI's parent HWND; we still need GPUI's *logical*
                // focus to move to the clicked pane.
                state.click_pending.store(true, Ordering::Release);
                if let Some(cell) = client_cell(state, lparam) {
                    *state.drag_anchor.lock() = Some(cell);
                    // Starting a new drag clears any prior selection;
                    // the extent catches up on the first mouse move.
                    let renderer = state.renderer.lock();
                    renderer.set_selection(Some(Selection {
                        anchor: cell,
                        extent: cell,
                    }));
                    drop(renderer);
                    // Capture so mouse release outside the HWND still
                    // delivers WM_LBUTTONUP and we don't get stuck in
                    // "dragging" state.
                    // SAFETY: SetCapture is safe on any HWND we own.
                    unsafe {
                        let _ = SetCapture(hwnd);
                    }
                }
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            if !state_ptr.is_null() {
                // SAFETY: state_ptr valid.
                let state = unsafe { &*state_ptr };
                // Snapshot the anchor under a single lock. Reading via
                // `is_some()` + `.unwrap()` would race: in principle the
                // field can only change on the same message thread we're
                // on, but the `Option::unwrap()` is still a latent panic
                // site if WM_LBUTTONUP ever executes between them (e.g.
                // a future scheduler quirk or re-entrant DispatchMessage).
                let anchor = *state.drag_anchor.lock();
                if let Some(anchor) = anchor {
                    if let Some(extent) = client_cell(state, lparam) {
                        let renderer = state.renderer.lock();
                        renderer.set_selection(Some(Selection { anchor, extent }));
                    }
                }
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if !state_ptr.is_null() {
                // SAFETY: state_ptr valid.
                let state = unsafe { &*state_ptr };
                let anchor = state.drag_anchor.lock().take();
                // SAFETY: ReleaseCapture is a no-op if we don't hold it.
                unsafe {
                    let _ = ReleaseCapture();
                }
                // Click-without-drag (anchor == current cell) → clear
                // the transient selection so an isolated click doesn't
                // leave a 1-cell highlight behind.
                if let (Some(anchor), Some(extent)) = (anchor, client_cell(state, lparam)) {
                    if anchor == extent {
                        let renderer = state.renderer.lock();
                        renderer.set_selection(None);
                    }
                }
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            if !state_ptr.is_null() {
                // SAFETY: state_ptr valid until WM_DESTROY.
                let state = unsafe { &*state_ptr };
                if state.vt.mouse_tracking_active() {
                    forward_wheel(hwnd, state, wparam, lparam);
                }
                // TODO(scrollback-ui): when not tracking, forward wheel
                // to a scrollback viewport once we add one. For now we
                // silently drop — matches Windows Terminal's behavior
                // when scrollback lines == 0.
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
