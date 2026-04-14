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
    fn con_sparkle_check_for_updates(controller: *mut std::ffi::c_void);
}

/// Opaque handle to the Sparkle updater controller.
///
/// Stored globally so the ObjC runtime retains it for the process lifetime.
/// We never release this — Sparkle must stay alive for the entire app session.
static CONTROLLER: OnceLock<usize> = OnceLock::new();
static STATUS: OnceLock<UpdaterStatus> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdaterStatus {
    Active,
    Disabled(UpdaterDisabledReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdaterDisabledReason {
    ChannelDoesNotPoll,
    NotBundled,
    MissingFrameworksPath,
    MissingSparkleFramework,
    FailedToLoadSparkleFramework,
    MissingFeedUrl,
    ControllerInitFailed,
    InitPanicked,
}

impl UpdaterStatus {
    pub fn can_check_manually(self) -> bool {
        matches!(self, Self::Active)
    }

    pub fn summary(self) -> &'static str {
        match self {
            Self::Active => "Auto-update enabled",
            Self::Disabled(_) => "Auto-update unavailable",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Active => "Sparkle is loaded and polling this release channel.",
            Self::Disabled(UpdaterDisabledReason::ChannelDoesNotPoll) => {
                "Development builds do not poll for updates."
            }
            Self::Disabled(UpdaterDisabledReason::NotBundled) => {
                "Updates only work from the bundled app, not cargo run."
            }
            Self::Disabled(UpdaterDisabledReason::MissingFrameworksPath) => {
                "The app bundle has no Frameworks directory."
            }
            Self::Disabled(UpdaterDisabledReason::MissingSparkleFramework) => {
                "Sparkle.framework is not embedded in the app bundle."
            }
            Self::Disabled(UpdaterDisabledReason::FailedToLoadSparkleFramework) => {
                "Sparkle.framework exists but failed to load."
            }
            Self::Disabled(UpdaterDisabledReason::MissingFeedUrl) => {
                "SUFeedURL is missing from the app bundle metadata."
            }
            Self::Disabled(UpdaterDisabledReason::ControllerInitFailed) => {
                "Sparkle failed to initialize its updater controller."
            }
            Self::Disabled(UpdaterDisabledReason::InitPanicked) => {
                "Updater initialization panicked and was disabled."
            }
        }
    }
}

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
            let _ = STATUS.set(UpdaterStatus::Disabled(UpdaterDisabledReason::InitPanicked));
            false
        }
    }
}

fn init_inner() -> bool {
    if CONTROLLER.get().is_some() {
        let _ = STATUS.set(UpdaterStatus::Active);
        return true;
    }

    let channel = con_core::release_channel::current();
    if !channel.polls_for_updates() {
        log::info!("updater: channel={} — skipping Sparkle init", channel.name());
        let _ = STATUS.set(UpdaterStatus::Disabled(
            UpdaterDisabledReason::ChannelDoesNotPoll,
        ));
        return false;
    }

    unsafe {
        // Verify we're running inside an app bundle with Sparkle
        let main_bundle: id = msg_send![class!(NSBundle), mainBundle];
        if main_bundle == nil {
            log::warn!("updater: no main bundle — likely running outside .app");
            let _ = STATUS.set(UpdaterStatus::Disabled(UpdaterDisabledReason::NotBundled));
            return false;
        }

        let frameworks_path: id = msg_send![main_bundle, privateFrameworksPath];
        if frameworks_path == nil {
            log::warn!("updater: no Frameworks path");
            let _ = STATUS.set(UpdaterStatus::Disabled(
                UpdaterDisabledReason::MissingFrameworksPath,
            ));
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
            let _ = STATUS.set(UpdaterStatus::Disabled(
                UpdaterDisabledReason::MissingSparkleFramework,
            ));
            return false;
        }
        let mut load_error: id = nil;
        let loaded: BOOL = msg_send![sparkle_bundle, loadAndReturnError: &mut load_error];
        if loaded != YES {
            if load_error != nil {
                let localized_description: id = msg_send![load_error, localizedDescription];
                let localized_reason: id = msg_send![load_error, localizedFailureReason];

                let desc_cstr: *const std::os::raw::c_char =
                    msg_send![localized_description, UTF8String];
                let reason_cstr: *const std::os::raw::c_char =
                    msg_send![localized_reason, UTF8String];

                let description = if desc_cstr.is_null() {
                    "<unknown>"
                } else {
                    std::ffi::CStr::from_ptr(desc_cstr)
                        .to_str()
                        .unwrap_or("<invalid utf8>")
                };
                let reason = if reason_cstr.is_null() {
                    ""
                } else {
                    std::ffi::CStr::from_ptr(reason_cstr)
                        .to_str()
                        .unwrap_or("")
                };
                log::warn!(
                    "updater: failed to load Sparkle.framework: {} {}",
                    description,
                    reason
                );
            } else {
                log::warn!("updater: failed to load Sparkle.framework");
            }
            let _ = STATUS.set(UpdaterStatus::Disabled(
                UpdaterDisabledReason::FailedToLoadSparkleFramework,
            ));
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
            let _ = STATUS.set(UpdaterStatus::Disabled(UpdaterDisabledReason::MissingFeedUrl));
            return false;
        }

        // Create SPUStandardUpdaterController via the ObjC trampoline.
        // The trampoline uses initWithStartingUpdater:YES and wraps in @try/@catch.
        let controller = con_sparkle_init_controller();
        if controller.is_null() {
            log::warn!("updater: SPUStandardUpdaterController init failed or threw — auto-update disabled");
            let _ = STATUS.set(UpdaterStatus::Disabled(
                UpdaterDisabledReason::ControllerInitFailed,
            ));
            return false;
        }

        let _ = CONTROLLER.set(controller as usize);
        let _ = STATUS.set(UpdaterStatus::Active);

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

pub fn status() -> UpdaterStatus {
    *STATUS.get_or_init(|| {
        if CONTROLLER.get().is_some() {
            UpdaterStatus::Active
        } else {
            UpdaterStatus::Disabled(UpdaterDisabledReason::ChannelDoesNotPoll)
        }
    })
}
