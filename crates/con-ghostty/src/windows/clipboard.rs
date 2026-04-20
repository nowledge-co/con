//! Win32 clipboard helpers (CF_UNICODETEXT).
//!
//! The Win32 clipboard is a single global handle-table; opening it is
//! exclusive (other processes block) so we open → copy bytes → close as
//! quickly as possible. On failure we log and return `None` / `Err` — a
//! terminal pane losing access to the clipboard for one frame is
//! annoying but never fatal.
//!
//! We use `CF_UNICODETEXT` (UTF-16 LE, NUL-terminated) because it's the
//! canonical unicode format; cooperating apps translate to/from their
//! preferred encoding. `GlobalAlloc(GMEM_MOVEABLE)` is required by
//! `SetClipboardData` — the clipboard takes ownership of the handle
//! and frees it when replaced (so we must NOT free it on the success
//! path).

use anyhow::{Context, Result};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;

/// Read UTF-16 text from the clipboard and decode to a Rust `String`.
/// Returns `Ok(None)` when the clipboard holds no text (an image, no
/// data, etc.) — distinct from an error opening / accessing it.
pub fn get_text(owner: HWND) -> Result<Option<String>> {
    // SAFETY: OpenClipboard on a valid (possibly null) HWND; pairs with
    // CloseClipboard below.
    unsafe {
        OpenClipboard(Some(owner)).context("OpenClipboard failed")?;
    }

    let result = (|| -> Result<Option<String>> {
        // SAFETY: CF_UNICODETEXT is a stable Win32 constant.
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT.0 as u32) };
        let handle = match handle {
            Ok(h) if !h.is_invalid() => h,
            _ => return Ok(None), // clipboard empty or holds non-text
        };

        // GlobalLock returns a pointer to the UTF-16 buffer. It's
        // NUL-terminated; walk it to find the length.
        // SAFETY: handle is a valid HGLOBAL per GetClipboardData contract.
        let hglobal = HGLOBAL(handle.0);
        let ptr = unsafe { GlobalLock(hglobal) } as *const u16;
        if ptr.is_null() {
            anyhow::bail!("GlobalLock returned NULL");
        }

        // SAFETY: valid pointer to NUL-terminated UTF-16.
        let mut len = 0usize;
        unsafe {
            while *ptr.add(len) != 0 {
                len += 1;
            }
        }
        // SAFETY: slice covers `len` u16 units before the terminator.
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
        let text = String::from_utf16_lossy(slice);

        // SAFETY: match the GlobalLock above.
        unsafe {
            let _ = GlobalUnlock(hglobal);
        }

        Ok(Some(text))
    })();

    // SAFETY: matches the OpenClipboard.
    unsafe {
        let _ = CloseClipboard();
    }
    result
}

/// Write UTF-8 text to the clipboard as CF_UNICODETEXT.
pub fn set_text(owner: HWND, text: &str) -> Result<()> {
    let mut utf16: Vec<u16> = text.encode_utf16().collect();
    utf16.push(0);
    let byte_len = utf16.len() * std::mem::size_of::<u16>();

    // SAFETY: GlobalAlloc returns a moveable handle; we lock, copy the
    // UTF-16 in, unlock, then transfer ownership to the clipboard.
    let hglobal = unsafe { GlobalAlloc(GMEM_MOVEABLE, byte_len) }
        .context("GlobalAlloc for clipboard buffer failed")?;

    // SAFETY: freshly-allocated HGLOBAL; lock returns the base pointer.
    let ptr = unsafe { GlobalLock(hglobal) } as *mut u16;
    if ptr.is_null() {
        // SAFETY: allocated above; free to avoid leak.
        unsafe {
            let _ = GlobalFree(Some(hglobal));
        }
        anyhow::bail!("GlobalLock for clipboard buffer returned NULL");
    }
    // SAFETY: `utf16.len()` units inside the allocation.
    unsafe {
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr, utf16.len());
        let _ = GlobalUnlock(hglobal);
    }

    // SAFETY: OpenClipboard/CloseClipboard pair.
    unsafe {
        OpenClipboard(Some(owner)).context("OpenClipboard failed")?;
    }
    let result = (|| -> Result<()> {
        // SAFETY: clipboard opened; EmptyClipboard is the prerequisite
        // for SetClipboardData to succeed (the old handle, if any, is
        // freed by the OS).
        unsafe {
            EmptyClipboard().context("EmptyClipboard failed")?;
        }
        // SAFETY: SetClipboardData takes ownership of hglobal on success.
        // Wrap the handle in HANDLE (SetClipboardData takes HANDLE).
        let _ = unsafe {
            SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hglobal.0)))
                .context("SetClipboardData failed")?
        };
        Ok(())
    })();
    // SAFETY: always close.
    unsafe {
        let _ = CloseClipboard();
    }

    if result.is_err() {
        // SAFETY: we still own hglobal since SetClipboardData failed to
        // take ownership. Free to avoid leaking the alloc.
        unsafe {
            let _ = GlobalFree(Some(hglobal));
        }
    }
    result
}
