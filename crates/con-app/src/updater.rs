//! Cross-platform auto-update surface.
//!
//! macOS uses Sparkle (loaded dynamically from the embedded
//! `Sparkle.framework` in the app bundle). All ObjC calls go through
//! a C trampoline (`sparkle_trampoline.m`) that wraps them in
//! `@try/@catch` — Rust's `catch_unwind` cannot catch ObjC exceptions.
//!
//! Windows uses a lightweight notify-only checker: on startup we
//! fetch the same Sparkle-shaped appcast XML and compare the latest
//! published version to the running binary. If newer we surface a
//! "download" link in Settings → Updates — no in-app download, no
//! exe replacement. Users grab the new ZIP and unpack it themselves.
//! Full auto-update is a follow-up.

// On Linux nothing consumes most of this module — the Updates card in
// settings_panel is cfg-gated to macOS/Windows. Keeping the surface
// compiled (rather than cfg-ing out every item) means main.rs can call
// `init()` unconditionally without #[cfg] noise.
#![cfg_attr(
    all(not(target_os = "macos"), not(target_os = "windows")),
    allow(dead_code)
)]

use std::sync::{Mutex, OnceLock};

#[cfg(target_os = "macos")]
use cocoa::base::{BOOL, YES, id, nil};
#[cfg(target_os = "macos")]
use objc::{class, msg_send, sel, sel_impl};

// FFI to the ObjC trampoline compiled by build.rs (macOS only).
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn con_sparkle_init_controller() -> *mut std::ffi::c_void;
    fn con_sparkle_check_for_updates(controller: *mut std::ffi::c_void);
}

/// Opaque handle to the Sparkle updater controller (macOS).
///
/// Stored globally so the ObjC runtime retains it for the process lifetime.
#[cfg(target_os = "macos")]
static CONTROLLER: OnceLock<usize> = OnceLock::new();
static STATUS: OnceLock<UpdaterStatus> = OnceLock::new();

/// Shared cross-platform "what we know about updates right now".
static LATEST: OnceLock<Mutex<CheckState>> = OnceLock::new();

fn latest_slot() -> &'static Mutex<CheckState> {
    LATEST.get_or_init(|| Mutex::new(CheckState::Idle))
}

/// Outcome of the last (or in-flight) update check.
///
/// Non-`Idle` variants are only constructed by the Windows
/// notify-only checker today. macOS delegates to Sparkle, which has
/// its own opaque state machine, so on that target the state stays
/// at `Idle` and the UI falls back to `UpdaterStatus::summary/detail`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub enum CheckState {
    /// Check has not been run yet, or the channel doesn't poll.
    Idle,
    /// A background fetch is currently in flight.
    Checking,
    /// Current binary is at or ahead of the latest published version.
    UpToDate,
    /// A newer version is published; user can follow the URL to grab it.
    UpdateAvailable { version: String, url: String },
    /// The last check failed; message is for the UI to display.
    Error(String),
}

pub fn latest_check() -> CheckState {
    latest_slot().lock().map(|g| g.clone()).unwrap_or(CheckState::Idle)
}

#[cfg(target_os = "windows")]
fn set_latest(state: CheckState) {
    if let Ok(mut g) = latest_slot().lock() {
        *g = state;
    }
}

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
            Self::Active => {
                if cfg!(target_os = "windows") {
                    "Periodic checks against the release feed; download installed manually."
                } else {
                    "Sparkle is loaded and polling this release channel."
                }
            }
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

/// Initialize the updater. Call once during app launch, after the
/// main window is open. Returns `true` if the update surface is live.
pub fn init() -> bool {
    match std::panic::catch_unwind(init_inner) {
        Ok(result) => result,
        Err(_) => {
            log::error!("updater: init panicked — auto-update disabled");
            let _ = STATUS.set(UpdaterStatus::Disabled(UpdaterDisabledReason::InitPanicked));
            false
        }
    }
}

#[cfg(target_os = "macos")]
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

        let controller = con_sparkle_init_controller();
        if controller.is_null() {
            log::warn!(
                "updater: SPUStandardUpdaterController init failed or threw — auto-update disabled"
            );
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

#[cfg(target_os = "windows")]
fn init_inner() -> bool {
    let channel = con_core::release_channel::current();
    if !channel.polls_for_updates() {
        log::info!(
            "updater: channel={} — notify-only updater idle",
            channel.name()
        );
        let _ = STATUS.set(UpdaterStatus::Disabled(
            UpdaterDisabledReason::ChannelDoesNotPoll,
        ));
        return false;
    }

    let _ = STATUS.set(UpdaterStatus::Active);
    windows_impl::spawn_check(channel);
    log::info!(
        "updater: notify-only check started — channel={}",
        channel.name()
    );
    true
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
fn init_inner() -> bool {
    let _ = STATUS.set(UpdaterStatus::Disabled(
        UpdaterDisabledReason::ChannelDoesNotPoll,
    ));
    false
}

/// Trigger a manual update check (e.g. from Settings → "Check for Updates").
#[cfg(target_os = "macos")]
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

#[cfg(target_os = "windows")]
pub fn check_for_updates() {
    let channel = con_core::release_channel::current();
    if !channel.polls_for_updates() {
        log::info!(
            "updater: channel={} — skipping manual check",
            channel.name()
        );
        return;
    }
    windows_impl::spawn_check(channel);
}

#[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
pub fn check_for_updates() {}

pub fn status() -> UpdaterStatus {
    *STATUS.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            if CONTROLLER.get().is_some() {
                UpdaterStatus::Active
            } else {
                UpdaterStatus::Disabled(UpdaterDisabledReason::ChannelDoesNotPoll)
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            UpdaterStatus::Disabled(UpdaterDisabledReason::ChannelDoesNotPoll)
        }
    })
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use super::{CheckState, set_latest};
    use con_core::release_channel::{self, ReleaseChannel};

    /// Spawn a one-shot check on a native thread. Uses a fresh tokio
    /// runtime for the HTTP fetch rather than relying on a shared
    /// app-level runtime — the check runs at most a few times per
    /// session so the overhead of building a runtime is fine, and it
    /// keeps this module fully self-contained.
    pub(super) fn spawn_check(channel: ReleaseChannel) {
        set_latest(CheckState::Checking);
        let url = channel.feed_url(release_channel::host_platform(), release_channel::host_arch());
        std::thread::spawn(move || {
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())
                .and_then(|rt| rt.block_on(async { fetch_and_compare(&url).await }));
            match result {
                Ok(state) => set_latest(state),
                Err(e) => {
                    log::warn!("updater: windows check failed: {e}");
                    set_latest(CheckState::Error(e));
                }
            }
        });
    }

    async fn fetch_and_compare(feed_url: &str) -> Result<CheckState, String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .map_err(|e| format!("http client: {e}"))?;
        let body = client
            .get(feed_url)
            .send()
            .await
            .map_err(|e| format!("fetch: {e}"))?
            .error_for_status()
            .map_err(|e| format!("status: {e}"))?
            .text()
            .await
            .map_err(|e| format!("read body: {e}"))?;

        let (version, url) = parse_latest(&body)
            .ok_or_else(|| "appcast missing shortVersionString or enclosure".to_string())?;

        let running = env!("CARGO_PKG_VERSION");
        if is_newer(&version, running) {
            Ok(CheckState::UpdateAvailable { version, url })
        } else {
            Ok(CheckState::UpToDate)
        }
    }

    /// Extract the first `<sparkle:shortVersionString>` and
    /// `<enclosure url="...">` from the feed. Sparkle appcasts list
    /// the newest item first, so we only need to parse the head of
    /// the document.
    fn parse_latest(xml: &str) -> Option<(String, String)> {
        let version = between(xml, "<sparkle:shortVersionString>", "</sparkle:shortVersionString>")?;
        let enclosure_start = xml.find("<enclosure")?;
        let enclosure_end = xml[enclosure_start..]
            .find('>')
            .map(|i| enclosure_start + i)?;
        let enclosure_tag = &xml[enclosure_start..=enclosure_end];
        let url = attr(enclosure_tag, "url")?;
        Some((version.trim().to_string(), url.to_string()))
    }

    fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
        let i = s.find(open)? + open.len();
        let j = s[i..].find(close)? + i;
        Some(&s[i..j])
    }

    fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
        let key = format!("{name}=\"");
        let i = tag.find(&key)? + key.len();
        let j = tag[i..].find('"')? + i;
        Some(&tag[i..j])
    }

    /// Compare two dotted versions numerically, ignoring any
    /// pre-release suffix (`-beta.1` etc). Returns true iff
    /// `latest > running`.
    fn is_newer(latest: &str, running: &str) -> bool {
        let parse = |v: &str| -> Vec<u64> {
            v.split(|c: char| c == '-' || c == '+')
                .next()
                .unwrap_or(v)
                .split('.')
                .map(|s| s.parse::<u64>().unwrap_or(0))
                .collect()
        };
        let l = parse(latest);
        let r = parse(running);
        let len = l.len().max(r.len());
        for i in 0..len {
            let a = l.get(i).copied().unwrap_or(0);
            let b = r.get(i).copied().unwrap_or(0);
            if a != b {
                return a > b;
            }
        }
        false
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_sparkle_feed() {
            let xml = r#"<?xml version="1.0"?>
            <rss xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
              <channel>
                <item>
                  <sparkle:shortVersionString>0.8.0</sparkle:shortVersionString>
                  <enclosure url="https://example.com/con-0.8.0.zip" length="12345" type="application/zip" sparkle:edSignature="abc"/>
                </item>
              </channel>
            </rss>"#;
            let (v, u) = parse_latest(xml).unwrap();
            assert_eq!(v, "0.8.0");
            assert_eq!(u, "https://example.com/con-0.8.0.zip");
        }

        #[test]
        fn version_comparison() {
            assert!(is_newer("0.8.0", "0.7.9"));
            assert!(is_newer("0.7.10", "0.7.9"));
            assert!(!is_newer("0.7.9", "0.7.9"));
            assert!(!is_newer("0.7.9", "0.8.0"));
            assert!(is_newer("1.0.0", "0.9.99"));
            // Pre-release suffixes are stripped; 0.8.0-beta.1 compares as 0.8.0.
            assert!(is_newer("0.8.0-beta.1", "0.7.9"));
        }
    }
}
