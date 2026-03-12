mod agent_panel;
mod input_bar;
mod terminal_view;
mod theme;
mod workspace;

use gpui::*;
use workspace::ConWorkspace;

actions!(con, [Quit, NewTab, ToggleAgentPanel, CloseTab]);

fn main() {
    env_logger::init();

    let config = con_core::Config::load().unwrap_or_default();

    Application::new().run(move |cx: &mut App| {
        // Register global keybindings
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-t", NewTab, None),
            KeyBinding::new("cmd-l", ToggleAgentPanel, None),
            KeyBinding::new("cmd-w", CloseTab, None),
        ]);

        cx.on_action::<Quit>(|_, cx| {
            cx.quit();
        });

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(1200.0), px(800.0)),
            })),
            titlebar: Some(TitlebarOptions {
                title: Some("con".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            ..Default::default()
        };

        cx.open_window(window_options, |_, cx| {
            cx.new(|cx| ConWorkspace::new(config.clone(), cx))
        })
        .unwrap();
    });
}
