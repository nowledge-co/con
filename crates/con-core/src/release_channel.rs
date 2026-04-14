//! Release channel detection and update-feed URL derivation.
//!
//! The channel is determined once at startup from the app bundle's
//! `Info.plist` (macOS) or a compile-time default, and is immutable
//! for the lifetime of the process.
//!
//! This module is intentionally platform-agnostic in its public API
//! so that Linux/Windows updaters can reuse `ReleaseChannel` and
//! `feed_url()` without depending on macOS-specific code.

use std::fmt;
use std::sync::OnceLock;

/// The global channel, set once at startup.
static CHANNEL: OnceLock<ReleaseChannel> = OnceLock::new();

/// Release channels supported by the update system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReleaseChannel {
    /// Local development build (`cargo run`). Never polls for updates.
    Dev,
    /// Pre-release builds distributed to testers.
    Beta,
    /// General-availability builds.
    Stable,
}

impl ReleaseChannel {
    /// Human-readable display name for UI.
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Dev => "con Dev",
            Self::Beta => "con Beta",
            Self::Stable => "con",
        }
    }

    /// Short machine identifier (used in feed URLs and config keys).
    pub fn name(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Beta => "beta",
            Self::Stable => "stable",
        }
    }

    /// Whether this channel should poll for updates.
    pub fn polls_for_updates(self) -> bool {
        match self {
            Self::Dev => false,
            Self::Beta | Self::Stable => true,
        }
    }

    /// Sparkle appcast feed URL for this channel and architecture.
    ///
    /// URL scheme:
    ///   `https://con-releases.nowledge.co/appcast/{channel}-{platform}-{arch}.xml`
    ///
    /// This is stable across releases.  The CI pipeline publishes
    /// updated appcasts to the corresponding GitHub Pages path.
    pub fn feed_url(self, arch: &str) -> String {
        format!(
            "https://con-releases.nowledge.co/appcast/{channel}-macos-{arch}.xml",
            channel = self.name(),
            arch = arch,
        )
    }

    /// Parse from the `ConReleaseChannel` value baked into Info.plist.
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "beta" => Self::Beta,
            "stable" => Self::Stable,
            _ => Self::Dev,
        }
    }
}

impl fmt::Display for ReleaseChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

// ---------------------------------------------------------------------------
// Platform-specific detection
// ---------------------------------------------------------------------------

/// Read `ConReleaseChannel` from the main bundle's Info.plist.
#[cfg(target_os = "macos")]
fn detect_channel() -> ReleaseChannel {
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CStr;

    unsafe {
        let bundle: *mut objc::runtime::Object = msg_send![class!(NSBundle), mainBundle];
        if bundle.is_null() {
            return ReleaseChannel::Dev;
        }

        let info: *mut objc::runtime::Object = msg_send![bundle, infoDictionary];
        if info.is_null() {
            return ReleaseChannel::Dev;
        }

        let key: *mut objc::runtime::Object =
            msg_send![class!(NSString), stringWithUTF8String: b"ConReleaseChannel\0".as_ptr()];
        let value: *mut objc::runtime::Object = msg_send![info, objectForKey: key];
        if value.is_null() {
            return ReleaseChannel::Dev;
        }

        let utf8: *const std::os::raw::c_char = msg_send![value, UTF8String];
        if utf8.is_null() {
            return ReleaseChannel::Dev;
        }

        let channel_str = CStr::from_ptr(utf8).to_str().unwrap_or("dev");
        ReleaseChannel::from_str(channel_str)
    }
}

#[cfg(not(target_os = "macos"))]
fn detect_channel() -> ReleaseChannel {
    match std::env::var("CON_RELEASE_CHANNEL")
        .as_deref()
        .unwrap_or("dev")
    {
        "beta" => ReleaseChannel::Beta,
        "stable" => ReleaseChannel::Stable,
        _ => ReleaseChannel::Dev,
    }
}

/// Detect the host architecture at runtime.
#[cfg(target_os = "macos")]
pub fn host_arch() -> &'static str {
    #[cfg(target_arch = "aarch64")]
    {
        "arm64"
    }
    #[cfg(target_arch = "x86_64")]
    {
        "x86_64"
    }
}

#[cfg(not(target_os = "macos"))]
pub fn host_arch() -> &'static str {
    // Use the same names as the macOS convention so feed URLs are
    // consistent across platforms.
    #[cfg(target_arch = "x86_64")]
    {
        "x86_64"
    }
    #[cfg(target_arch = "aarch64")]
    {
        "arm64"
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the global release channel. Call once at startup.
pub fn init() -> ReleaseChannel {
    *CHANNEL.get_or_init(detect_channel)
}

/// Get the current release channel. Panics if `init()` was not called.
pub fn current() -> ReleaseChannel {
    *CHANNEL
        .get()
        .expect("release_channel::init() must be called before current()")
}
