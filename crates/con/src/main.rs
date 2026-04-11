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
mod workspace;

use gpui::*;
use gpui_component::ActiveTheme;
use workspace::ConWorkspace;

actions!(
    con,
    [
        Quit,
        NewWindow,
        NewTab,
        ToggleAgentPanel,
        ToggleInputBar,
        CloseTab,
        SplitRight,
        SplitDown,
        FocusInput,
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

fn open_con_window(config: con_core::Config, exit_on_error: bool, cx: &mut App) {
    let window_options = default_window_options(cx);
    cx.spawn(async move |cx| {
        if let Err(err) = cx.open_window(window_options, |window, cx| {
            let view = cx.new(|cx| ConWorkspace::new(config.clone(), window, cx));
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

fn main() {
    env_logger::init();

    let config = con_core::Config::load().unwrap_or_default();

    let app = gpui_platform::application()
        .with_quit_mode(QuitMode::Explicit)
        .with_assets(assets::ConAssets);
    app.run(move |cx: &mut App| {
        // Set dock icon for development (cargo run)
        set_dock_icon();

        // Initialize gpui-component subsystems (theme, input, dialog, etc.)
        gpui_component::init(cx);

        // Load and activate con's design theme (synced to terminal theme)
        theme::init_theme(
            cx,
            &config.terminal.theme,
            &config.terminal.font_family,
            &config.appearance.ui_font_family,
        );

        // Register ghostty terminal key bindings (Tab interception, etc.)
        #[cfg(target_os = "macos")]
        ghostty_view::init(cx);

        // Register global keybindings from user config
        let kb = &config.keybindings;
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
            KeyBinding::new(&kb.toggle_input_bar, ToggleInputBar, None),
        ]);

        cx.on_action(|_: &NewWindow, cx: &mut App| {
            let config = con_core::Config::load().unwrap_or_default();
            open_con_window(config, false, cx);
        });
        cx.on_action(|_: &NewTab, cx: &mut App| {
            if cx.active_window().is_none() {
                let config = con_core::Config::load().unwrap_or_default();
                open_con_window(config, false, cx);
            }
        });

        cx.set_menus(vec![
            Menu {
                name: "con".into(),
                items: vec![
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
                ],
                disabled: false,
            },
        ]);

        open_con_window(config.clone(), true, cx);
    });
}
