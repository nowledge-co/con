//! Activity bar — the file/search section switcher inside the sidebar drawer.
//!
//! Clicking a slot icon switches the sidebar section. Clicking the
//! already-active slot toggles the drawer/panel closed.
//!
//! Visual rules
//! ---
//! - Fixed height: 36 px while the left sidebar is visible.
//! - Active slot: accent-colored icon.
//! - Inactive slots: muted_foreground icon.
//! - No text labels — icons only.
//! - Surface separation via bg opacity, no borders.

use gpui::{
    Context, EventEmitter, IntoElement, ParentElement, Render, Styled, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme, Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
};

pub const ACTIVITY_BAR_HEADER_HEIGHT: f32 = 36.0;

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
            self.left_panel_open = !self.left_panel_open;
            cx.emit(ActivityTogglePanel);
        } else {
            self.active_slot = slot;
            self.left_panel_open = true;
            cx.emit(ActivitySlotChanged { slot });
        }
        cx.notify();
    }
}

impl Render for ActivityBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let active_slot = self.active_slot;
        let compact_launcher = !self.left_panel_open;

        div()
            .id("activity-bar")
            .flex()
            .items_center()
            .justify_center()
            .gap(px(4.0))
            .flex_shrink_0()
            .when(compact_launcher, |bar| {
                bar.w(px(36.0)).h(px(70.0)).flex_col()
            })
            .when(!compact_launcher, |bar| {
                bar.h(px(ACTIVITY_BAR_HEADER_HEIGHT))
                    .w_full()
                    .flex_row()
                    .px(px(8.0))
            })
            .child(activity_slot_button(
                "activity-files",
                "phosphor/folders.svg",
                active_slot == ActivitySlot::Files,
                theme,
                cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                    this.set_slot(ActivitySlot::Files, cx);
                }),
            ))
            .child(activity_slot_button(
                "activity-search",
                "phosphor/file-magnifying-glass.svg",
                active_slot == ActivitySlot::Search,
                theme,
                cx.listener(|this, _: &gpui::ClickEvent, _window, cx| {
                    this.set_slot(ActivitySlot::Search, cx);
                }),
            ))
    }
}

fn activity_slot_button<F>(
    id: &'static str,
    icon: &'static str,
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
        .text_color(icon_color)
        .rounded(px(6.0))
        .with_size(px(28.0))
        .cursor_pointer()
        .on_click(handler)
}
