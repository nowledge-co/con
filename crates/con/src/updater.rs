//! macOS auto-updater powered by Sparkle.
//!
//! Sparkle is loaded dynamically from the embedded framework at
//! `Contents/Frameworks/Sparkle.framework`.  If the framework is
//! absent (e.g. `cargo run` dev builds), the updater silently
//! disables itself.
//!
//! All Sparkle ObjC calls go through a C trampoline
//! (`sparkle_trampoline.m`) that wraps them in `@try/@catch`.
//! Rust's `catch_unwind` cannot catch ObjC exceptions — without the
//! trampoline, any ObjC exception during Sparkle init would
//! propagate as `__rust_foreign_exception` → SIGABRT.

use cocoa::base::{BOOL, YES, id, nil};
use objc::{class, msg_send, sel, sel_impl};
use std::sync::OnceLock;

// FFI to the ObjC trampoline compiled by build.rs
unsafe extern "C" {
    fn con_sparkle_init_controller() -> *mut std::ffi::c_void;
    fn con_sparkle_start_updater(controller: *mut std::ffi::c_void) -> i32;
    fn con_sparkle_check_for_updates(controller: *mut std::ffi::c_void);
}

/// Opaque handle to the Sparkle updater controller.
///
/// Stored globally so the ObjC runtime retains it for the process lifetime.
/// We never release this — Sparkle must stay alive for the entire app session.
static CONTROLLER: OnceLock<usize> = OnceLock::new();

/// Initialize the Sparkle updater.
///
/// Call once during app launch, after the main window is open.
/// Returns `true` if Sparkle was loaded and started successfully.
///
/// Sparkle init and start are wrapped in ObjC `@try/@catch` so that
/// any exception from the framework is logged and swallowed rather
/// than crashing the app.
pub fn init() -> bool {
    match std::panic::catch_unwind(init_inner) {
        Ok(result) => result,
        Err(_) => {
            log::error!("updater: Sparkle init panicked — auto-update disabled");
            false
        }
    }
}

fn init_inner() -> bool {
    if CONTROLLER.get().is_some() {
        return true;
    }

    let channel = con_core::release_channel::current();
    if !channel.polls_for_updates() {
        log::info!("updater: channel={} — skipping Sparkle init", channel.name());
        return false;
    }

    unsafe {
        // Verify we're running inside an app bundle with Sparkle
        let main_bundle: id = msg_send![class!(NSBundle), mainBundle];
        if main_bundle == nil {
            log::warn!("updater: no main bundle — likely running outside .app");
            return false;
        }

        let frameworks_path: id = msg_send![main_bundle, privateFrameworksPath];
        if frameworks_path == nil {
            log::warn!("updater: no Frameworks path");
            return false;
        }
        let sparkle_subpath: id = msg_send![
            class!(NSString),
            stringWithUTF8String: b"Sparkle.framework\0".as_ptr()
        ];
        let sparkle_path: id =
            msg_send![frameworks_path, stringByAppendingPathComponent: sparkle_subpath];

        let sparkle_bundle: id = msg_send![class!(NSBundle), bundleWithPath: sparkle_path];
        if sparkle_bundle == nil {
            log::info!("updater: Sparkle.framework not found — auto-update disabled");
            return false;
        }
        let loaded: BOOL = msg_send![sparkle_bundle, load];
        if loaded != YES {
            log::warn!("updater: failed to load Sparkle.framework");
            return false;
        }

        // Verify SUFeedURL is set (otherwise Sparkle will throw)
        let info_dict: id = msg_send![main_bundle, infoDictionary];
        let feed_key: id = msg_send![
            class!(NSString),
            stringWithUTF8String: b"SUFeedURL\0".as_ptr()
        ];
        let feed_url: id = msg_send![info_dict, objectForKey: feed_key];
        if feed_url == nil {
            log::info!("updater: SUFeedURL not set in Info.plist — auto-update disabled");
            return false;
        }

        // Create SPUStandardUpdaterController via the ObjC trampoline.
        // The trampoline uses initForStartingUpdater:NO and wraps in @try/@catch.
        let controller = con_sparkle_init_controller();
        if controller.is_null() {
            log::warn!("updater: SPUStandardUpdaterController init failed or threw — auto-update disabled");
            return false;
        }

        // Start the updater (begins automatic checking).
        // Also wrapped in @try/@catch.
        let started = con_sparkle_start_updater(controller);
        if started == 0 {
            log::warn!("updater: startUpdater failed — auto-update disabled");
            return false;
        }

        let _ = CONTROLLER.set(controller as usize);

        log::info!(
            "updater: Sparkle initialized — channel={}, polling=true",
            channel.name()
        );
        true
    }
}

/// Trigger a manual update check (e.g. from Settings → "Check for Updates").
pub fn check_for_updates() {
    let controller = match CONTROLLER.get() {
        Some(&ptr) => ptr as *mut std::ffi::c_void,
        None => {
            log::info!("updater: not initialized — cannot check for updates");
            return;
        }
    };

    unsafe {
        con_sparkle_check_for_updates(controller);
    }
}

/// Whether the updater is active (Sparkle was loaded and is polling).
pub fn is_active() -> bool {
    CONTROLLER.get().is_some()
}
