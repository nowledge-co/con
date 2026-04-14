//! macOS auto-updater powered by Sparkle.
//!
//! Sparkle is loaded dynamically from the embedded framework at
//! `Contents/Frameworks/Sparkle.framework`.  If the framework is
//! absent (e.g. `cargo run` dev builds), the updater silently
//! disables itself.
//!
//! Sparkle reads `SUFeedURL` and `SUPublicEDKey` from Info.plist,
//! which are injected by the release build scripts when
//! `CON_SPARKLE_FEED_URL` and `CON_SPARKLE_PUBLIC_ED_KEY` are set.

use cocoa::base::{BOOL, YES, id, nil};
use objc::{class, msg_send, sel, sel_impl};
use std::sync::OnceLock;

/// Opaque handle to the Sparkle updater controller.
///
/// Stored globally so the ObjC runtime retains it for the process lifetime.
static CONTROLLER: OnceLock<usize> = OnceLock::new();

/// Initialize the Sparkle updater.
///
/// Call once during app launch, after the main window is open.
/// Returns `true` if Sparkle was loaded and started successfully.
pub fn init() -> bool {
    if CONTROLLER.get().is_some() {
        return true;
    }

    let channel = con_core::release_channel::current();
    if !channel.polls_for_updates() {
        log::info!("updater: channel={} — skipping Sparkle init", channel.name());
        return false;
    }

    unsafe {
        // Locate Sparkle.framework inside the app bundle
        let main_bundle: id = msg_send![class!(NSBundle), mainBundle];
        if main_bundle == nil {
            log::warn!("updater: no main bundle — likely running outside .app");
            return false;
        }

        // Build path: <bundle>/Contents/Frameworks/Sparkle.framework
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

        // Load the framework bundle
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

        // Verify SUFeedURL is set (otherwise Sparkle will crash)
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

        // Create SPUStandardUpdaterController.
        // `initForStartingUpdater:updaterDelegate:userDriverDelegate:`
        //   startingUpdater: YES — start checking immediately
        //   updaterDelegate: nil — use Sparkle defaults
        //   userDriverDelegate: nil — use Sparkle's standard UI
        let controller_class = objc::runtime::Class::get("SPUStandardUpdaterController");
        let controller_class = match controller_class {
            Some(c) => c,
            None => {
                log::warn!("updater: SPUStandardUpdaterController class not found");
                return false;
            }
        };

        let alloc: id = msg_send![controller_class, alloc];
        let controller: id = msg_send![alloc,
            initForStartingUpdater: YES
            updaterDelegate: nil
            userDriverDelegate: nil
        ];

        if controller == nil {
            log::warn!("updater: failed to create SPUStandardUpdaterController");
            return false;
        }

        // alloc+init returns a +1 retained object.  We store the raw
        // pointer in a static and never release — the controller lives
        // for the entire process lifetime.
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
        Some(&ptr) => ptr as id,
        None => {
            log::info!("updater: not initialized — cannot check for updates");
            return;
        }
    };

    unsafe {
        let updater: id = msg_send![controller, updater];
        let _: () = msg_send![updater, checkForUpdates];
    }
}

/// Whether the updater is active (Sparkle was loaded and is polling).
pub fn is_active() -> bool {
    CONTROLLER.get().is_some()
}
