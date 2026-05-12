//! Activity bar — the file/search section switcher inside the sidebar drawer.
//!
//! Clicking a slot icon switches the sidebar section. Clicking the
//! already-active slot toggles the drawer/panel closed.
//!
//! Visual rules
//! ---
//! - Fixed height: 32 px while the left sidebar is visible.
//! - Active slot: accent-colored icon.
//! - Inactive slots: muted_foreground icon.
//! - No text labels — icons only.
//! - Surface separation via bg opacity, no borders.

use gpui::{
    Context, EventEmitter, IntoElement, ParentElement, Render, Styled, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme, Icon, Selectable, Sizable as _,
    button::{Button, ButtonVariants as _},
};

pub const ACTIVITY_BAR_HEADER_HEIGHT: f32 = 32.0;

/// The content slot currently shown in the left panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivitySlot {
    /// File explorer — backed by `FileTreeView`.
    Files,
    /// Workspace text search — backed by `SidebarSearchView`.
    Search,
}

impl ActivitySlot {
    pub fn as_str(self) -> &'static str {
        match self {
            ActivitySlot::Files => "files",
            ActivitySlot::Search => "search",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "search" => ActivitySlot::Search,
            _ => ActivitySlot::Files,
        }
    }
}

/// Emitted when the user clicks a slot icon.
pub struct ActivitySlotChanged {
    pub slot: ActivitySlot,
}

/// Emitted when the user clicks the already-active slot (toggle panel).
pub struct ActivityTogglePanel;

impl EventEmitter<ActivitySlotChanged> for ActivityBar {}
impl EventEmitter<ActivityTogglePanel> for ActivityBar {}

pub struct ActivityBar {
    pub active_slot: ActivitySlot,
    pub left_panel_open: bool,
}

impl ActivityBar {
    pub fn new() -> Self {
        Self {
            active_slot: ActivitySlot::Files,
            left_panel_open: true,
        }
    }

    pub fn set_slot(&mut self, slot: ActivitySlot, cx: &mut Context<Self>) {
        if self.active_slot == slot {
            self.close_panel(cx);
        } else {
            self.active_slot = slot;
            self.left_panel_open = true;
            cx.emit(ActivitySlotChanged { slot });
            cx.notify();
        }
    }

    pub fn close_panel(&mut self, cx: &mut Context<Self>) {
        if self.left_panel_open {
            self.left_panel_open = false;
            cx.emit(ActivityTogglePanel);
            cx.notify();
        }
    }
}

impl Render for ActivityBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let active_slot = self.active_slot;

        div()
            .id("activity-bar")
            .h(px(ACTIVITY_BAR_HEADER_HEIGHT))
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px(px(10.0))
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(3.0))
                    .p(px(1.0))
                    .rounded(px(6.0))
                    .bg(theme.foreground.opacity(0.038))
                    .child(activity_slot_button(
                        "activity-files",
                        "phosphor/folder.svg",
                        "Files",
                        active_slot == ActivitySlot::Files,
                        theme,
                        cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                            this.set_slot(ActivitySlot::Files, cx);
                        }),
                    ))
                    .child(activity_slot_button(
                        "activity-search",
                        "phosphor/magnifying-glass.svg",
                        "Search",
                        active_slot == ActivitySlot::Search,
                        theme,
                        cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                            this.set_slot(ActivitySlot::Search, cx);
                        }),
                    )),
            )
            .child(
                Button::new("activity-close")
                    .icon(Icon::default().path("phosphor/x.svg"))
                    .ghost()
                    .text_color(theme.muted_foreground)
                    .rounded(px(5.0))
                    .with_size(px(22.0))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                        this.close_panel(cx);
                    })),
            )
    }
}

fn activity_slot_button<F>(
    id: &'static str,
    icon: &'static str,
    tooltip: &'static str,
    active: bool,
    theme: &gpui_component::Theme,
    handler: F,
) -> impl IntoElement
where
    F: Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
{
    let icon_color = if active {
        theme.primary
    } else {
        theme.muted_foreground
    };
    Button::new(id)
        .icon(Icon::default().path(icon))
        .ghost()
        .selected(active)
        .text_color(icon_color)
        .rounded(px(5.0))
        .with_size(px(24.0))
        .tooltip(tooltip)
        .cursor_pointer()
        .on_click(handler)
}
