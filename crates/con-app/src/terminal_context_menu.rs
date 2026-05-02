use gpui::{Action, Context, Window};
use gpui_component::menu::PopupMenu;

pub(crate) fn terminal_context_menu(
    menu: PopupMenu,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    menu.menu("Paste", Box::new(crate::Paste) as Box<dyn Action>)
        .menu("Copy", Box::new(crate::Copy) as Box<dyn Action>)
        .menu(
            "Clear Terminal",
            Box::new(crate::ClearTerminal) as Box<dyn Action>,
        )
        .separator()
        .submenu("Split Pane", window, cx, |menu, _window, _cx| {
            menu.menu("Right", Box::new(crate::SplitRight) as Box<dyn Action>)
                .menu("Down", Box::new(crate::SplitDown) as Box<dyn Action>)
                .menu("Left", Box::new(crate::SplitLeft) as Box<dyn Action>)
                .menu("Up", Box::new(crate::SplitUp) as Box<dyn Action>)
        })
        .menu(
            "Toggle Pane Zoom",
            Box::new(crate::TogglePaneZoom) as Box<dyn Action>,
        )
        .menu("Close Pane", Box::new(crate::ClosePane) as Box<dyn Action>)
        .separator()
        .submenu("Surfaces", window, cx, |menu, _window, _cx| {
            menu.menu(
                "New in Pane",
                Box::new(crate::NewSurface) as Box<dyn Action>,
            )
            .menu(
                "New Split Right",
                Box::new(crate::NewSurfaceSplitRight) as Box<dyn Action>,
            )
            .menu(
                "New Split Down",
                Box::new(crate::NewSurfaceSplitDown) as Box<dyn Action>,
            )
            .separator()
            .menu("Next", Box::new(crate::NextSurface) as Box<dyn Action>)
            .menu(
                "Previous",
                Box::new(crate::PreviousSurface) as Box<dyn Action>,
            )
            .menu(
                "Close Current",
                Box::new(crate::CloseSurface) as Box<dyn Action>,
            )
        })
        .separator()
        .menu(
            "Focus Input",
            Box::new(crate::FocusInput) as Box<dyn Action>,
        )
        .menu(
            "Command Palette",
            Box::new(crate::command_palette::ToggleCommandPalette) as Box<dyn Action>,
        )
}
