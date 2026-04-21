// Release builds on Windows use the GUI subsystem so double-clicking
// the exe (or launching it from Explorer / a shortcut) doesn't spawn a
// stray console window alongside the GPUI window. Debug builds keep
// the console subsystem so `cargo wrun` in a terminal still prints
// env_logger output and panic traces inline.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]
// Suppress warnings from objc 0.2's `sel_impl!` and `class!` macros
// checking `cfg(feature = "cargo-clippy")`.
#![allow(unexpected_cfgs)]

mod agent_panel;
mod assets;
mod chat_markdown;
mod command_palette;
#[cfg(target_os = "macos")]
mod global_hotkey;

// The terminal-view module is selected per platform:
//   macOS   -> ghostty_view.rs (libghostty + child NSView)
//   Windows -> windows_view.rs (libghostty-vt + ConPTY + D3D11 child HWND)
//   Linux   -> linux_view.rs (Linux-specific placeholder until backend lands)
//   other   -> stub_view.rs
//
// All three expose the same public type names (`GhosttyView`,
// `GhosttyTitleChanged`, `GhosttyProcessExited`, `GhosttyFocusChanged`,
// `GhosttySplitRequested`, `init`) so downstream modules
// (`terminal_pane`, `workspace`) compile on every target without
// per-callsite cfg gates. See `docs/impl/windows-port.md` and
// `docs/impl/linux-port.md`.
#[cfg(target_os = "macos")]
mod ghostty_view;
#[cfg(target_os = "windows")]
#[path = "windows_view.rs"]
mod ghostty_view;
#[cfg(target_os = "linux")]
#[path = "linux_view.rs"]
mod ghostty_view;
#[cfg(all(
    not(target_os = "macos"),
    not(target_os = "windows"),
    not(target_os = "linux")
))]
#[path = "stub_view.rs"]
mod ghostty_view;

mod input_bar;
mod keycaps;
mod model_registry;
mod motion;
mod pane_tree;
mod settings_panel;
mod sidebar;
mod terminal_pane;
mod theme;
mod updater;
mod workspace;

use con_core::config::KeybindingConfig;
use con_core::session::{GlobalHistoryState, Session};
use gpui::*;
use gpui_component::ActiveTheme;
#[cfg(target_os = "macos")]
use gpui_component::link::Link;
use workspace::ConWorkspace;

actions!(
    con,
    [
        ShowAbout,
        Quit,
        NewWindow,
        NewTab,
        NextTab,
        PreviousTab,
        ToggleSummon,
        ToggleAgentPanel,
        ToggleInputBar,
        CloseTab,
        SplitRight,
        SplitDown,
        FocusInput,
        CycleInputMode,
        TogglePaneScopePicker,
        CheckForUpdates,
        HideApp,
        HideOtherApps,
        ShowAllApps,
        Undo,
        Redo,
        Cut,
        Copy,
        Paste,
        SelectAll
    ]
);

/// Set the macOS dock icon at runtime (for `cargo run` — bundled apps use Info.plist).
#[cfg(target_os = "macos")]
fn set_dock_icon() {
    use cocoa::appkit::{NSApp, NSApplication, NSImage};
    use cocoa::base::nil;
    use cocoa::foundation::NSData;
    use objc::rc::autoreleasepool;

    const ICON_PNG: &[u8] = include_bytes!("../../../assets/Con-macOS-Dark-256x256@2x.png");

    autoreleasepool(|| unsafe {
        let data = NSData::dataWithBytes_length_(
            nil,
            ICON_PNG.as_ptr() as *const std::ffi::c_void,
            ICON_PNG.len() as u64,
        );
        let icon = NSImage::initWithData_(NSImage::alloc(nil), data);
        NSApp().setApplicationIconImage_(icon);
    });
}

#[cfg(not(target_os = "macos"))]
fn set_dock_icon() {}

#[cfg(target_os = "macos")]
fn supports_transparent_main_window() -> bool {
    use cocoa::base::nil;
    use objc::{class, msg_send, sel, sel_impl};

    #[repr(C)]
    struct NSOperatingSystemVersion {
        major_version: isize,
        minor_version: isize,
        patch_version: isize,
    }

    unsafe {
        let process_info: cocoa::base::id = msg_send![class!(NSProcessInfo), processInfo];
        if process_info == nil {
            return true;
        }
        let version: NSOperatingSystemVersion = msg_send![process_info, operatingSystemVersion];
        version.major_version >= 13
    }
}

#[cfg(not(target_os = "macos"))]
#[cfg(target_os = "windows")]
fn supports_transparent_main_window() -> bool {
    true
}

#[cfg(target_os = "linux")]
fn supports_transparent_main_window() -> bool {
    false
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn supports_transparent_main_window() -> bool {
    false
}

/// Enable a Windows 11 DWM backdrop on the top-level window.
///
/// `blur=true` selects `DWMSBT_TRANSIENTWINDOW` (Acrylic, real Gaussian
/// blur of what's behind the window). `blur=false` selects
/// `DWMSBT_MAINWINDOW` (Mica, a static desktop-image-tinted fill — no
/// blur, much cheaper). Both render only when the GPUI Root above is
/// transparent.
///
/// Returns `true` when the DWM attribute was accepted — we use that
/// signal to keep the GPUI Root transparent so the backdrop shows
/// through. On Windows 10 / older Win11 / RDP / Arm32 / any host where
/// DwmSetWindowAttribute rejects `DWMWA_SYSTEMBACKDROP_TYPE`, we fall
/// back to an opaque theme fill in `open_con_window` (otherwise the
/// compositor shows the desktop through the window, which is what the
/// user observed when a modal hid the pane HWND).
#[cfg(target_os = "windows")]
fn apply_windows_backdrop(window: &mut Window, blur: bool) -> bool {
    set_windows_backdrop(window, blur).is_some()
}

/// Live re-apply of the DWM backdrop type. Called from the workspace
/// theme-update path so toggling "background blur" in settings switches
/// between Acrylic and Mica without restarting the window.
#[cfg(target_os = "windows")]
pub fn set_windows_backdrop_blur(window: &mut Window, blur: bool) {
    let _ = set_windows_backdrop(window, blur);
}

#[cfg(target_os = "windows")]
fn set_windows_backdrop(window: &mut Window, blur: bool) -> Option<()> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMSBT_MAINWINDOW, DWMSBT_TRANSIENTWINDOW,
        DWMWA_SYSTEMBACKDROP_TYPE,
    };

    let handle = HasWindowHandle::window_handle(window).ok()?;
    let hwnd = match handle.as_raw() {
        RawWindowHandle::Win32(h) => HWND(h.hwnd.get() as *mut std::ffi::c_void),
        _ => return None,
    };

    // NB: we deliberately do NOT call DwmExtendFrameIntoClientArea
    // here. That call is the documented path for legacy GDI / WPF
    // windows and it works well on a Win32-composed window, but GPUI's
    // top-level window is composited through DirectComposition. Mixing
    // the two made the whole window render opaque (see
    // Screenshot 2026-04-22 at 13.14.09). Acrylic/Mica with a DComp
    // client requires a `DCompositionCreateBackdropBrush` visual in
    // GPUI's tree; until that lands upstream, we keep the attribute
    // set so the non-client strip reflects the user's choice and the
    // terminal stays transparent.
    // SAFETY: hwnd is a live top-level window (we just received it
    // from GPUI's window-handle surface); DWMWA_SYSTEMBACKDROP_TYPE
    // takes a 4-byte integer by pointer.
    let try_apply = |value: i32, label: &'static str| -> windows::core::Result<()> {
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &value as *const i32 as *const std::ffi::c_void,
                std::mem::size_of::<i32>() as u32,
            )
        }
        .map(|()| log::info!("Windows: applied DWM {label} backdrop on main window"))
    };

    if blur {
        // Try Acrylic first; if the OS rejects it (older Win11 builds,
        // disabled effects), fall back to Mica so the window stays
        // transparent instead of going opaque.
        if try_apply(DWMSBT_TRANSIENTWINDOW.0, "Acrylic").is_ok() {
            return Some(());
        }
        log::info!("Windows: Acrylic backdrop unavailable; trying Mica fallback");
    }
    match try_apply(DWMSBT_MAINWINDOW.0, "Mica") {
        Ok(()) => Some(()),
        Err(err) => {
            log::info!(
                "Windows: Mica backdrop unavailable ({err:?}); \
                 falling back to opaque theme fill"
            );
            None
        }
    }
}

// On non-macOS targets, con now branches by platform: Windows has the
// shipped local backend, Linux has an in-progress local PTY + VT path,
// and any remaining targets still fall back to `stub_view.rs`. See the
// platform docs under `docs/impl/`.

fn default_window_options(cx: &mut App) -> WindowOptions {
    let transparent = supports_transparent_main_window();
    WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1200.0), px(800.0)), cx)),
        titlebar: default_titlebar_options(transparent),
        window_background: if transparent {
            WindowBackgroundAppearance::Transparent
        } else {
            WindowBackgroundAppearance::Opaque
        },
        ..Default::default()
    }
}
fn default_titlebar_options(transparent: bool) -> Option<TitlebarOptions> {
    Some(TitlebarOptions {
        title: Some("con".into()),
        appears_transparent: transparent,
        ..Default::default()
    })
}

fn open_con_window(config: con_core::Config, session: Session, exit_on_error: bool, cx: &mut App) {
    let window_options = default_window_options(cx);
    cx.spawn(async move |cx| {
        if let Err(err) = cx.open_window(window_options, |window, cx| {
            let restored_session = session.clone();
            let view = cx
                .new(|cx| ConWorkspace::from_session(config.clone(), restored_session, window, cx));
            #[cfg(target_os = "windows")]
            let mica_applied = apply_windows_backdrop(window, config.appearance.terminal_blur);
            #[cfg(not(target_os = "windows"))]
            let mica_applied = false;
            cx.new(|cx| {
                // On Windows we want the DWM-provided backdrop (Mica)
                // to shine through wherever the app isn't painting —
                // most importantly, the terminal area when the child
                // pane HWND is hidden behind a modal. That requires a
                // transparent Root. If Mica isn't available (Win10,
                // RDP, older Win11), fall back to an opaque theme fill
                // so the window doesn't look "see-through to desktop".
                let background = if cfg!(target_os = "windows") {
                    if mica_applied {
                        cx.theme().transparent
                    } else {
                        cx.theme().background
                    }
                } else if supports_transparent_main_window() {
                    cx.theme().transparent
                } else {
                    cx.theme().background
                };
                gpui_component::Root::new(view, window, cx).bg(background)
            })
        }) {
            if exit_on_error {
                eprintln!("Fatal: failed to open window: {err}");
                std::process::exit(1);
            } else {
                log::error!("Failed to open window: {err}");
            }
        }
    })
    .detach();
}

fn fresh_window_session_with_history() -> Session {
    let persisted = Session::load().unwrap_or_default();
    let persisted_history = GlobalHistoryState::load().unwrap_or_default();
    let mut session = Session::default();

    session.global_shell_history = if persisted_history.global_shell_history.is_empty() {
        persisted.global_shell_history
    } else {
        persisted_history.global_shell_history
    };
    session.input_history = if !persisted_history.input_history.is_empty() {
        persisted_history
            .input_history
            .into_iter()
            .filter_map(|entry| {
                let command = entry.trim();
                (!command.is_empty()).then(|| command.to_string())
            })
            .collect()
    } else if persisted.input_history.is_empty() {
        session
            .global_shell_history
            .iter()
            .filter_map(|entry| {
                let command = entry.command.trim();
                (!command.is_empty()).then(|| command.to_string())
            })
            .collect()
    } else {
        persisted
            .input_history
            .into_iter()
            .filter_map(|entry| {
                let command = entry.trim();
                (!command.is_empty()).then(|| command.to_string())
            })
            .collect()
    };

    session
}

pub(crate) fn toggle_global_summon(cx: &mut App) {
    let frontmost_window = cx.window_stack().and_then(|windows| windows.last().cloned());
    let has_windows = frontmost_window.is_some();

    #[cfg(target_os = "macos")]
    let app_is_active = global_hotkey::is_app_active();
    #[cfg(not(target_os = "macos"))]
    let app_is_active = cx.active_window().is_some();

    if !has_windows {
        let config = con_core::Config::load().unwrap_or_default();
        open_con_window(config, fresh_window_session_with_history(), false, cx);
        cx.activate(true);
        return;
    }

    if app_is_active {
        cx.hide();
    } else {
        cx.activate(true);
        if let Some(window_handle) = frontmost_window {
            let _ = cx.update_window(window_handle, |_, window, _| {
                window.activate_window();
            });
        }
    }
}

#[cfg(target_os = "macos")]
fn bundle_info_value(key: &'static [u8]) -> Option<String> {
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CStr;

    unsafe {
        let bundle: *mut objc::runtime::Object = msg_send![class!(NSBundle), mainBundle];
        if bundle.is_null() {
            return None;
        }
        let info: *mut objc::runtime::Object = msg_send![bundle, infoDictionary];
        if info.is_null() {
            return None;
        }
        let key_ns: *mut objc::runtime::Object =
            msg_send![class!(NSString), stringWithUTF8String: key.as_ptr()];
        if key_ns.is_null() {
            return None;
        }
        let value: *mut objc::runtime::Object = msg_send![info, objectForKey: key_ns];
        if value.is_null() {
            return None;
        }
        let utf8: *const std::os::raw::c_char = msg_send![value, UTF8String];
        if utf8.is_null() {
            return None;
        }
        CStr::from_ptr(utf8).to_str().ok().map(ToOwned::to_owned)
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn app_display_version() -> String {
    #[cfg(target_os = "macos")]
    let version = bundle_info_value(b"CFBundleShortVersionString\0")
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    // Non-macOS builds pick up the full tag-derived version (e.g.
    // `0.1.0-beta.31`) from `CON_RELEASE_VERSION`, which the release
    // pipeline exports before `cargo build`. Cargo.toml is frozen at
    // `0.1.0` so `CARGO_PKG_VERSION` alone loses the prerelease suffix.
    #[cfg(not(target_os = "macos"))]
    let version = option_env!("CON_RELEASE_VERSION")
        .map(str::to_owned)
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    version
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn app_build_number() -> String {
    #[cfg(target_os = "macos")]
    let build =
        bundle_info_value(b"CFBundleVersion\0").unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    #[cfg(not(target_os = "macos"))]
    let build = option_env!("CON_RELEASE_VERSION")
        .map(str::to_owned)
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    build
}

// About-panel helpers and window: only wired in to the menu on macOS
// (Sparkle lives in the same menu, and the Cocoa "About con" gesture
// hits this path). The functions stay platform-neutral so they're
// trivially reusable when a cross-platform About window lands.
#[cfg(target_os = "macos")]
fn about_panel_name() -> String {
    #[cfg(target_os = "macos")]
    {
        bundle_info_value(b"CFBundleDisplayName\0")
            .or_else(|| bundle_info_value(b"CFBundleName\0"))
            .unwrap_or_else(|| "con".to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        "con".to_string()
    }
}

#[cfg(target_os = "macos")]
fn about_panel_version_detail() -> String {
    let version = app_display_version();
    let build = app_build_number();
    let channel = con_core::release_channel::current().display_name();

    if build.is_empty() || build == version {
        match con_core::release_channel::current() {
            con_core::release_channel::ReleaseChannel::Dev => "Development Build".to_string(),
            _ => channel.to_string(),
        }
    } else {
        format!("Build {build} • {channel}")
    }
}

#[cfg(target_os = "macos")]
struct AboutView {
    app_name: String,
    version: String,
    version_detail: String,
}

#[cfg(target_os = "macos")]
impl Render for AboutView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let repo_url = "https://github.com/nowledge-co/con-terminal";
        let detail_surface = theme.secondary_active.opacity(0.34);
        let quiet_text = theme.foreground.opacity(0.64);
        let muted_text = theme.foreground.opacity(0.48);

        div()
            .size_full()
            .bg(theme.background)
            .px(px(28.0))
            .py(px(28.0))
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(14.0))
                    .child(
                        div()
                            .size(px(88.0))
                            .p(px(2.0))
                            .child(
                                img("Con-macOS-Dark-256x256@2x.png")
                                    .size_full()
                                    .object_fit(ObjectFit::Contain),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .max_w(px(310.0))
                            .gap(px(5.0))
                            .child(
                                div()
                                    .text_size(px(26.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.app_name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(14.0))
                                    .line_height(relative(1.35))
                                    .text_align(TextAlign::Center)
                                    .text_color(quiet_text)
                                    .child("A GPU-accelerated terminal with AI harness."),
                            ),
                    )
                    .child(
                        div()
                            .px(px(12.0))
                            .py(px(7.0))
                            .rounded(px(999.0))
                            .bg(detail_surface)
                            .font_family(theme.mono_font_family.clone())
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.foreground.opacity(0.8))
                            .child(format!("{} • {}", self.version, self.version_detail)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(6.0))
                            .child(
                                Link::new("about-repo")
                                    .href(repo_url)
                                    .text_size(px(12.5))
                                    .font_family(theme.mono_font_family.clone())
                                    .child(repo_url),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(muted_text)
                            .child("Copyright © 2026 Nowledge Labs, LLC"),
                    ),
            )
    }
}

#[cfg(target_os = "macos")]
fn show_about_window(cx: &mut App) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(432.0), px(388.0)), cx)),
        titlebar: Some(TitlebarOptions {
            title: Some(format!("About {}", about_panel_name()).into()),
            appears_transparent: true,
            ..Default::default()
        }),
        window_background: WindowBackgroundAppearance::Opaque,
        ..Default::default()
    };

    let app_name = about_panel_name();
    let version = app_display_version();
    let version_detail = about_panel_version_detail();

    if let Err(err) = cx.open_window(options, move |window, cx| {
        let about = cx.new(|_| AboutView {
            app_name: app_name.clone(),
            version: version.clone(),
            version_detail: version_detail.clone(),
        });
        cx.new(|cx| gpui_component::Root::new(about, window, cx).bg(cx.theme().background))
    }) {
        log::error!("Failed to open About window: {err}");
    }
}

pub(crate) fn bind_app_keybindings(cx: &mut App, kb: &KeybindingConfig) {
    cx.bind_keys([
        KeyBinding::new(&kb.quit, Quit, None),
        KeyBinding::new(&kb.new_window, NewWindow, None),
        KeyBinding::new(&kb.new_tab, NewTab, None),
        KeyBinding::new(&kb.next_tab, NextTab, None),
        KeyBinding::new(&kb.previous_tab, PreviousTab, None),
        KeyBinding::new("secondary-shift-]", NextTab, None),
        KeyBinding::new("secondary-shift-[", PreviousTab, None),
        KeyBinding::new(&kb.toggle_agent, ToggleAgentPanel, None),
        KeyBinding::new(&kb.close_tab, CloseTab, None),
        KeyBinding::new(&kb.settings, settings_panel::ToggleSettings, None),
        KeyBinding::new(
            &kb.command_palette,
            command_palette::ToggleCommandPalette,
            None,
        ),
        KeyBinding::new(&kb.split_right, SplitRight, None),
        KeyBinding::new(&kb.split_down, SplitDown, None),
        KeyBinding::new(&kb.focus_input, FocusInput, None),
        KeyBinding::new(&kb.cycle_input_mode, CycleInputMode, None),
        KeyBinding::new(&kb.toggle_input_bar, ToggleInputBar, None),
        KeyBinding::new(&kb.toggle_pane_scope, TogglePaneScopePicker, None),
        KeyBinding::new(&kb.quit, Quit, Some("Input")),
        KeyBinding::new(&kb.new_window, NewWindow, Some("Input")),
        KeyBinding::new(&kb.new_tab, NewTab, Some("Input")),
        KeyBinding::new(&kb.next_tab, NextTab, Some("Input")),
        KeyBinding::new(&kb.previous_tab, PreviousTab, Some("Input")),
        KeyBinding::new("secondary-shift-]", NextTab, Some("Input")),
        KeyBinding::new("secondary-shift-[", PreviousTab, Some("Input")),
        KeyBinding::new(&kb.toggle_agent, ToggleAgentPanel, Some("Input")),
        KeyBinding::new(&kb.close_tab, CloseTab, Some("Input")),
        KeyBinding::new(&kb.settings, settings_panel::ToggleSettings, Some("Input")),
        KeyBinding::new(
            &kb.command_palette,
            command_palette::ToggleCommandPalette,
            Some("Input"),
        ),
        KeyBinding::new(&kb.split_right, SplitRight, Some("Input")),
        KeyBinding::new(&kb.split_down, SplitDown, Some("Input")),
        KeyBinding::new(&kb.focus_input, FocusInput, Some("Input")),
        KeyBinding::new(&kb.cycle_input_mode, CycleInputMode, Some("Input")),
        KeyBinding::new(&kb.toggle_input_bar, ToggleInputBar, Some("Input")),
        KeyBinding::new(&kb.toggle_pane_scope, TogglePaneScopePicker, Some("Input")),
    ]);

    // Hide app / Hide others / Show all are macOS system-menu conventions
    // with no equivalent on Windows or Linux — cmd-h, cmd-alt-h, and
    // cmd-alt-shift-h are the canonical modifiers, so we keep them
    // verbatim inside a cfg gate rather than routing through `secondary`.
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-h", HideApp, None),
        KeyBinding::new("cmd-alt-h", HideOtherApps, None),
        KeyBinding::new("cmd-alt-shift-h", ShowAllApps, None),
        KeyBinding::new("cmd-h", HideApp, Some("Input")),
        KeyBinding::new("cmd-alt-h", HideOtherApps, Some("Input")),
        KeyBinding::new("cmd-alt-shift-h", ShowAllApps, Some("Input")),
    ]);
}

/// Install a panic hook that writes every panic (including from
/// background threads) to both stderr and a log file in the platform
/// temp dir. On Windows specifically, a GUI-launched exe has no
/// console by default and the default panic output is lost — this
/// guarantees we always have a record to read after a silent exit.
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>");
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".into());
        let payload = payload_as_str(info.payload()).unwrap_or("<non-string payload>");

        let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let record = format!(
            "[{timestamp}] PANIC on thread '{thread_name}' at {location}:\n  \
             {payload}\n\nBacktrace:\n{backtrace}\n\n"
        );

        // Always try stderr first.
        let _ = {
            use std::io::Write;
            let mut stderr = std::io::stderr().lock();
            let _ = stderr.write_all(record.as_bytes());
            stderr.flush()
        };

        // Then persist to a file — this is the one the user can actually
        // read after the process exits.
        let log_path = std::env::temp_dir().join("con-panic.log");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            use std::io::Write;
            let _ = f.write_all(record.as_bytes());
            let _ = f.flush();
            eprintln!("[panic log] {}", log_path.display());
        }

        // Chain to the previous hook so cargo / test harnesses still
        // see what they expect.
        previous(info);
    }));
}

fn payload_as_str(payload: &(dyn std::any::Any + Send)) -> Option<&str> {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        Some(*s)
    } else if let Some(s) = payload.downcast_ref::<String>() {
        Some(s.as_str())
    } else {
        None
    }
}

/// Install a Win32 vectored exception handler. Catches C-level faults
/// (EXCEPTION_ACCESS_VIOLATION, EXCEPTION_STACK_OVERFLOW, illegal
/// instruction, etc.) that bypass Rust's panic machinery — common when
/// an FFI library (libghostty-vt, a GPU driver) crashes inside its
/// own code. We log the exception code + address and the last breadcrumb
/// visible in stderr, then let the exception propagate so the process
/// still terminates.
///
/// Vectored handlers run *before* structured exception handling (SEH),
/// so ours fires even if the FFI library has its own `__try`/`__except`.
#[cfg(target_os = "windows")]
fn install_seh_filter() {
    use windows::Win32::System::Diagnostics::Debug::{
        AddVectoredExceptionHandler, EXCEPTION_POINTERS,
    };

    extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
        // SAFETY: `info` is provided by the OS during an exception
        // dispatch; it points at a valid EXCEPTION_POINTERS structure.
        // We only read a few scalar fields and write to stderr /
        // a file; no heap allocation is done from this handler.
        let (code, address) = unsafe {
            if info.is_null() {
                (0u32, std::ptr::null_mut())
            } else {
                let er = (*info).ExceptionRecord;
                if er.is_null() {
                    (0, std::ptr::null_mut())
                } else {
                    ((*er).ExceptionCode.0 as u32, (*er).ExceptionAddress)
                }
            }
        };

        // Only log "interesting" exceptions — many Windows internals
        // throw soft exceptions (DLL not found probes, debugger
        // breaks) that we shouldn't report. The ones below are the
        // real crashes.
        let interesting = matches!(
            code,
            0xC0000005  // ACCESS_VIOLATION
            | 0xC00000FD  // STACK_OVERFLOW
            | 0xC000001D  // ILLEGAL_INSTRUCTION
            | 0xC0000094  // INT_DIVIDE_BY_ZERO
            | 0xC0000096  // PRIV_INSTRUCTION
            | 0xC000013A  // CONTROL_C_EXIT
        );

        if interesting {
            let record = format!(
                "[SEH] Exception {:#x} at {:p} — likely C/FFI crash. \
                 The most recent log line above is the last thing that \
                 ran before the fault.\n",
                code, address
            );
            use std::io::Write;
            let _ = std::io::stderr().lock().write_all(record.as_bytes());
            let log_path = std::env::temp_dir().join("con-panic.log");
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&log_path)
            {
                let _ = f.write_all(record.as_bytes());
            }
        }

        // EXCEPTION_CONTINUE_SEARCH = 0: let normal SEH unwinding run.
        0
    }

    // SAFETY: registering a handler is always safe; the handler fn is
    // `extern "system"` with the correct signature.
    unsafe {
        let _ = AddVectoredExceptionHandler(1, Some(handler));
    }
}

fn main() {
    // Install a panic hook before anything else so every panic —
    // including ones from background threads that would otherwise be
    // invisible — gets written to both stderr and a dated log file.
    // Critical on Windows where double-clicking the exe detaches
    // stderr and a panic produces a silent exit.
    install_panic_hook();

    // Catch C-level access violations / stack overflows / etc. from
    // FFI libraries (libghostty-vt, GPU driver shims) that bypass
    // Rust's panic infrastructure entirely.
    #[cfg(target_os = "windows")]
    install_seh_filter();

    // Always capture backtraces unless the user already set a
    // preference. `full` prints symbols + line numbers when debug info
    // is present; harmless in release.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        // SAFETY: single-threaded during startup, standard env-var set.
        unsafe { std::env::set_var("RUST_BACKTRACE", "full") };
    }

    // Default to `info` for our crates if the user didn't set RUST_LOG,
    // so early-init traces are visible without an explicit opt-in.
    let mut builder = env_logger::Builder::from_default_env();
    if std::env::var_os("RUST_LOG").is_none() {
        builder.filter_level(log::LevelFilter::Info);
    }
    builder.init();

    log::info!("con starting (pid {})", std::process::id());

    let config = con_core::Config::load().unwrap_or_default();
    log::info!("config loaded");

    let app = gpui_platform::application()
        .with_quit_mode(QuitMode::Explicit)
        .with_assets(assets::ConAssets);
    log::info!("gpui application created");
    app.on_reopen(|cx| {
        let has_windows = cx
            .window_stack()
            .map(|windows| !windows.is_empty())
            .unwrap_or(false);
        if has_windows {
            cx.activate(true);
            return;
        }

        let config = con_core::Config::load().unwrap_or_default();
        open_con_window(config, fresh_window_session_with_history(), false, cx);
        cx.activate(true);
    });
    app.run(move |cx: &mut App| {
        // Detect release channel (stable/beta/dev) from bundle Info.plist
        let channel = con_core::release_channel::init();
        log::info!("Release channel: {}", channel.display_name());

        // Set dock icon for development (cargo run)
        set_dock_icon();

        // Initialize gpui-component subsystems (theme, input, dialog, etc.)
        gpui_component::init(cx);
        input_bar::InputBar::init(cx);

        // Load and activate con's design theme (synced to terminal theme)
        theme::init_theme(
            cx,
            &config.terminal.theme,
            &config.terminal.font_family,
            &config.appearance.ui_font_family,
            config.appearance.ui_font_size,
        );

        // Register ghostty terminal key bindings (Tab interception, etc.)
        // The stub impl is a no-op but we call it unconditionally so the
        // code path is shape-identical across platforms.
        ghostty_view::init(cx);

        // Register global keybindings from user config
        bind_app_keybindings(cx, &config.keybindings);
        #[cfg(target_os = "macos")]
        global_hotkey::init(cx, &config.keybindings);

        cx.on_action(|_: &NewWindow, cx: &mut App| {
            let config = con_core::Config::load().unwrap_or_default();
            open_con_window(config, fresh_window_session_with_history(), false, cx);
        });
        cx.on_action(|_: &ToggleSummon, cx: &mut App| {
            toggle_global_summon(cx);
        });
        cx.on_action(|_: &NewTab, cx: &mut App| {
            if cx.active_window().is_none() {
                let config = con_core::Config::load().unwrap_or_default();
                open_con_window(config, fresh_window_session_with_history(), false, cx);
            }
        });

        #[cfg(target_os = "macos")]
        cx.on_action(|_: &ShowAbout, cx: &mut App| {
            show_about_window(cx);
        });

        cx.on_action(|_: &CheckForUpdates, _cx: &mut App| {
            updater::check_for_updates();
        });
        cx.on_action(|_: &HideApp, cx: &mut App| {
            cx.hide();
        });
        cx.on_action(|_: &HideOtherApps, cx: &mut App| {
            cx.hide_other_apps();
        });
        cx.on_action(|_: &ShowAllApps, cx: &mut App| {
            cx.unhide_other_apps();
        });

        cx.set_menus(vec![
            Menu {
                name: "con".into(),
                items: vec![
                    MenuItem::action("About con", ShowAbout),
                    MenuItem::separator(),
                    MenuItem::action("Check for Updates…", CheckForUpdates),
                    MenuItem::separator(),
                    MenuItem::action("Settings…", settings_panel::ToggleSettings),
                    MenuItem::separator(),
                    MenuItem::action("Hide con", HideApp),
                    MenuItem::action("Hide Others", HideOtherApps),
                    MenuItem::action("Show All", ShowAllApps),
                    MenuItem::separator(),
                    MenuItem::action("Quit con", Quit),
                ],
                disabled: false,
            },
            Menu {
                name: "File".into(),
                items: vec![
                    MenuItem::action("New Window", NewWindow),
                    MenuItem::action("New Tab", NewTab),
                    MenuItem::action("Close Tab", CloseTab),
                    MenuItem::separator(),
                    MenuItem::action("Split Right", SplitRight),
                    MenuItem::action("Split Down", SplitDown),
                ],
                disabled: false,
            },
            Menu {
                name: "Edit".into(),
                items: vec![
                    MenuItem::os_action("Undo", Undo, OsAction::Undo),
                    MenuItem::os_action("Redo", Redo, OsAction::Redo),
                    MenuItem::separator(),
                    MenuItem::os_action("Cut", Cut, OsAction::Cut),
                    MenuItem::os_action("Copy", Copy, OsAction::Copy),
                    MenuItem::os_action("Paste", Paste, OsAction::Paste),
                    MenuItem::separator(),
                    MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
                ],
                disabled: false,
            },
            Menu {
                name: "View".into(),
                items: vec![
                    MenuItem::action("Toggle Agent Panel", ToggleAgentPanel),
                    MenuItem::action("Toggle Input Bar", ToggleInputBar),
                    MenuItem::action("Command Palette", command_palette::ToggleCommandPalette),
                    MenuItem::separator(),
                    MenuItem::action("Focus Input", FocusInput),
                    MenuItem::action("Cycle Input Mode", CycleInputMode),
                ],
                disabled: false,
            },
        ]);

        open_con_window(
            config.clone(),
            Session::load().unwrap_or_default(),
            true,
            cx,
        );

        // Initialize the auto-updater. On macOS this loads Sparkle
        // from the app bundle; on Windows it kicks off a notify-only
        // background check against the release appcast.
        updater::init();
    });
}
