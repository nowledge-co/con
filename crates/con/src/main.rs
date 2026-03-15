mod agent_panel;
mod input_bar;
mod settings_panel;
mod terminal_view;
mod theme;
mod workspace;

use gpui::*;
use gpui_component::ActiveTheme;
use workspace::ConWorkspace;

actions!(con, [Quit, NewTab, ToggleAgentPanel, CloseTab]);

fn main() {
    env_logger::init();

    let config = con_core::Config::load().unwrap_or_default();

    let app = gpui_platform::application().with_assets(gpui_component_assets::Assets);
    app.run(move |cx: &mut App| {
        // Initialize gpui-component subsystems (theme, input, dialog, etc.)
        gpui_component::init(cx);

        // Load and activate con's design theme
        theme::init_theme(cx);

        // Register global keybindings
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-t", NewTab, None),
            KeyBinding::new("cmd-l", ToggleAgentPanel, None),
            KeyBinding::new("cmd-w", CloseTab, None),
            KeyBinding::new("cmd-,", settings_panel::ToggleSettings, None),
        ]);

        cx.on_action::<Quit>(|_, cx| {
            cx.quit();
        });

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(
                size(px(1200.0), px(800.0)),
                cx,
            )),
            titlebar: Some(TitlebarOptions {
                title: Some("con".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| ConWorkspace::new(config.clone(), window, cx));
                cx.new(|cx| {
                    gpui_component::Root::new(view, window, cx).bg(cx.theme().background)
                })
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}
