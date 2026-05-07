/// Shared tab context menu builder used by both the horizontal tab strip
/// and the vertical sidebar panel rows.
use con_core::session::TabAccentColor;
use gpui::{App, ElementId, Hsla, MouseButton, Window, div, prelude::*, px};
use gpui_component::{
    ActiveTheme, h_flex,
    menu::{PopupMenu, PopupMenuItem},
};
use std::cell::Cell;
use std::rc::Rc;

type WinCb = Box<dyn Fn(&mut Window, &mut App) + 'static>;
type ColorCb = Box<dyn Fn(Option<TabAccentColor>, &mut Window, &mut App) + 'static>;

/// Orientation-specific actions. Each field is `Option` — `None` means
/// the item is not shown for this orientation.
pub(crate) struct TabMenuOptions {
    pub rename: WinCb,
    pub duplicate: WinCb,
    /// "Reset Name" — only shown when tab has a user label.
    pub reset_name: Option<WinCb>,
    /// "Move Up" — vertical only, only when index > 0.
    pub move_up: Option<WinCb>,
    /// "Move Down" — vertical only, only when index < total-1.
    pub move_down: Option<WinCb>,
    /// "Close Tabs to the Right" — horizontal only.
    pub close_to_right: Option<WinCb>,
    pub close_tab: WinCb,
    /// `None` = hide (single tab).
    pub close_others: Option<WinCb>,
    /// Called when a color is selected (None = clear).
    pub set_color: ColorCb,
    /// Current tab color, used to show the active ring.
    pub current_color: Option<TabAccentColor>,
}

const ACCENT_COLORS: &[Option<TabAccentColor>] = &[
    None,
    Some(TabAccentColor::Red),
    Some(TabAccentColor::Orange),
    Some(TabAccentColor::Yellow),
    Some(TabAccentColor::Green),
    Some(TabAccentColor::Teal),
    Some(TabAccentColor::Blue),
    Some(TabAccentColor::Purple),
    Some(TabAccentColor::Pink),
];

pub(crate) fn build_tab_context_menu(menu: PopupMenu, opts: TabMenuOptions) -> PopupMenu {
    let TabMenuOptions {
        rename,
        duplicate,
        reset_name,
        move_up,
        move_down,
        close_to_right,
        close_tab,
        close_others,
        set_color,
        current_color,
    } = opts;

    let mut menu = menu
        .item(PopupMenuItem::new("Rename").on_click(move |_, window, cx| rename(window, cx)))
        .item(
            PopupMenuItem::new("Duplicate Tab")
                .on_click(move |_, window, cx| duplicate(window, cx)),
        );

    if let Some(reset) = reset_name {
        menu = menu.item(
            PopupMenuItem::new("Reset Name").on_click(move |_, window, cx| reset(window, cx)),
        );
    }

    menu = menu.separator();

    if let Some(up) = move_up {
        menu =
            menu.item(PopupMenuItem::new("Move Up").on_click(move |_, window, cx| up(window, cx)));
    }
    if let Some(down) = move_down {
        menu = menu
            .item(PopupMenuItem::new("Move Down").on_click(move |_, window, cx| down(window, cx)));
    }

    menu = menu
        .separator()
        .item(PopupMenuItem::new("Close Tab").on_click(move |_, window, cx| close_tab(window, cx)));

    if let Some(others) = close_others {
        menu = menu.item(
            PopupMenuItem::new("Close Other Tabs")
                .on_click(move |_, window, cx| others(window, cx)),
        );
    }

    if let Some(right) = close_to_right {
        menu = menu.item(
            PopupMenuItem::new("Close Tabs to the Right")
                .on_click(move |_, window, cx| right(window, cx)),
        );
    }

    // Color swatches — all in a single row via one ElementItem.
    // A shared Cell carries the swatch the user pressed (via on_mouse_down)
    // to the ElementItem's on_click handler which fires when the menu row
    // is clicked and closes the menu.
    menu = menu.separator();

    let set_color = Rc::new(set_color);
    // Use a sentinel that means "nothing pressed yet". Color choices,
    // including "No Color", are represented by their swatch index.
    let pressed: Rc<Cell<u8>> = Rc::new(Cell::new(u8::MAX));
    let pressed_click = pressed.clone();
    let set_color_click = set_color.clone();

    menu.item(
        PopupMenuItem::element(move |_window, cx| {
            let dot = px(14.0);
            let ring = px(20.0);

            let mut row = h_flex().gap(px(4.0)).px(px(2.0)).py(px(2.0));

            for (idx, &color) in ACCENT_COLORS.iter().enumerate() {
                let swatch_hsla: Option<Hsla> =
                    color.map(|c| crate::tab_colors::tab_accent_color_hsla(c, cx));
                let is_current = current_color == color;
                let pressed_ref = pressed.clone();
                let idx_u8 = idx as u8;

                let swatch_bg = swatch_hsla.unwrap_or(cx.theme().muted.opacity(0.35));
                let wrapper_bg = if is_current {
                    swatch_hsla.unwrap_or(cx.theme().muted).opacity(0.25)
                } else {
                    gpui::transparent_black()
                };

                let swatch = div()
                    .id(ElementId::Integer(idx as u64))
                    .w(ring)
                    .h(ring)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .rounded_full()
                    .bg(wrapper_bg)
                    .hover(|s| s.opacity(0.75))
                    .on_mouse_down(MouseButton::Left, move |_, _, _| {
                        pressed_ref.set(idx_u8);
                    })
                    .child(div().w(dot).h(dot).rounded_full().bg(swatch_bg));

                row = row.child(swatch);
            }

            row
        })
        .on_click(move |_, window, cx| {
            let idx = pressed_click.get();
            if (idx as usize) < ACCENT_COLORS.len() {
                set_color_click(ACCENT_COLORS[idx as usize], window, cx);
            }
            // Reset sentinel
            pressed_click.set(u8::MAX);
        }),
    )
}
