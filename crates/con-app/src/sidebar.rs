//! Vertical tabs side panel.
//!
//! Toggle in *Settings → Appearance → Tabs → Vertical Tabs*.
//!
//! Two runtime states (no auto-expand-on-hover anymore — see design
//! note at the bottom of this docblock):
//!
//! - **Collapsed (rail)** — narrow icon rail (~44 px). One smart icon
//!   per tab. Hovering an icon pops a small floating **tab card**
//!   anchored to the right of the icon (name, subtitle, pane count).
//!   Card is purely informational — it never displaces the rail or
//!   the terminal pane and dismisses the moment the cursor leaves
//!   the icon. Drag an icon directly in collapsed mode to reorder.
//!
//! - **Pinned panel** — full panel (~240 px) with two-line rows
//!   (name in system font, optional subtitle in mono). Persisted
//!   across restart via `session.vertical_tabs_pinned`. Drag a row
//!   to reorder; right-click a row for the context menu.
//!
//! Why a hover card instead of an auto-expanding overlay
//! ---
//! The first iteration of vertical tabs auto-expanded a full panel
//! on hover. That's how Microsoft does Edge vertical tabs and it
//! reads as aggressive — passive intent (just trying to remember
//! what tab 3 is) takes over the workspace. It also makes drag-from-
//! collapsed broken: the user clicks an icon to drag, the cursor
//! leaves the icon to start the drag, the overlay dismisses, the
//! drop zone disappears. Apple's pattern (Finder sidebar, Mail
//! mailbox list) is a tooltip-style card that appears next to the
//! icon without taking over the layout. We do that.
//!
//! Visual rules
//! ---
//! - Active row: elevated pill bg + foreground text. **No accent
//!   bar.** A single, unambiguous selection cue is enough; doubling
//!   it (pill + bar + bold + accent color) is decorative chrome.
//! - Action affordances (rename pencil, close X) are hover-only on
//!   every row, including the active one. Quiet by default; reveal
//!   on intent.
//! - Surface separation comes from `surface_tone()` (foreground
//!   blended into background at small intensities), not borders.

use crate::motion::MotionValue;
use gpui::{
    AnyElement, App, Context, Div, Entity, EventEmitter, FontWeight, Hsla, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, ParentElement, Render, SharedString, Stateful,
    StatefulInteractiveElement, Styled, WeakEntity, Window, div, prelude::*, px, svg,
};
use gpui_component::{
    ActiveTheme, InteractiveElementExt, Sizable,
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
};
use std::time::Duration;

/// Width of the always-visible icon rail in collapsed mode.
pub const RAIL_WIDTH: f32 = 44.0;
/// Width of the full panel in pinned mode.
pub const PANEL_WIDTH: f32 = 240.0;
/// Width of the floating hover card shown when the cursor is over a
/// rail icon. Slightly wider than the panel so two-line rows are
/// comfortable at this magnification.
const HOVER_CARD_WIDTH: f32 = 240.0;
/// Per-row height in pinned mode (two-line layout — name + subtitle).
const ROW_HEIGHT: f32 = 44.0;
const ROW_HEIGHT_NO_SUBTITLE: f32 = 32.0;
/// Per-icon size in the rail.
const RAIL_ICON_SIZE: f32 = 32.0;
/// Vertical gap between rail icons. Used to compute the icon's
/// y-center for hover-card anchoring.
const RAIL_ICON_GAP: f32 = 2.0;

/// Cubic ease-out feel for the panel width animation.
const PANEL_TWEEN: Duration = Duration::from_millis(220);

/// One row in the vertical tabs panel.
pub struct SessionEntry {
    pub name: String,
    pub subtitle: Option<String>,
    pub is_ssh: bool,
    pub needs_attention: bool,
    pub icon: &'static str,
    pub has_user_label: bool,
    /// How many panes the tab contains (split count). Surfaced in
    /// the rail's hover card so the user can see "this tab has 3
    /// panes" without expanding.
    pub pane_count: usize,
}

/// Visual state of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelMode {
    Collapsed,
    Pinned,
}

/// Per-tab transient state — the inline-rename input.
struct RenameState {
    index: usize,
    input: Entity<InputState>,
}

/// What the user is currently dragging.
#[derive(Clone)]
pub struct DraggedTab {
    pub index: usize,
    pub label: SharedString,
    pub icon: &'static str,
}

impl Render for DraggedTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(10.0))
            .py(px(6.0))
            .rounded(px(8.0))
            .bg(surface_tone(theme, 0.22))
            .text_color(theme.foreground)
            .text_size(px(12.0))
            .font_family(theme.font_family.clone())
            .child(
                svg()
                    .path(self.icon)
                    .size(px(14.0))
                    .text_color(theme.foreground),
            )
            .child(self.label.clone())
    }
}

/// Vertical tabs side panel.
pub struct SessionSidebar {
    mode: PanelMode,
    sessions: Vec<SessionEntry>,
    active_session: usize,
    leading_top_pad: f32,
    /// Smooth width animation between rail (0.0) and pinned (1.0).
    width_motion: MotionValue,
    /// Inline rename state, `Some` while the user is editing a label.
    rename: Option<RenameState>,
    /// The rail-icon index currently under the cursor. Drives the
    /// floating hover card. Cleared on rail mouse-leave.
    hovered_rail: Option<usize>,
    /// Tab index the user is currently hovering as a drop target
    /// while a `DraggedTab` is in flight. Resets once the drop
    /// fires (or on the next render after the drag completes).
    drop_target: Option<usize>,
}

pub struct SidebarSelect {
    pub index: usize,
}
pub struct NewSession;
pub struct SidebarCloseTab {
    pub index: usize,
}
pub struct SidebarRename {
    pub index: usize,
    /// `None` clears the user override and falls back to smart naming.
    pub label: Option<String>,
}
pub struct SidebarDuplicate {
    pub index: usize,
}
pub struct SidebarReorder {
    pub from: usize,
    pub to: usize,
}
pub struct SidebarCloseOthers {
    pub index: usize,
}

impl EventEmitter<SidebarSelect> for SessionSidebar {}
impl EventEmitter<NewSession> for SessionSidebar {}
impl EventEmitter<SidebarCloseTab> for SessionSidebar {}
impl EventEmitter<SidebarRename> for SessionSidebar {}
impl EventEmitter<SidebarDuplicate> for SessionSidebar {}
impl EventEmitter<SidebarReorder> for SessionSidebar {}
impl EventEmitter<SidebarCloseOthers> for SessionSidebar {}

impl SessionSidebar {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            mode: PanelMode::Collapsed,
            sessions: Vec::new(),
            active_session: 0,
            leading_top_pad: if cfg!(target_os = "macos") { 36.0 } else { 6.0 },
            width_motion: MotionValue::new(0.0),
            rename: None,
            hovered_rail: None,
            drop_target: None,
        }
    }

    pub fn set_pinned(&mut self, pinned: bool, cx: &mut Context<Self>) {
        let new_mode = if pinned {
            PanelMode::Pinned
        } else {
            PanelMode::Collapsed
        };
        if self.mode != new_mode {
            self.mode = new_mode;
            self.cancel_rename(cx);
            self.hovered_rail = None;
            let target = if pinned { 1.0 } else { 0.0 };
            self.width_motion.set_target(target, PANEL_TWEEN);
            cx.notify();
        }
    }

    pub fn is_pinned(&self) -> bool {
        matches!(self.mode, PanelMode::Pinned)
    }

    /// Width the panel currently occupies in the workspace flex row.
    /// Tweens between RAIL_WIDTH and PANEL_WIDTH during pin/unpin.
    /// Hover cards float over the terminal area and never contribute
    /// to occupied width.
    pub fn occupied_width(&self) -> f32 {
        let t = self.width_motion.current().clamp(0.0, 1.0);
        RAIL_WIDTH + (PANEL_WIDTH - RAIL_WIDTH) * t
    }

    pub fn toggle_pinned(&mut self, cx: &mut Context<Self>) {
        let now_pinned = !self.is_pinned();
        self.set_pinned(now_pinned, cx);
    }

    /// Update the session list from workspace state.
    pub fn sync_sessions(
        &mut self,
        sessions: Vec<SessionEntry>,
        active: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(state) = &self.rename {
            if state.index >= sessions.len() {
                self.rename = None;
            }
        }
        self.sessions = sessions;
        self.active_session = active;
        cx.notify();
    }

    fn begin_rename(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.sessions.len() {
            return;
        }
        let initial = self.sessions[index].name.clone();
        let input = cx.new(|cx| {
            let mut s = InputState::new(window, cx);
            s.set_value(&initial, window, cx);
            s.set_placeholder("Tab name", window, cx);
            s
        });

        cx.subscribe_in(&input, window, {
            move |this, input_entity, event: &InputEvent, _window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    let value = input_entity.read(cx).value().to_string();
                    let value = value.trim();
                    let label = if value.is_empty() {
                        None
                    } else {
                        Some(value.to_string())
                    };
                    cx.emit(SidebarRename { index, label });
                    this.rename = None;
                    cx.notify();
                }
                _ => {}
            }
        })
        .detach();

        input.update(cx, |state, cx| state.focus(window, cx));
        self.rename = Some(RenameState { index, input });
        cx.notify();
    }

    fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if self.rename.take().is_some() {
            cx.notify();
        }
    }

    /// Workspace-facing helper: start inline rename (deferred so the
    /// triggering popup menu has a frame to dismiss before our input
    /// claims focus).
    pub fn start_rename(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.mode, PanelMode::Collapsed) {
            self.set_pinned(true, cx);
        }
        cx.defer_in(window, move |this, window, cx| {
            this.begin_rename(index, window, cx);
        });
    }

    fn render_rail(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = cx.theme();
        let rail_bg = surface_tone(theme, 0.10);
        let session_count = self.sessions.len();
        let mut rail = div()
            .id("vertical-tabs-rail")
            .relative()
            .w(px(RAIL_WIDTH))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(self.leading_top_pad))
            .pb(px(8.0))
            .gap(px(RAIL_ICON_GAP))
            .bg(rail_bg)
            // Mouse-leave on the rail container clears the hover card
            // even when the cursor exits via a fast diagonal motion
            // that may skip the per-icon hover transitions.
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered && this.hovered_rail.take().is_some() {
                    cx.notify();
                }
            }))
            // Container-level drag-move so the drop-target indicator
            // tracks the cursor even when it drifts off the 32px
            // pill into the 2-px gap between pills (or just brushes
            // the pill's rounded corner). The per-pill on_drag_move
            // handlers below give the same feedback while the cursor
            // is squarely on a pill; this fallback covers the gaps.
            .on_drag_move::<DraggedTab>(cx.listener(
                move |this, event: &gpui::DragMoveEvent<DraggedTab>, _, cx| {
                    if let Some(target) =
                        rail_row_for_cursor(event, session_count, this.leading_top_pad)
                    {
                        if this.drop_target != Some(target) {
                            this.drop_target = Some(target);
                            this.hovered_rail = None;
                            cx.notify();
                        }
                    }
                },
            ))
            // Container-level on_drop fires when the cursor is
            // anywhere inside the rail's bounds at mouseup —
            // including the gaps between pills. Without this the
            // user has to land the cursor precisely on a 32×32 pill
            // to reorder, which is unforgiving on a 44-px rail.
            .on_drop(cx.listener(move |this, dragged: &DraggedTab, window, cx| {
                let from = dragged.index;
                let to = this
                    .drop_target
                    .or_else(|| {
                        rail_row_for_cursor_position(
                            window.mouse_position(),
                            session_count,
                            this.leading_top_pad,
                        )
                    })
                    .unwrap_or(from);
                this.drop_target = None;
                if from != to {
                    cx.emit(SidebarReorder { from, to });
                }
                cx.notify();
            }))
            .child(rail_icon_button(
                "vertical-tabs-rail-new",
                "phosphor/plus.svg",
                theme.muted_foreground,
                theme,
                cx.listener(|_, _, _, cx| cx.emit(NewSession)),
            ))
            .child(
                div()
                    .h(px(1.0))
                    .w(px(20.0))
                    .my(px(2.0))
                    .bg(theme.muted_foreground.opacity(0.18)),
            );

        for (i, session) in self.sessions.iter().enumerate() {
            let is_active = i == self.active_session;
            let active_bg = surface_tone(theme, 0.22);
            let hover_bg = surface_tone(theme, 0.08);
            let drop_bg = surface_tone(theme, 0.18);
            let is_drop_target =
                self.drop_target == Some(i) && cx.has_active_drag();
            let dragged = DraggedTab {
                index: i,
                label: session.name.clone().into(),
                icon: session.icon,
            };

            let mut pill = div()
                .id(SharedString::from(format!("rail-tab-{i}")))
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .size(px(RAIL_ICON_SIZE))
                .rounded(px(8.0))
                .cursor_pointer()
                .bg(if is_drop_target {
                    drop_bg
                } else if is_active {
                    active_bg
                } else {
                    gpui::transparent_black()
                })
                .hover(move |s| if is_active { s } else { s.bg(hover_bg) })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        if let Some(state) = &this.rename {
                            if state.index != i {
                                this.rename = None;
                            }
                        }
                        cx.emit(SidebarSelect { index: i });
                    }),
                )
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |_this, _, _, cx| cx.emit(SidebarCloseTab { index: i })),
                )
                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                    let want = if *hovered { Some(i) } else { None };
                    if this.hovered_rail != want {
                        this.hovered_rail = want;
                        cx.notify();
                    }
                }))
                .on_drag(dragged, |dragged, _offset, _window, cx| {
                    cx.new(|_| dragged.clone())
                })
                .on_drag_move::<DraggedTab>(cx.listener(
                    move |this, event: &gpui::DragMoveEvent<DraggedTab>, _, cx| {
                        // GPUI fires drag_move on EVERY listener of a
                        // matching type — filter by element bounds.
                        if !point_in_bounds(&event.event.position, &event.bounds) {
                            return;
                        }
                        if this.drop_target != Some(i) {
                            this.drop_target = Some(i);
                            // Hide the hover card while a drag is in
                            // flight — it would just clutter the
                            // drop-target indicator.
                            this.hovered_rail = None;
                            cx.notify();
                        }
                    },
                ))
                .on_drop(cx.listener(move |this, dragged: &DraggedTab, _, cx| {
                    let from = dragged.index;
                    let to = i;
                    this.drop_target = None;
                    if from != to {
                        cx.emit(SidebarReorder { from, to });
                    }
                    cx.notify();
                }))
                .child(
                    svg()
                        .path(session.icon)
                        .size(px(16.0))
                        .text_color(if is_active {
                            theme.foreground
                        } else {
                            theme.muted_foreground.opacity(0.78)
                        }),
                );

            if session.needs_attention && !is_active {
                pill = pill.child(
                    div()
                        .absolute()
                        .top(px(3.0))
                        .right(px(3.0))
                        .size(px(6.0))
                        .rounded_full()
                        .bg(theme.primary),
                );
            }
            if is_drop_target {
                // Primary-color tick on the leading edge so the drop
                // target is unambiguous during a drag (the rail is
                // too narrow for a horizontal indicator line of the
                // kind the pinned panel uses).
                pill = pill.child(
                    div()
                        .absolute()
                        .left(px(-6.0))
                        .top(px(8.0))
                        .bottom(px(8.0))
                        .w(px(3.0))
                        .rounded(px(2.0))
                        .bg(theme.primary),
                );
            }

            rail = rail.child(pill);
        }

        rail = rail.child(div().flex_1());
        rail.child(rail_icon_button(
            "vertical-tabs-rail-expand",
            "phosphor/sidebar-simple.svg",
            theme.muted_foreground.opacity(0.7),
            theme,
            cx.listener(|this, _, _, cx| this.toggle_pinned(cx)),
        ))
    }

    /// Floating hover card shown next to the hovered rail icon.
    /// Composed in workspace coordinates so it renders OVER the
    /// translucent terminal pane (instead of behind it).
    ///
    /// The card vertically tracks the cursor's current y so the user
    /// always sees the card beside their finger. Anchoring it to the
    /// icon's geometric center sounded cleaner but actually requires
    /// computing the rail layout from this side, which is brittle —
    /// the cursor IS the icon, and "follow the cursor" is the
    /// established Apple tooltip pattern (Finder, Mail, Safari).
    pub fn render_hover_card_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !matches!(self.mode, PanelMode::Collapsed) {
            return None;
        }
        let i = self.hovered_rail?;
        if self.drop_target.is_some() {
            return None;
        }
        let session = self.sessions.get(i)?;
        let theme = cx.theme();
        let bg = surface_tone(theme, 0.22);
        let edge = surface_tone(theme, 0.32);

        // Anchor the card vertically on the cursor — its row IS the
        // icon under the cursor by construction.
        let cursor = window.mouse_position();
        let card_height = if session.subtitle.is_some() { 64.0 } else { 44.0 };
        let top = (f32::from(cursor.y) - card_height / 2.0).max(self.leading_top_pad);

        let name_color = theme.foreground;
        let sub_color = theme.muted_foreground.opacity(0.65);
        let meta_color = theme.muted_foreground.opacity(0.50);
        let mono_font = theme.mono_font_family.clone();
        let sys_font = theme.font_family.clone();

        let mut card_inner = div()
            .px(px(12.0))
            .py(px(8.0))
            .child(
                div()
                    .text_size(px(12.5))
                    .line_height(px(16.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(name_color)
                    .font_family(sys_font)
                    .truncate()
                    .child(session.name.clone()),
            );

        if let Some(sub) = session.subtitle.as_ref() {
            card_inner = card_inner.child(
                div()
                    .mt(px(2.0))
                    .text_size(px(11.0))
                    .line_height(px(14.0))
                    .text_color(sub_color)
                    .font_family(mono_font)
                    .truncate()
                    .child(sub.clone()),
            );
        }

        let mut meta = div()
            .mt(px(if session.subtitle.is_some() { 4.0 } else { 2.0 }))
            .flex()
            .items_center()
            .gap(px(8.0))
            .text_size(px(10.5))
            .line_height(px(13.0))
            .text_color(meta_color);

        let pane_label = match session.pane_count {
            0 | 1 => "1 pane".to_string(),
            n => format!("{n} panes"),
        };
        meta = meta.child(div().child(pane_label));
        if session.is_ssh {
            meta = meta.child(div().child("·").text_color(meta_color));
            meta = meta.child(div().child("SSH"));
        }
        if session.needs_attention {
            meta = meta
                .child(div().child("·").text_color(meta_color))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .child(div().size(px(6.0)).rounded_full().bg(theme.primary))
                        .child(div().child("unread")),
                );
        }
        card_inner = card_inner.child(meta);

        let card = div()
            .id("vertical-tabs-hover-card")
            .absolute()
            .top(px(top))
            .left(px(RAIL_WIDTH + 6.0))
            .w(px(HOVER_CARD_WIDTH))
            .rounded(px(8.0))
            .bg(bg)
            .occlude()
            .child(card_inner)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(-3.0))
                    .h_full()
                    .w(px(3.0))
                    .rounded(px(2.0))
                    .bg(edge),
            );

        Some(card.into_any_element())
    }

    fn render_panel_body(
        &mut self,
        is_overlay: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let body_bg;
        let header_color;
        let header_font;
        let new_btn;
        let toggle_btn;
        {
            let theme = cx.theme();
            body_bg = surface_tone(theme, 0.18);
            header_color = theme.muted_foreground.opacity(0.55);
            header_font = theme.font_family.clone();
            new_btn = panel_icon_button(
                "vertical-tabs-panel-new",
                "phosphor/plus.svg",
                theme,
                cx.listener(|_this, _, _, cx| cx.emit(NewSession)),
            );
            toggle_btn = panel_icon_button(
                if is_overlay {
                    "vertical-tabs-overlay-pin"
                } else {
                    "vertical-tabs-panel-collapse"
                },
                "phosphor/sidebar-simple.svg",
                theme,
                cx.listener(|this, _, _, cx| this.toggle_pinned(cx)),
            );
        }

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(36.0))
            .px(px(12.0))
            .pt(px(self.leading_top_pad.max(0.0)))
            .child(
                div()
                    .text_size(px(10.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(header_color)
                    .font_family(header_font)
                    .child(format!("{} TABS", self.sessions.len())),
            )
            .child(div().flex().gap(px(2.0)).child(new_btn).child(toggle_btn));

        let mut list = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .px(px(6.0))
            .pt(px(2.0))
            .gap(px(2.0));

        let renaming_index = self.rename.as_ref().map(|r| r.index);
        let rename_input = self.rename.as_ref().map(|r| r.input.clone());
        let drop_target = self.drop_target;

        let total = self.sessions.len();
        for i in 0..total {
            let session_clone = SessionEntry {
                name: self.sessions[i].name.clone(),
                subtitle: self.sessions[i].subtitle.clone(),
                is_ssh: self.sessions[i].is_ssh,
                needs_attention: self.sessions[i].needs_attention,
                icon: self.sessions[i].icon,
                has_user_label: self.sessions[i].has_user_label,
                pane_count: self.sessions[i].pane_count,
            };
            let row = self.render_panel_row(
                i,
                &session_clone,
                renaming_index,
                rename_input.clone(),
                drop_target,
                total,
                window,
                cx,
            );
            list = list.child(row);
        }

        div()
            .flex()
            .flex_col()
            .h_full()
            .w(px(PANEL_WIDTH))
            .flex_shrink_0()
            .bg(body_bg)
            .child(header)
            .child(list)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_panel_row(
        &self,
        i: usize,
        session: &SessionEntry,
        renaming_index: Option<usize>,
        rename_input: Option<Entity<InputState>>,
        drop_target: Option<usize>,
        total: usize,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme_clone = cx.theme().clone();
        let theme = &theme_clone;

        let is_active = i == self.active_session;
        let is_renaming = renaming_index == Some(i);
        let is_drop_target = drop_target == Some(i) && drop_target != Some(self.active_session);

        let row_h = if session.subtitle.is_some() {
            ROW_HEIGHT
        } else {
            ROW_HEIGHT_NO_SUBTITLE
        };

        let label_block: AnyElement = if is_renaming {
            if let Some(input) = rename_input {
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&input).small().appearance(false))
                    .into_any_element()
            } else {
                div().flex_1().min_w_0().into_any_element()
            }
        } else {
            let name = session.name.clone();
            let subtitle = session.subtitle.clone();
            let mono = theme.mono_font_family.clone();
            let sys = theme.font_family.clone();
            let fg = if is_active {
                theme.foreground
            } else {
                theme.muted_foreground.opacity(0.92)
            };
            let sub_fg = theme.muted_foreground.opacity(0.55);
            let mut block = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(1.0))
                .child(
                    div()
                        .overflow_hidden()
                        .truncate()
                        .text_size(px(12.5))
                        .line_height(px(16.0))
                        .font_family(sys)
                        .text_color(fg)
                        .when(is_active, |s| s.font_weight(FontWeight::MEDIUM))
                        .child(name),
                );
            if let Some(sub) = subtitle {
                block = block.child(
                    div()
                        .overflow_hidden()
                        .truncate()
                        .text_size(px(10.5))
                        .line_height(px(14.0))
                        .font_family(mono)
                        .text_color(sub_fg)
                        .child(sub),
                );
            }
            block.into_any_element()
        };

        let row_group = SharedString::from(format!("panel-tab-row-{i}"));

        // Both action buttons hover-only on every row, including
        // active. Apple-quiet: visible on intent, hidden by default.
        let rename_btn = panel_icon_button_small(
            SharedString::from(format!("panel-tab-rename-{i}")),
            "phosphor/pencil-simple.svg",
            theme,
            cx.listener(move |this, _, window, cx| this.begin_rename(i, window, cx)),
        )
        .invisible()
        .group_hover(row_group.clone(), |s| s.visible());
        let close_btn = panel_icon_button_small(
            SharedString::from(format!("panel-tab-close-{i}")),
            "phosphor/x.svg",
            theme,
            cx.listener(move |_this, _, _, cx| cx.emit(SidebarCloseTab { index: i })),
        )
        .invisible()
        .group_hover(row_group.clone(), |s| s.visible());

        let drop_line = div()
            .absolute()
            .left(px(8.0))
            .right(px(8.0))
            .top(px(-1.0))
            .h(px(2.0))
            .rounded(px(1.0))
            .bg(if is_drop_target {
                theme.primary
            } else {
                gpui::transparent_black()
            });

        let row_bg = if is_active {
            theme.background
        } else {
            gpui::transparent_black()
        };
        let hover_bg = surface_tone(theme, 0.10);

        let dragged = DraggedTab {
            index: i,
            label: session.name.clone().into(),
            icon: session.icon,
        };

        let mut icon_stack = div().relative().flex_shrink_0().child(
            svg()
                .path(session.icon)
                .size(px(15.0))
                .text_color(if is_active {
                    theme.foreground
                } else {
                    theme.muted_foreground.opacity(0.78)
                }),
        );
        if session.needs_attention && !is_active {
            icon_stack = icon_stack.child(
                div()
                    .absolute()
                    .top(px(-2.0))
                    .right(px(-2.0))
                    .size(px(6.0))
                    .rounded_full()
                    .bg(theme.primary),
            );
        }

        let row = div()
            .id(SharedString::from(format!("panel-tab-{i}")))
            .group(row_group.clone())
            .relative()
            .flex()
            .items_center()
            .gap(px(8.0))
            .pl(px(10.0))
            .pr(px(4.0))
            .h(px(row_h))
            .rounded(px(8.0))
            .cursor_pointer()
            .bg(row_bg)
            .text_color(if is_active {
                theme.foreground
            } else {
                theme.muted_foreground.opacity(0.92)
            })
            .hover(move |s| if is_active { s } else { s.bg(hover_bg) })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    if let Some(state) = &this.rename {
                        if state.index != i {
                            this.rename = None;
                        }
                    }
                    cx.emit(SidebarSelect { index: i });
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |_this, _, _, cx| cx.emit(SidebarCloseTab { index: i })),
            )
            .on_double_click(cx.listener(move |this, _, window, cx| {
                this.begin_rename(i, window, cx);
            }))
            .on_drag(dragged, |dragged, _offset, _window, cx| {
                cx.new(|_| dragged.clone())
            })
            .on_drag_move::<DraggedTab>(cx.listener(
                move |this, event: &gpui::DragMoveEvent<DraggedTab>, _, cx| {
                    if !point_in_bounds(&event.event.position, &event.bounds) {
                        return;
                    }
                    if this.drop_target != Some(i) {
                        this.drop_target = Some(i);
                        cx.notify();
                    }
                },
            ))
            .on_drop(cx.listener(move |this, dragged: &DraggedTab, _, cx| {
                let from = dragged.index;
                let to = i;
                this.drop_target = None;
                if from != to {
                    cx.emit(SidebarReorder { from, to });
                }
                cx.notify();
            }))
            .context_menu({
                let total = total;
                let has_user_label = session.has_user_label;
                let weak = cx.weak_entity();
                move |menu, _window, _cx| {
                    build_row_context_menu(menu, weak.clone(), i, total, has_user_label)
                }
            })
            .child(drop_line)
            .child(icon_stack)
            .child(label_block)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .child(rename_btn)
                    .child(close_btn),
            );
        row.into_any_element()
    }
}

impl Render for SessionSidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Clear stale drop indicator after the drag completes — GPUI
        // doesn't expose an on_drag_end hook for the source element.
        if self.drop_target.is_some() && !cx.has_active_drag() {
            self.drop_target = None;
        }
        // Drive the width-tween animation frame.
        let _progress = self.width_motion.value(window);

        match self.mode {
            PanelMode::Pinned => {
                let t = self.width_motion.current().clamp(0.0, 1.0);
                let visible_w = RAIL_WIDTH + (PANEL_WIDTH - RAIL_WIDTH) * t;
                let panel = self.render_panel_body(false, window, cx);
                div()
                    .flex()
                    .h_full()
                    .w(px(visible_w))
                    .flex_shrink_0()
                    .overflow_hidden()
                    .child(panel)
                    .into_any_element()
            }
            PanelMode::Collapsed => self.render_rail(window, cx).into_any_element(),
        }
    }
}

/// Compose a surface color one perceptual step darker (light theme) or
/// lighter (dark theme) than `theme.background`. Foreground is forced
/// to a desaturated extreme-luminance overlay so the result reads
/// against any palette author's choice of `theme.background`.
fn surface_tone(theme: &gpui_component::Theme, intensity: f32) -> Hsla {
    let mut over = theme.foreground;
    over.s = 0.0;
    over.l = if theme.foreground.l < 0.5 { 0.0 } else { 1.0 };
    over.a = intensity.clamp(0.0, 1.0);
    theme.background.blend(over)
}

fn point_in_bounds(p: &gpui::Point<gpui::Pixels>, b: &gpui::Bounds<gpui::Pixels>) -> bool {
    p.x >= b.origin.x
        && p.x < b.origin.x + b.size.width
        && p.y >= b.origin.y
        && p.y < b.origin.y + b.size.height
}

/// Map the cursor's current y-position (during a rail drag) to the
/// 0-based row index of the closest pill. Used so the user can drop
/// anywhere in the rail's vertical column — including the 2-px gaps
/// between pills — without losing the drop target.
fn rail_row_for_cursor(
    event: &gpui::DragMoveEvent<DraggedTab>,
    session_count: usize,
    leading_top_pad: f32,
) -> Option<usize> {
    if !point_in_bounds(&event.event.position, &event.bounds) {
        return None;
    }
    rail_row_for_cursor_position_in_bounds(
        event.event.position,
        event.bounds,
        session_count,
        leading_top_pad,
    )
}

/// Same idea, but called from `on_drop` where we don't get a
/// `DragMoveEvent` — just the cursor position. The bounds aren't
/// available there either, so we have to reconstruct the rail's row
/// y-positions arithmetically. Returns `None` if the cursor isn't
/// inside any plausible row band.
fn rail_row_for_cursor_position(
    cursor: gpui::Point<gpui::Pixels>,
    session_count: usize,
    leading_top_pad: f32,
) -> Option<usize> {
    if session_count == 0 {
        return None;
    }
    // Mirror the rail layout in render_rail:
    //   pt(leading_top_pad)
    //   + new-tab button (28 px) + RAIL_ICON_GAP
    //   + separator h(1) + my(2)*2 = 5 px + RAIL_ICON_GAP
    //   + i * (RAIL_ICON_SIZE + RAIL_ICON_GAP)
    let header = leading_top_pad + 28.0 + RAIL_ICON_GAP + 5.0 + RAIL_ICON_GAP;
    let stride = RAIL_ICON_SIZE + RAIL_ICON_GAP;
    let y = f32::from(cursor.y);
    if y < header {
        return Some(0);
    }
    let raw = ((y - header) / stride).floor() as i64;
    if raw < 0 {
        return Some(0);
    }
    let idx = (raw as usize).min(session_count.saturating_sub(1));
    Some(idx)
}

fn rail_row_for_cursor_position_in_bounds(
    cursor: gpui::Point<gpui::Pixels>,
    bounds: gpui::Bounds<gpui::Pixels>,
    session_count: usize,
    leading_top_pad: f32,
) -> Option<usize> {
    if session_count == 0 {
        return None;
    }
    // Coordinate of the cursor relative to the rail's top edge.
    let local_y = f32::from(cursor.y - bounds.origin.y);
    let header = leading_top_pad + 28.0 + RAIL_ICON_GAP + 5.0 + RAIL_ICON_GAP;
    let stride = RAIL_ICON_SIZE + RAIL_ICON_GAP;
    if local_y < header {
        return Some(0);
    }
    let raw = ((local_y - header) / stride).floor() as i64;
    if raw < 0 {
        return Some(0);
    }
    let idx = (raw as usize).min(session_count.saturating_sub(1));
    Some(idx)
}

fn rail_icon_button<F>(
    id: &'static str,
    icon: &'static str,
    icon_color: Hsla,
    theme: &gpui_component::Theme,
    handler: F,
) -> Stateful<Div>
where
    F: Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
{
    let hover_bg = surface_tone(theme, 0.10);
    div()
        .id(id)
        .size(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .child(svg().path(icon).size(px(14.0)).text_color(icon_color))
        .on_mouse_down(MouseButton::Left, handler)
}

fn panel_icon_button<F>(
    id: &'static str,
    icon: &'static str,
    theme: &gpui_component::Theme,
    handler: F,
) -> Stateful<Div>
where
    F: Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
{
    let hover_bg = surface_tone(theme, 0.10);
    div()
        .id(id)
        .size(px(24.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .child(
            svg()
                .path(icon)
                .size(px(13.0))
                .text_color(theme.muted_foreground),
        )
        .on_mouse_down(MouseButton::Left, handler)
}

fn build_row_context_menu(
    menu: PopupMenu,
    weak: WeakEntity<SessionSidebar>,
    index: usize,
    total: usize,
    has_user_label: bool,
) -> PopupMenu {
    let mut menu = menu
        .item(PopupMenuItem::new("Rename").on_click({
            let weak = weak.clone();
            move |_, window, cx| {
                if let Some(entity) = weak.upgrade() {
                    entity.update(cx, |this, cx| this.start_rename(index, window, cx));
                }
            }
        }))
        .item(PopupMenuItem::new("Duplicate").on_click({
            let weak = weak.clone();
            move |_, _, cx| {
                if let Some(entity) = weak.upgrade() {
                    entity.update(cx, |_, cx| cx.emit(SidebarDuplicate { index }));
                }
            }
        }));
    if has_user_label {
        menu = menu.item(PopupMenuItem::new("Reset Name").on_click({
            let weak = weak.clone();
            move |_, _, cx| {
                if let Some(entity) = weak.upgrade() {
                    entity.update(cx, |_, cx| {
                        cx.emit(SidebarRename {
                            index,
                            label: None,
                        })
                    });
                }
            }
        }));
    }
    menu = menu.separator();
    if index > 0 {
        menu = menu.item(PopupMenuItem::new("Move Up").on_click({
            let weak = weak.clone();
            move |_, _, cx| {
                if let Some(entity) = weak.upgrade() {
                    entity.update(cx, |_, cx| {
                        cx.emit(SidebarReorder {
                            from: index,
                            to: index - 1,
                        })
                    });
                }
            }
        }));
    }
    if index + 1 < total {
        menu = menu.item(PopupMenuItem::new("Move Down").on_click({
            let weak = weak.clone();
            move |_, _, cx| {
                if let Some(entity) = weak.upgrade() {
                    entity.update(cx, |_, cx| {
                        cx.emit(SidebarReorder {
                            from: index,
                            to: index + 1,
                        })
                    });
                }
            }
        }));
    }
    menu = menu
        .separator()
        .item(PopupMenuItem::new("Close Tab").on_click({
            let weak = weak.clone();
            move |_, _, cx| {
                if let Some(entity) = weak.upgrade() {
                    entity.update(cx, |_, cx| cx.emit(SidebarCloseTab { index }));
                }
            }
        }));
    if total > 1 {
        menu = menu.item(PopupMenuItem::new("Close Other Tabs").on_click({
            let weak = weak.clone();
            move |_, _, cx| {
                if let Some(entity) = weak.upgrade() {
                    entity.update(cx, |_, cx| cx.emit(SidebarCloseOthers { index }));
                }
            }
        }));
    }
    menu
}

fn panel_icon_button_small<F>(
    id: SharedString,
    icon: &'static str,
    theme: &gpui_component::Theme,
    handler: F,
) -> Stateful<Div>
where
    F: Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
{
    let hover_bg = surface_tone(theme, 0.16);
    div()
        .id(id)
        .size(px(20.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.0))
        .cursor_pointer()
        .hover(move |s| s.bg(hover_bg))
        .child(
            svg()
                .path(icon)
                .size(px(11.0))
                .text_color(theme.muted_foreground.opacity(0.72)),
        )
        .on_mouse_down(MouseButton::Left, handler)
}
