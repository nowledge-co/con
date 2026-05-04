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
#[cfg(target_os = "macos")]
mod cli_shim;
mod command_palette;
#[cfg(target_os = "macos")]
mod global_hotkey;
#[cfg(target_os = "macos")]
mod hotkey_window;
#[cfg(target_os = "macos")]
mod macos_windowing;

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
mod terminal_context_menu;
#[cfg(any(target_os = "windows", target_os = "linux"))]
mod terminal_ime;
mod terminal_links;
mod terminal_pane;
mod terminal_paste;
mod terminal_restore;
mod terminal_shortcuts;
mod theme;
mod updater;
mod workspace;

use con_core::config::KeybindingConfig;
use con_core::session::{
    GlobalHistoryState, PaneLayoutState, Session, SurfaceState, WORKSPACE_ERROR_SURFACE_OWNER,
};
use con_core::workspace_layout::WorkspaceLayout;
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
        SelectTab1,
        SelectTab2,
        SelectTab3,
        SelectTab4,
        SelectTab5,
        SelectTab6,
        SelectTab7,
        SelectTab8,
        SelectTab9,
        NextWindow,
        PreviousWindow,
        ToggleSummon,
        ToggleHotkeyWindow,
        ToggleAgentPanel,
        ToggleInputBar,
        ToggleVerticalTabs,
        CloseTab,
        ClosePane,
        TogglePaneZoom,
        SplitRight,
        SplitDown,
        SplitLeft,
        SplitUp,
        ClearTerminal,
        ClearRestoredTerminalHistory,
        ExportWorkspaceLayout,
        AddWorkspaceLayoutTabs,
        OpenWorkspaceLayoutWindow,
        NewSurface,
        NewSurfaceSplitRight,
        NewSurfaceSplitDown,
        NextSurface,
        PreviousSurface,
        RenameSurface,
        CloseSurface,
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
    // GPUI's X11 backend already creates the window against an ARGB
    // (depth-32) visual when transparency is requested, and the
    // Wayland backend drops the opaque-region hint so the compositor
    // honors per-pixel alpha. Both xfwm4 / mutter / kwin composite
    // through it, so an always-on transparent root is safe on every
    // Linux session that has any compositor at all. Headless / minimal
    // sessions still render correctly because the workspace itself
    // paints `theme.background.opacity(...)` on its surfaces.
    true
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

/// Live re-apply of the Linux background appearance. Mirrors
/// `set_windows_backdrop_blur` so the workspace theme-update path
/// can toggle "background blur" in settings without restarting the
/// window.
///
/// `blur=true` requests `WindowBackgroundAppearance::Blurred`. The
/// gpui_linux Wayland backend honors that via the
/// `org_kde_kwin_blur` protocol (real Gaussian blur of what's
/// behind the window — works on KDE Plasma Wayland). On X11 and on
/// Wayland compositors that don't expose the blur protocol
/// (mutter / GNOME, sway by default), the renderer keeps the
/// window transparent but does NOT draw a blur — there's no
/// equivalent of DWM's Acrylic API there. We still flip to
/// `Transparent` in that case so per-pane opacity has a desktop
/// to composite over.
#[cfg(target_os = "linux")]
pub fn set_linux_window_blur(window: &mut Window, blur: bool) {
    use gpui::WindowBackgroundAppearance;
    window.set_background_appearance(if blur {
        WindowBackgroundAppearance::Blurred
    } else {
        WindowBackgroundAppearance::Transparent
    });
}

#[cfg(target_os = "macos")]
fn should_use_macos_window_glass_backdrop(blur: bool, effective_opacity: f32) -> bool {
    blur && effective_opacity < 0.999
}

#[cfg(target_os = "macos")]
pub fn set_macos_window_glass_backdrop(window: &mut Window, blur: bool, effective_opacity: f32) {
    if !supports_transparent_main_window() {
        window.set_background_appearance(WindowBackgroundAppearance::Opaque);
        return;
    }

    window.set_background_appearance(
        if should_use_macos_window_glass_backdrop(blur, effective_opacity) {
            WindowBackgroundAppearance::Blurred
        } else {
            WindowBackgroundAppearance::Transparent
        },
    );
}

#[cfg(target_os = "windows")]
fn set_windows_backdrop(window: &mut Window, blur: bool) -> Option<()> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DWMSBT_MAINWINDOW, DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DwmSetWindowAttribute,
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

fn default_window_options(config: &con_core::Config, cx: &mut App) -> WindowOptions {
    let transparent = supports_transparent_main_window();
    WindowOptions {
        window_bounds: Some(default_workspace_window_bounds(cx)),
        titlebar: default_titlebar_options(transparent),
        window_decorations: default_window_decorations(),
        window_background: default_window_background(config, transparent),
        ..Default::default()
    }
}

#[cfg(target_os = "macos")]
fn hotkey_window_bounds(cx: &mut App) -> WindowBounds {
    let fallback_size = size(px(1440.0), px(480.0));
    let fallback_bounds = Bounds::centered(None, fallback_size, cx);

    let Some(display) = cx.primary_display() else {
        return WindowBounds::Windowed(fallback_bounds);
    };

    let visible = display.visible_bounds();
    let bounds = Bounds::new(
        visible.origin,
        size(visible.size.width, visible.size.height / 2.0),
    );
    WindowBounds::Windowed(bounds)
}

#[cfg(target_os = "macos")]
fn hotkey_window_options(config: &con_core::Config, cx: &mut App) -> WindowOptions {
    let mut options = default_window_options(config, cx);
    options.window_bounds = Some(hotkey_window_bounds(cx));
    options.titlebar = None;
    options
}

fn default_window_background(
    config: &con_core::Config,
    transparent: bool,
) -> WindowBackgroundAppearance {
    #[cfg(not(target_os = "macos"))]
    let _ = config;

    if !transparent {
        return WindowBackgroundAppearance::Opaque;
    }

    #[cfg(target_os = "macos")]
    {
        let effective_opacity =
            ConWorkspace::effective_terminal_opacity(config.appearance.terminal_opacity);
        if should_use_macos_window_glass_backdrop(
            config.appearance.terminal_blur,
            effective_opacity,
        ) {
            return WindowBackgroundAppearance::Blurred;
        }
    }

    WindowBackgroundAppearance::Transparent
}

fn default_workspace_window_bounds(cx: &mut App) -> WindowBounds {
    let fallback_size = size(px(1200.0), px(800.0));
    let fallback_bounds = Bounds::centered(None, fallback_size, cx);
    let fallback_display_bounds = cx.primary_display().map(|display| display.visible_bounds());

    let active_bounds = cx.active_window().and_then(|handle| {
        handle
            .update(cx, |_, window, cx| {
                let bounds = match window.window_bounds() {
                    WindowBounds::Windowed(bounds) => bounds,
                    WindowBounds::Maximized(_) | WindowBounds::Fullscreen(_) => {
                        return None;
                    }
                };
                let display_bounds = window
                    .display(cx)
                    .map(|display| display.visible_bounds())
                    .or(fallback_display_bounds);
                Some((bounds, display_bounds))
            })
            .ok()
            .flatten()
    });

    let Some((base_bounds, display_bounds)) = active_bounds else {
        return WindowBounds::Windowed(fallback_bounds);
    };

    const CASCADE_OFFSET: f32 = 28.0;

    let proposed_origin = base_bounds.origin + point(px(CASCADE_OFFSET), px(CASCADE_OFFSET));
    let proposed_bounds = Bounds::new(proposed_origin, base_bounds.size);
    let Some(display_bounds) = display_bounds else {
        return WindowBounds::Windowed(proposed_bounds);
    };

    let display_right = display_bounds.origin.x + display_bounds.size.width;
    let display_bottom = display_bounds.origin.y + display_bounds.size.height;
    let window_right = proposed_bounds.origin.x + proposed_bounds.size.width;
    let window_bottom = proposed_bounds.origin.y + proposed_bounds.size.height;

    let fits_horizontally = window_right <= display_right;
    let fits_vertically = window_bottom <= display_bottom;
    let final_origin = match (fits_horizontally, fits_vertically) {
        (true, true) => proposed_origin,
        (false, true) => point(display_bounds.origin.x, proposed_origin.y),
        (true, false) => point(proposed_origin.x, display_bounds.origin.y),
        (false, false) => display_bounds.origin,
    };

    WindowBounds::Windowed(Bounds::new(final_origin, base_bounds.size))
}

fn default_titlebar_options(transparent: bool) -> Option<TitlebarOptions> {
    Some(TitlebarOptions {
        title: Some("con".into()),
        appears_transparent: transparent,
        ..Default::default()
    })
}

fn default_window_decorations() -> Option<WindowDecorations> {
    // Linux now ships a client-side titlebar drawn in
    // `workspace.rs::draw_top_bar` (the same top bar Windows uses),
    // so the GPUI app shell paints its own brand chrome instead of
    // stacking the xfwm4 / mutter / kwin frame on top of it. The
    // gpui_linux X11 backend gracefully falls back to server-side
    // decorations when no compositor is present, so this is safe on
    // headless / minimal sessions too. macOS continues to use the
    // default (system traffic-light cluster over an in-app top bar
    // with `leading_pad = 78px`).
    #[cfg(target_os = "linux")]
    {
        Some(WindowDecorations::Client)
    }

    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

pub(crate) fn open_con_window(
    config: con_core::Config,
    session: Session,
    exit_on_error: bool,
    cx: &mut App,
) {
    #[cfg(target_os = "macos")]
    crate::hotkey_window::ensure_created_for_app_run(cx);

    let window_options = default_window_options(&config, cx);
    cx.spawn(async move |cx| {
        if let Err(err) = cx.open_window(window_options, |window, cx| {
            let restored_session = session.clone();
            let view = cx
                .new(|cx| ConWorkspace::from_session(config.clone(), restored_session, window, cx));
            #[cfg(target_os = "windows")]
            let mica_applied = apply_windows_backdrop(window, config.appearance.terminal_blur);
            #[cfg(not(target_os = "windows"))]
            let mica_applied = false;

            #[cfg(target_os = "linux")]
            set_linux_window_blur(window, config.appearance.terminal_blur);
            cx.new(|cx| {
                // On Windows we want the DWM-provided backdrop (Mica)
                // to shine through wherever the app isn't painting —
                // most importantly, the terminal area when the child
                // pane HWND is hidden behind a modal. That requires a
                // transparent Root. If Mica isn't available (Win10,
                // RDP, older Win11), fall back to an opaque theme fill
                // so the window doesn't look "see-through to desktop".
                //
                // On macOS 12 and older we still need the window itself
                // to be opaque, but the GPUI root above the embedded
                // Ghostty NSView must stay transparent. Making both the
                // window and the root opaque fixed the desktop leak but
                // painted the fallback theme background over the terminal
                // surface, leaving Monterey users with a blank beige pane.
                let background = if cfg!(target_os = "windows") {
                    if mica_applied {
                        cx.theme().transparent
                    } else {
                        cx.theme().background
                    }
                } else if cfg!(target_os = "macos") {
                    // The window background policy decides whether the
                    // desktop can bleed through. On macOS the root
                    // itself must remain transparent so the embedded
                    // Ghostty NSView below GPUI stays visible, even on
                    // Monterey where the top-level window falls back to
                    // opaque.
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

#[cfg(target_os = "macos")]
pub(crate) fn open_hotkey_window(config: con_core::Config, session: Session, cx: &mut App) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let window_options = hotkey_window_options(&config, cx);
    cx.spawn(async move |cx| {
        if let Err(err) = cx.open_window(window_options, |window, cx| {
            let restored_session = session.clone();
            let view = cx
                .new(|cx| ConWorkspace::from_session(config.clone(), restored_session, window, cx));

            let raw_ptr = HasWindowHandle::window_handle(window)
                .ok()
                .and_then(|handle| match handle.as_raw() {
                    RawWindowHandle::AppKit(handle) => Some(handle.ns_view.as_ptr().cast()),
                    _ => None,
                })
                .and_then(crate::hotkey_window::window_from_view_ptr);
            if let Some(raw_ptr) = raw_ptr {
                crate::hotkey_window::store_window_ptr(
                    raw_ptr,
                    config.keybindings.hotkey_window_always_on_top,
                );
            }

            cx.new(|cx| gpui_component::Root::new(view, window, cx).bg(cx.theme().transparent))
        }) {
            log::error!("Failed to open hotkey window: {err}");
        }
    })
    .detach();
}

fn fresh_window_session_with_history() -> Session {
    fresh_window_session_with_history_for_cwd(None)
}

#[cfg(target_os = "macos")]
fn default_hotkey_window_cwd() -> Option<std::path::PathBuf> {
    dirs::home_dir()
}

#[cfg(not(target_os = "macos"))]
fn default_hotkey_window_cwd() -> Option<std::path::PathBuf> {
    None
}

fn fresh_window_session_with_history_for_cwd(cwd: Option<std::path::PathBuf>) -> Session {
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

    if let Some(cwd) = cwd {
        let cwd = cwd.to_string_lossy().to_string();
        if let Some(tab) = session.tabs.first_mut() {
            tab.cwd = Some(cwd.clone());
            if let Some(pane) = tab.panes.first_mut() {
                pane.cwd = Some(cwd);
            }
        }
    }

    session
}

fn fallback_cwd_for_workspace_path(path: &std::path::Path) -> Option<std::path::PathBuf> {
    if path.is_dir() {
        return Some(path.to_path_buf());
    }
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .and_then(|parent| {
            parent
                .ancestors()
                .find(|ancestor| !ancestor.as_os_str().is_empty() && ancestor.is_dir())
                .map(std::path::Path::to_path_buf)
        })
}

fn workspace_path_error_session(path: &std::path::Path, err: &anyhow::Error) -> Session {
    let mut session =
        fresh_window_session_with_history_for_cwd(fallback_cwd_for_workspace_path(path));
    let screen_text = vec![
        "Con could not open the requested workspace profile.".to_string(),
        format!("Path: {}", path.display()),
        format!("Reason: {err}"),
        "Opened a fresh shell instead. Fix the profile and run con <path> again.".to_string(),
        String::new(),
    ];

    if let Some(tab) = session.tabs.first_mut() {
        tab.title = "Workspace Error".to_string();
        tab.user_label = Some("Workspace Error".to_string());
        tab.focused_pane_id = Some(0);
        let cwd = tab
            .cwd
            .clone()
            .or_else(|| tab.panes.first().and_then(|pane| pane.cwd.clone()));
        tab.layout = Some(PaneLayoutState::Leaf {
            pane_id: 0,
            cwd: cwd.clone(),
            active_surface_id: Some(0),
            surfaces: vec![SurfaceState {
                surface_id: 0,
                title: Some("Shell".to_string()),
                // Synthetic Con-owned diagnostic surface. Keep it out of
                // human/subagent owner namespaces used by the orchestrator.
                owner: Some(WORKSPACE_ERROR_SURFACE_OWNER.to_string()),
                cwd,
                close_pane_when_last: false,
                screen_text,
            }],
        });
    }

    session
}

fn startup_path_argument() -> Option<std::path::PathBuf> {
    std::env::args_os().skip(1).find_map(|arg| {
        let text = arg.to_string_lossy();
        (!text.starts_with('-')).then(|| std::path::PathBuf::from(arg))
    })
}

pub(crate) fn session_from_workspace_layout_path(
    path: impl AsRef<std::path::Path>,
) -> anyhow::Result<Session> {
    let path = path.as_ref();
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    if path.is_file() {
        return session_from_workspace_layout_file(path);
    }

    if path.is_dir() {
        let layout_path = WorkspaceLayout::default_path_for_root(&path);
        if layout_path.exists() {
            return session_from_workspace_layout_file(layout_path);
        }
        return Ok(fresh_window_session_with_history_for_cwd(Some(path)));
    }

    anyhow::bail!("workspace path does not exist: {}", path.display())
}

fn startup_session() -> Session {
    if let Some(path) = startup_path_argument() {
        match session_from_workspace_layout_path(&path) {
            Ok(session) => {
                log::info!("opening workspace path {}", path.display());
                return session;
            }
            Err(err) => {
                log::warn!("failed to open workspace path {}: {err}", path.display());
                return workspace_path_error_session(&path, &err);
            }
        }
    }

    if live_control_endpoint_exists() {
        log::info!(
            "existing con control endpoint detected; opening a fresh window session with shared history"
        );
        fresh_window_session_with_history()
    } else {
        Session::load().unwrap_or_default()
    }
}

pub(crate) fn workspace_layout_root_for_file(path: &std::path::Path) -> std::path::PathBuf {
    let Some(parent) = path.parent() else {
        return std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    };

    if parent.file_name().and_then(|name| name.to_str()) == Some(".con")
        && let Some(project_root) = parent.parent()
    {
        return project_root.to_path_buf();
    }

    parent.to_path_buf()
}

pub(crate) fn session_from_workspace_layout_file(
    path: impl AsRef<std::path::Path>,
) -> anyhow::Result<Session> {
    let path = path.as_ref();
    let layout = WorkspaceLayout::load(path)?;
    let profile_root = workspace_layout_root_for_file(path);
    let layout_root = if layout.root.trim().is_empty() || layout.root == "." {
        profile_root
    } else {
        let root = std::path::Path::new(&layout.root);
        if root.is_absolute() {
            root.to_path_buf()
        } else {
            profile_root.join(root)
        }
    };
    let history = GlobalHistoryState::load().unwrap_or_default();
    layout.to_session(layout_root, Some(&history))
}

#[cfg(unix)]
fn live_control_endpoint_exists() -> bool {
    let path = con_core::control_socket_path();
    let addr = match socket2::SockAddr::unix(&path) {
        Ok(addr) => addr,
        Err(err) => {
            log::debug!(
                "skipping con control endpoint probe for invalid socket path {}: {err}",
                path.display()
            );
            return false;
        }
    };

    let socket = match socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None) {
        Ok(socket) => socket,
        Err(err) => {
            log::debug!("failed to create con control endpoint probe socket: {err}");
            return false;
        }
    };

    match socket.connect_timeout(&addr, std::time::Duration::from_millis(75)) {
        Ok(()) => true,
        Err(err) => {
            log::debug!(
                "con control endpoint probe did not find a live server at {}: {err}",
                path.display()
            );
            false
        }
    }
}

#[cfg(windows)]
fn live_control_endpoint_exists() -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Pipes::WaitNamedPipeW;
    use windows::core::PCWSTR;

    let path = con_core::control_socket_path();
    let pipe_name: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    if unsafe { WaitNamedPipeW(PCWSTR(pipe_name.as_ptr()), 75).as_bool() } {
        true
    } else {
        log::debug!(
            "con control endpoint probe did not find a live pipe at {} within 75ms",
            path.display()
        );
        false
    }
}

#[cfg(not(any(unix, windows)))]
fn live_control_endpoint_exists() -> bool {
    false
}

pub(crate) fn toggle_global_summon(cx: &mut App) {
    let frontmost_window = cx
        .window_stack()
        .and_then(|windows| windows.first().cloned());
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
    let build = bundle_info_value(b"CFBundleVersion\0")
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    #[cfg(not(target_os = "macos"))]
    let build = option_env!("CON_RELEASE_VERSION")
        .map(str::to_owned)
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());

    build
}

#[cfg(target_os = "linux")]
fn apply_linux_platform_override() {
    let Some(requested) = std::env::var("CON_LINUX_BACKEND")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
    else {
        return;
    };

    match requested.as_str() {
        "x11" => {
            if std::env::var_os("DISPLAY").is_some() {
                unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
                log::info!("Linux compositor override: forcing X11 backend");
            } else {
                log::warn!(
                    "CON_LINUX_BACKEND=x11 requested, but DISPLAY is not set; keeping default compositor detection"
                );
            }
        }
        "wayland" => {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                unsafe { std::env::remove_var("DISPLAY") };
                log::info!("Linux compositor override: forcing Wayland backend");
            } else {
                log::warn!(
                    "CON_LINUX_BACKEND=wayland requested, but WAYLAND_DISPLAY is not set; keeping default compositor detection"
                );
            }
        }
        other => {
            log::warn!(
                "Ignoring unsupported CON_LINUX_BACKEND={other:?}; expected \"x11\" or \"wayland\""
            );
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn apply_linux_platform_override() {}

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
                        div().size(px(88.0)).p(px(2.0)).child(
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
                        div().flex().flex_col().items_center().gap(px(6.0)).child(
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
        KeyBinding::new("secondary-1", SelectTab1, None),
        KeyBinding::new("secondary-2", SelectTab2, None),
        KeyBinding::new("secondary-3", SelectTab3, None),
        KeyBinding::new("secondary-4", SelectTab4, None),
        KeyBinding::new("secondary-5", SelectTab5, None),
        KeyBinding::new("secondary-6", SelectTab6, None),
        KeyBinding::new("secondary-7", SelectTab7, None),
        KeyBinding::new("secondary-8", SelectTab8, None),
        KeyBinding::new("secondary-9", SelectTab9, None),
        KeyBinding::new(&kb.toggle_agent, ToggleAgentPanel, None),
        KeyBinding::new(&kb.close_tab, CloseTab, None),
        KeyBinding::new(&kb.close_pane, ClosePane, None),
        KeyBinding::new(&kb.toggle_pane_zoom, TogglePaneZoom, None),
        KeyBinding::new(&kb.settings, settings_panel::ToggleSettings, None),
        KeyBinding::new(
            &kb.command_palette,
            command_palette::ToggleCommandPalette,
            None,
        ),
        KeyBinding::new(&kb.split_right, SplitRight, None),
        KeyBinding::new(&kb.split_down, SplitDown, None),
        KeyBinding::new(&kb.new_surface, NewSurface, None),
        KeyBinding::new(&kb.new_surface_split_right, NewSurfaceSplitRight, None),
        KeyBinding::new(&kb.new_surface_split_down, NewSurfaceSplitDown, None),
        KeyBinding::new(&kb.next_surface, NextSurface, None),
        KeyBinding::new(&kb.previous_surface, PreviousSurface, None),
        KeyBinding::new(&kb.rename_surface, RenameSurface, None),
        KeyBinding::new(&kb.close_surface, CloseSurface, None),
        KeyBinding::new(&kb.focus_input, FocusInput, None),
        KeyBinding::new(&kb.cycle_input_mode, CycleInputMode, None),
        KeyBinding::new(&kb.toggle_input_bar, ToggleInputBar, None),
        KeyBinding::new(&kb.toggle_pane_scope, TogglePaneScopePicker, None),
        KeyBinding::new(&kb.toggle_vertical_tabs, ToggleVerticalTabs, None),
        KeyBinding::new(&kb.quit, Quit, Some("Input")),
        KeyBinding::new(&kb.new_window, NewWindow, Some("Input")),
        KeyBinding::new(&kb.new_tab, NewTab, Some("Input")),
        KeyBinding::new(&kb.next_tab, NextTab, Some("Input")),
        KeyBinding::new(&kb.previous_tab, PreviousTab, Some("Input")),
        KeyBinding::new("secondary-shift-]", NextTab, Some("Input")),
        KeyBinding::new("secondary-shift-[", PreviousTab, Some("Input")),
        KeyBinding::new("secondary-1", SelectTab1, Some("Input")),
        KeyBinding::new("secondary-2", SelectTab2, Some("Input")),
        KeyBinding::new("secondary-3", SelectTab3, Some("Input")),
        KeyBinding::new("secondary-4", SelectTab4, Some("Input")),
        KeyBinding::new("secondary-5", SelectTab5, Some("Input")),
        KeyBinding::new("secondary-6", SelectTab6, Some("Input")),
        KeyBinding::new("secondary-7", SelectTab7, Some("Input")),
        KeyBinding::new("secondary-8", SelectTab8, Some("Input")),
        KeyBinding::new("secondary-9", SelectTab9, Some("Input")),
        KeyBinding::new(&kb.toggle_agent, ToggleAgentPanel, Some("Input")),
        KeyBinding::new(&kb.close_tab, CloseTab, Some("Input")),
        KeyBinding::new(&kb.close_pane, ClosePane, Some("Input")),
        KeyBinding::new(&kb.toggle_pane_zoom, TogglePaneZoom, Some("Input")),
        KeyBinding::new(&kb.settings, settings_panel::ToggleSettings, Some("Input")),
        KeyBinding::new(
            &kb.command_palette,
            command_palette::ToggleCommandPalette,
            Some("Input"),
        ),
        KeyBinding::new(&kb.split_right, SplitRight, Some("Input")),
        KeyBinding::new(&kb.split_down, SplitDown, Some("Input")),
        KeyBinding::new(&kb.new_surface, NewSurface, Some("Input")),
        KeyBinding::new(
            &kb.new_surface_split_right,
            NewSurfaceSplitRight,
            Some("Input"),
        ),
        KeyBinding::new(
            &kb.new_surface_split_down,
            NewSurfaceSplitDown,
            Some("Input"),
        ),
        KeyBinding::new(&kb.next_surface, NextSurface, Some("Input")),
        KeyBinding::new(&kb.previous_surface, PreviousSurface, Some("Input")),
        KeyBinding::new(&kb.rename_surface, RenameSurface, Some("Input")),
        KeyBinding::new(&kb.close_surface, CloseSurface, Some("Input")),
        KeyBinding::new(&kb.focus_input, FocusInput, Some("Input")),
        KeyBinding::new(&kb.cycle_input_mode, CycleInputMode, Some("Input")),
        KeyBinding::new(&kb.toggle_input_bar, ToggleInputBar, Some("Input")),
        KeyBinding::new(&kb.toggle_pane_scope, TogglePaneScopePicker, Some("Input")),
        KeyBinding::new(&kb.toggle_vertical_tabs, ToggleVerticalTabs, Some("Input")),
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
        KeyBinding::new("cmd-`", NextWindow, None),
        KeyBinding::new("cmd->", NextWindow, None),
        KeyBinding::new("cmd-shift-`", PreviousWindow, None),
        KeyBinding::new("cmd-~", PreviousWindow, None),
        KeyBinding::new("cmd-<", PreviousWindow, None),
        KeyBinding::new("cmd-h", HideApp, Some("Input")),
        KeyBinding::new("cmd-alt-h", HideOtherApps, Some("Input")),
        KeyBinding::new("cmd-alt-shift-h", ShowAllApps, Some("Input")),
        KeyBinding::new("cmd-`", NextWindow, Some("Input")),
        KeyBinding::new("cmd->", NextWindow, Some("Input")),
        KeyBinding::new("cmd-shift-`", PreviousWindow, Some("Input")),
        KeyBinding::new("cmd-~", PreviousWindow, Some("Input")),
        KeyBinding::new("cmd-<", PreviousWindow, Some("Input")),
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
            | 0xC000013A // CONTROL_C_EXIT
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
    let mut log_file_path = None;
    if let Some(path) = std::env::var_os("CON_LOG_FILE") {
        match std::fs::File::create(&path) {
            Ok(file) => {
                builder.target(env_logger::Target::Pipe(Box::new(file)));
                builder.write_style(env_logger::WriteStyle::Never);
                log_file_path = Some(std::path::PathBuf::from(&path));
            }
            Err(err) => {
                eprintln!(
                    "con: failed to open CON_LOG_FILE={}: {err}",
                    std::path::Path::new(&path).display()
                );
            }
        }
    }
    builder.init();

    log::info!("con starting (pid {})", std::process::id());
    if let Some(path) = log_file_path.as_ref() {
        log::info!("logging to {}", path.display());
    }
    #[cfg(target_os = "macos")]
    cli_shim::ensure_cli_shim();
    apply_linux_platform_override();

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
        #[cfg(target_os = "macos")]
        hotkey_window::init(cx, &config.keybindings);
        #[cfg(target_os = "macos")]
        macos_windowing::install_window_cycle_shortcuts();

        cx.on_action(|_: &NewWindow, cx: &mut App| {
            let config = con_core::Config::load().unwrap_or_default();
            open_con_window(config, fresh_window_session_with_history(), false, cx);
        });
        cx.on_action(|_: &ToggleSummon, cx: &mut App| {
            toggle_global_summon(cx);
        });
        #[cfg(target_os = "macos")]
        cx.on_action(|_: &ToggleHotkeyWindow, cx: &mut App| {
            hotkey_window::toggle(cx);
        });
        #[cfg(target_os = "macos")]
        cx.on_action(|_: &NextWindow, _cx: &mut App| {
            macos_windowing::cycle_app_window(false);
        });
        #[cfg(target_os = "macos")]
        cx.on_action(|_: &PreviousWindow, _cx: &mut App| {
            macos_windowing::cycle_app_window(true);
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
                    MenuItem::separator(),
                    MenuItem::action("Close Tab", CloseTab),
                    MenuItem::action("Close Pane", ClosePane),
                    MenuItem::action("Toggle Pane Zoom", TogglePaneZoom),
                    MenuItem::separator(),
                    MenuItem::action("Split Right", SplitRight),
                    MenuItem::action("Split Down", SplitDown),
                    MenuItem::action("Split Left", SplitLeft),
                    MenuItem::action("Split Up", SplitUp),
                    MenuItem::separator(),
                    MenuItem::action("Clear Terminal", ClearTerminal),
                    MenuItem::action(
                        "Clear Restored Terminal History",
                        ClearRestoredTerminalHistory,
                    ),
                    MenuItem::separator(),
                    MenuItem::action("New Surface Tab", NewSurface),
                    MenuItem::action("New Surface Pane Right", NewSurfaceSplitRight),
                    MenuItem::action("New Surface Pane Down", NewSurfaceSplitDown),
                    MenuItem::action("Next Surface Tab", NextSurface),
                    MenuItem::action("Previous Surface Tab", PreviousSurface),
                    MenuItem::action("Rename Current Surface", RenameSurface),
                    MenuItem::action("Close Current Surface", CloseSurface),
                ],
                disabled: false,
            },
            Menu {
                name: "Workspace".into(),
                items: vec![
                    MenuItem::action("Save Layout Profile…", ExportWorkspaceLayout),
                    MenuItem::separator(),
                    MenuItem::action("Add Tabs from Layout Profile…", AddWorkspaceLayoutTabs),
                    MenuItem::action(
                        "Open Layout Profile in New Window…",
                        OpenWorkspaceLayoutWindow,
                    ),
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
                    #[cfg(target_os = "macos")]
                    MenuItem::action("Hotkey Window", ToggleHotkeyWindow),
                    MenuItem::action("Command Palette", command_palette::ToggleCommandPalette),
                    MenuItem::separator(),
                    MenuItem::action("Toggle Input / Terminal", FocusInput),
                    MenuItem::action("Cycle Input Mode", CycleInputMode),
                ],
                disabled: false,
            },
            #[cfg(target_os = "macos")]
            Menu {
                name: "Window".into(),
                items: vec![
                    MenuItem::action("Next Window", NextWindow),
                    MenuItem::action("Previous Window", PreviousWindow),
                ],
                disabled: false,
            },
        ]);

        open_con_window(config.clone(), startup_session(), true, cx);

        // Initialize the auto-updater. On macOS this loads Sparkle
        // from the app bundle; on Windows it kicks off a notify-only
        // background check against the release appcast.
        updater::init();
    });
}
