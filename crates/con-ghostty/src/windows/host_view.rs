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

use anyhow::{Context, Result};
use parking_lot::Mutex;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, CreateWindowExW, DefWindowProcW, DestroyWindow, GWLP_USERDATA, GetWindowLongPtrW,
    HCURSOR, HICON, MoveWindow, RegisterClassExW, SW_SHOW, SetParent, SetWindowLongPtrW,
    ShowWindow, WINDOW_EX_STYLE, WM_CHAR, WM_DESTROY, WM_DPICHANGED, WM_KEYDOWN, WM_LBUTTONDOWN,
    WM_MOUSEWHEEL, WM_PAINT, WM_SIZE, WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
};

use super::conpty::{ConPty, PtySize};
use super::render::{Renderer, RendererConfig};
use super::vt::VtScreen;

const WINDOW_CLASS_NAME: windows::core::PCWSTR =
    windows::core::w!("ConWindowsTerminalHostView");

/// Per-HWND state, pointed to by `GWLP_USERDATA`. Boxed so the address
/// is stable across the WM_NCCREATE handoff.
struct HostState {
    renderer: Mutex<Renderer>,
    vt: Arc<VtScreen>,
    conpty: Arc<ConPty>,
    config: RendererConfig,
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

        // Build the renderer now that we have an HWND.
        let mut renderer_config = config.clone();
        renderer_config.initial_width = width;
        renderer_config.initial_height = height;
        log::info!("HostView: creating Renderer");
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

    /// Send a UTF-8 string to the child shell.
    pub fn write_input(&self, text: &str) {
        // SAFETY: state is valid until WM_DESTROY.
        let state = unsafe { &*self.state };
        let _ = state.conpty.write(text.as_bytes());
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
                // window; renderer is mutex-protected.
                let state = unsafe { &*state_ptr };
                {
                    let mut renderer = state.renderer.lock();
                    let _ = renderer.resize(width, height);
                }
                let renderer = state.renderer.lock();
                let (cols, rows) = renderer.grid_for_dimensions(&state.config);
                let (cell_w, cell_h) = (
                    (width / cols as u32).max(1),
                    (height / rows as u32).max(1),
                );
                drop(renderer);
                let _ = state.vt.resize(cols, rows, cell_w, cell_h);
                let _ = state.conpty.resize(PtySize { cols, rows });
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            // TODO(phase-3-dpi): rebuild glyph atlas at new DPI.
            // The new DPI is in HIWORD(wparam); the suggested rect is in lparam.
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
            // TODO(phase-3-keys): translate VK_* to xterm escape
            // sequences for arrows / F-keys / Home / End / etc.
            LRESULT(0)
        }
        WM_LBUTTONDOWN | WM_MOUSEWHEEL => {
            // TODO(phase-3-mouse): forward to mouse-tracking modes.
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
