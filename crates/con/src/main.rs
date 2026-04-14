// Suppress warnings from objc 0.2's `sel_impl!` and `class!` macros
// checking `cfg(feature = "cargo-clippy")`.
#![allow(unexpected_cfgs)]

mod agent_panel;
mod assets;
mod chat_markdown;
mod command_palette;
#[cfg(target_os = "macos")]
mod ghostty_view;
mod input_bar;
mod model_registry;
mod motion;
mod pane_tree;
mod settings_panel;
mod sidebar;
mod terminal_pane;
mod theme;
#[cfg(target_os = "macos")]
mod updater;
mod workspace;

use con_core::config::KeybindingConfig;
use con_core::session::Session;
use gpui::*;
use gpui_component::{
    ActiveTheme,
    link::Link,
};
use workspace::ConWorkspace;

actions!(
    con,
    [
        ShowAbout,
        Quit,
        NewWindow,
        NewTab,
        ToggleAgentPanel,
        ToggleInputBar,
        CloseTab,
        SplitRight,
        SplitDown,
        FocusInput,
        CycleInputMode,
        TogglePaneScopePicker,
        CheckForUpdates,
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

#[cfg(not(target_os = "macos"))]
compile_error!("con currently requires macOS and the embedded Ghostty backend.");

fn default_window_options(cx: &mut App) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::centered(size(px(1200.0), px(800.0)), cx)),
        titlebar: Some(TitlebarOptions {
            title: Some("con".into()),
            appears_transparent: true,
            ..Default::default()
        }),
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    }
}

fn open_con_window(config: con_core::Config, session: Session, exit_on_error: bool, cx: &mut App) {
    let window_options = default_window_options(cx);
    cx.spawn(async move |cx| {
        if let Err(err) = cx.open_window(window_options, |window, cx| {
            let restored_session = session.clone();
            let view = cx
                .new(|cx| ConWorkspace::from_session(config.clone(), restored_session, window, cx));
            cx.new(|cx| gpui_component::Root::new(view, window, cx).bg(cx.theme().transparent))
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

pub(crate) fn app_display_version() -> String {
    #[cfg(target_os = "macos")]
    let version = bundle_info_value(b"CFBundleShortVersionString\0")
        .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    #[cfg(not(target_os = "macos"))]
    let version = env!("CARGO_PKG_VERSION").to_string();

    version
}

pub(crate) fn app_build_number() -> String {
    #[cfg(target_os = "macos")]
    let build =
        bundle_info_value(b"CFBundleVersion\0").unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string());
    #[cfg(not(target_os = "macos"))]
    let build = env!("CARGO_PKG_VERSION").to_string();

    build
}

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

struct AboutView {
    app_name: String,
    version: String,
    version_detail: String,
}

impl Render for AboutView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let repo_url = "https://github.com/nowledge-co/con";
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
}

fn main() {
    env_logger::init();

    let config = con_core::Config::load().unwrap_or_default();

    let app = gpui_platform::application()
        .with_quit_mode(QuitMode::Explicit)
        .with_assets(assets::ConAssets);
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
        #[cfg(target_os = "macos")]
        ghostty_view::init(cx);

        // Register global keybindings from user config
        bind_app_keybindings(cx, &config.keybindings);

        cx.on_action(|_: &NewWindow, cx: &mut App| {
            let config = con_core::Config::load().unwrap_or_default();
            open_con_window(config, Session::default(), false, cx);
        });
        cx.on_action(|_: &NewTab, cx: &mut App| {
            if cx.active_window().is_none() {
                let config = con_core::Config::load().unwrap_or_default();
                open_con_window(config, Session::default(), false, cx);
            }
        });

        #[cfg(target_os = "macos")]
        cx.on_action(|_: &ShowAbout, cx: &mut App| {
            show_about_window(cx);
        });

        #[cfg(target_os = "macos")]
        cx.on_action(|_: &CheckForUpdates, _cx: &mut App| {
            updater::check_for_updates();
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

        // Initialize Sparkle auto-updater (loads framework from app bundle)
        #[cfg(target_os = "macos")]
        updater::init();
    });
}
