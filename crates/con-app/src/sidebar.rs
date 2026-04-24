//! Vertical tabs side panel.
//!
//! Toggle in *Settings → Appearance → Tabs → Vertical Tabs*.
//!
//! Three runtime states (Chrome-style):
//!
//! - **Collapsed** — narrow icon rail (~44 px). One smart icon per tab
//!   (terminal / globe / code / pulse / book-open / file-code) with
//!   the active tab highlighted by an opaque pill.
//! - **Hover-peek** — when collapsed, hovering the rail floats out the
//!   full panel (~240 px) above the terminal area as an absolute
//!   overlay. Mouse-leave returns to the rail. Does NOT reflow the
//!   terminal pane.
//! - **Pinned** — the panel sits in flow next to the terminal pane.
//!   Persisted across restart via `session.vertical_tabs_pinned`.
//!
//! Row anatomy (panel mode):
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │  ▎  ◉ vim README.md             ✎  ✕       │  ← active row
//! │     ⌐ ~/proj                                │
//! ├─────────────────────────────────────────────┤
//! │     ◉ con-cli e2e               ✕           │
//! │     ⌐ skills/con-cli-e2e                    │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! Visual rules:
//! - Name uses **system font** (`theme.font_family`) — tabs are named
//!   things, not commands.
//! - Subtitle uses **mono font** (`theme.mono_font_family`) — paths
//!   and `user@host` are technical detail.
//! - Active row gets a 3-px accent bar on the leading edge plus the
//!   elevated pill background (Apple's "selection" gesture).
//! - Close `X` is invisible until the row is hovered (or the row is
//!   active). Pencil affordance for inline rename ditto.
//!
//! Interactions:
//! - Click row → activate tab.
//! - Middle-click → close tab.
//! - Right-click → context menu (Rename / Duplicate / Move Up / Move
//!   Down / Close / Close Other Tabs).
//! - Double-click name → inline rename.
//! - Click pencil → inline rename.
//! - Click `X` → close tab.
//! - Click sidebar-toggle (rail bottom or panel header right) → pin /
//!   unpin.

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

/// Width of the always-visible icon rail in collapsed / hover-peek modes.
pub const RAIL_WIDTH: f32 = 44.0;
/// Width of the full panel in pinned mode and the hover-peek overlay.
pub const PANEL_WIDTH: f32 = 240.0;
/// Per-row height in pinned/peek mode (two-line layout — name + subtitle).
const ROW_HEIGHT: f32 = 44.0;
const ROW_HEIGHT_NO_SUBTITLE: f32 = 32.0;

/// Cubic bezier feel for the panel width animation.
const PANEL_TWEEN: Duration = Duration::from_millis(220);
/// Slightly faster for the peek slide-in.
const PEEK_TWEEN: Duration = Duration::from_millis(160);

/// One row in the vertical tabs panel.
pub struct SessionEntry {
    pub name: String,
    pub subtitle: Option<String>,
    pub is_ssh: bool,
    pub needs_attention: bool,
    pub icon: &'static str,
    pub has_user_label: bool,
}

/// Visual state of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelMode {
    Collapsed,
    Pinned,
}

/// Per-tab transient state — currently just the inline-rename input
/// state when the user is editing this row's label. Keyed by the
/// session/tab index. Cleared on commit / cancel / sync.
struct RenameState {
    index: usize,
    input: Entity<InputState>,
}

/// What the user is currently dragging from the panel.
#[derive(Clone)]
pub struct DraggedTab {
    pub index: usize,
    pub label: SharedString,
    pub icon: &'static str,
}

impl Render for DraggedTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        // A tiny floating chip the cursor carries while dragging — same
        // shape as a row but compact and elevated.
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
    hover_peek: bool,
    sessions: Vec<SessionEntry>,
    active_session: usize,
    leading_top_pad: f32,
    /// Smooth width animation as the panel collapses / expands /
    /// peeks. Drives the rendered width via `MotionValue::value`.
    /// 0.0 = rail-only, 1.0 = full pinned panel.
    width_motion: MotionValue,
    /// Same idea but for the peek overlay's slide-in.
    peek_motion: MotionValue,
    /// Inline rename — `Some` when the user is editing a label.
    rename: Option<RenameState>,
    /// Tab index the user is currently hovering as a drop target while
    /// a `DraggedTab` is in flight. Resets once the drop fires.
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
            hover_peek: false,
            sessions: Vec::new(),
            active_session: 0,
            leading_top_pad: if cfg!(target_os = "macos") { 36.0 } else { 6.0 },
            width_motion: MotionValue::new(0.0),
            peek_motion: MotionValue::new(0.0),
            rename: None,
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
            self.hover_peek = false;
            self.cancel_rename(cx);
            // Sync the animation target to the new mode. We don't
            // animate the *initial* set_pinned (constructor seeding
            // from session) because that would cause a visible
            // expand-on-launch.
            let target = if pinned { 1.0 } else { 0.0 };
            self.width_motion.set_target(target, PANEL_TWEEN);
            self.peek_motion.set_target(0.0, PEEK_TWEEN);
            cx.notify();
        }
    }

    pub fn is_pinned(&self) -> bool {
        matches!(self.mode, PanelMode::Pinned)
    }

    /// Width the panel currently occupies in the workspace flex row.
    /// During the pin / unpin tween this varies smoothly between
    /// `RAIL_WIDTH` and `PANEL_WIDTH`. The hover-peek overlay does NOT
    /// contribute to occupied width — it floats above the terminal.
    pub fn occupied_width(&self) -> f32 {
        let t = self.width_motion.current().clamp(0.0, 1.0);
        RAIL_WIDTH + (PANEL_WIDTH - RAIL_WIDTH) * t
    }

    pub fn toggle_pinned(&mut self, cx: &mut Context<Self>) {
        let now_pinned = !self.is_pinned();
        self.set_pinned(now_pinned, cx);
    }

    fn set_hover_peek(&mut self, peek: bool, cx: &mut Context<Self>) {
        if matches!(self.mode, PanelMode::Pinned) {
            return;
        }
        if self.hover_peek != peek {
            self.hover_peek = peek;
            self.peek_motion
                .set_target(if peek { 1.0 } else { 0.0 }, PEEK_TWEEN);
            if !peek {
                self.cancel_rename(cx);
            }
            cx.notify();
        }
    }

    /// Update the session list from workspace state.
    pub fn sync_sessions(
        &mut self,
        sessions: Vec<SessionEntry>,
        active: usize,
        cx: &mut Context<Self>,
    ) {
        // If the renamed tab no longer exists, drop the rename state.
        if let Some(state) = &self.rename {
            if state.index >= sessions.len() {
                self.rename = None;
            }
        }
        self.sessions = sessions;
        self.active_session = active;
        cx.notify();
    }

    /// Begin inline rename for the row at `index`. Workspace fires
    /// this from menu and double-click handlers via `start_rename()`.
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

        // Enter commits, Esc cancels. Blur is a no-op intentionally:
        // the menu dismiss cycle blurs the input before the user has
        // a chance to type, and committing on first blur would eat
        // every menu-driven rename. The active row's tab-click
        // handler explicitly cancels the rename if the user clicks
        // away to a different tab.
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

        // Focus the input. Selecting the existing text would be nice
        // but `InputState::select_all` is `pub(super)` in the
        // upstream — so for now the user just lands at the end of
        // the existing label and can Cmd+A if they want.
        input.update(cx, |state, cx| {
            state.focus(window, cx);
        });

        self.rename = Some(RenameState { index, input });
        cx.notify();
    }

    fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if self.rename.take().is_some() {
            cx.notify();
        }
    }

    /// Workspace-facing helper: start inline rename for a given index.
    /// Called from the workspace's Rename action wiring and from the
    /// row's context menu. We defer one frame so the popup menu has
    /// time to dismiss + return focus to the panel before we point
    /// focus at the new InputState — otherwise the menu's
    /// dismiss-on-confirm cycle eats the input's focus immediately.
    pub fn start_rename(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.mode, PanelMode::Collapsed) {
            self.set_pinned(true, cx);
        }
        cx.defer_in(window, move |this, window, cx| {
            this.begin_rename(index, window, cx);
        });
    }

    fn render_rail(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = cx.theme();
        let rail_bg = surface_tone(theme, 0.10);
        let mut rail = div()
            .id("vertical-tabs-rail")
            .w(px(RAIL_WIDTH))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .pt(px(self.leading_top_pad))
            .pb(px(8.0))
            .gap(px(2.0))
            .bg(rail_bg)
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.set_hover_peek(*hovered, cx);
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

            let mut pill = div()
                .id(SharedString::from(format!("rail-tab-{i}")))
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .size(px(32.0))
                .rounded(px(8.0))
                .cursor_pointer()
                .bg(if is_active {
                    active_bg
                } else {
                    gpui::transparent_black()
                })
                .hover(move |s| if is_active { s } else { s.bg(hover_bg) })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_this, _, _, cx| {
                        cx.emit(SidebarSelect { index: i });
                    }),
                )
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |_this, _, _, cx| {
                        cx.emit(SidebarCloseTab { index: i });
                    }),
                )
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

            rail = rail.child(pill);
        }

        let _ = window;
        rail = rail.child(div().flex_1());

        rail.child(rail_icon_button(
            "vertical-tabs-rail-expand",
            "phosphor/sidebar-simple.svg",
            theme.muted_foreground.opacity(0.7),
            theme,
            cx.listener(|this, _, _, cx| this.toggle_pinned(cx)),
        ))
    }

    fn render_panel_body(
        &mut self,
        is_overlay: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        // Clone every theme value we'll need *before* re-borrowing
        // `cx` to render rows (each row may take `&mut Context` for
        // `cx.listener`). Holding a `&Theme` across those calls is a
        // borrow conflict.
        let body_bg;
        let header_color;
        let header_font;
        let new_btn;
        let toggle_btn;
        {
            let theme = cx.theme();
            body_bg = surface_tone(theme, 0.18);
            header_color = theme.muted_foreground.opacity(0.62);
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
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(header_color)
                    .font_family(header_font)
                    .child(format!("TABS · {}", self.sessions.len())),
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
        // Snapshot every theme color we reference *before* taking any
        // `cx.listener` borrows; we'll lean on these locals through
        // the rest of the function.
        let theme_clone: gpui_component::Theme;
        {
            theme_clone = cx.theme().clone();
        }
        let theme = &theme_clone;
        let is_active = i == self.active_session;
        let is_renaming = renaming_index == Some(i);
        let is_drop_target = drop_target == Some(i) && drop_target != Some(self.active_session);

        let row_h = if session.subtitle.is_some() {
            ROW_HEIGHT
        } else {
            ROW_HEIGHT_NO_SUBTITLE
        };

        // Two-line title block: name (system font) + subtitle (mono).
        // Or, when renaming, the InputState entity inline.
        let label_block: AnyElement = if is_renaming {
            if let Some(input) = rename_input {
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        Input::new(&input)
                            .small()
                            .appearance(false),
                    )
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

        // Right-cluster: rename pencil (inactive rows hidden until
        // hover) + close X (ditto). On the active row, both are
        // always visible so the user has affordances at hand.
        let action_visible = is_active;
        let mut rename_btn = panel_icon_button_small(
            SharedString::from(format!("panel-tab-rename-{i}")),
            if session.has_user_label {
                "phosphor/pencil-simple.svg"
            } else {
                "phosphor/pencil-simple.svg"
            },
            theme,
            cx.listener(move |this, _, window, cx| {
                this.begin_rename(i, window, cx);
            }),
        );
        if !action_visible {
            rename_btn = rename_btn
                .invisible()
                .group_hover(row_group.clone(), |s| s.visible());
        }
        let mut close_btn = panel_icon_button_small(
            SharedString::from(format!("panel-tab-close-{i}")),
            "phosphor/x.svg",
            theme,
            cx.listener(move |_this, _, _, cx| {
                cx.emit(SidebarCloseTab { index: i });
            }),
        );
        if !action_visible {
            close_btn = close_btn
                .invisible()
                .group_hover(row_group.clone(), |s| s.visible());
        }

        // Active accent bar — Apple-style 3px leading rule.
        let accent_bar = div()
            .absolute()
            .left(px(0.0))
            .top(px(8.0))
            .bottom(px(8.0))
            .w(px(3.0))
            .rounded(px(2.0))
            .bg(if is_active {
                theme.primary
            } else {
                gpui::transparent_black()
            });

        // Drop indicator — a 2-px line at the top edge when this row
        // is the current drop target.
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

        let mut icon_stack = div()
            .relative()
            .flex_shrink_0()
            .child(
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
                    // Clicking a different row dismisses any pending
                    // rename. Same row stays in rename mode.
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
                cx.listener(move |_this, _, _, cx| {
                    cx.emit(SidebarCloseTab { index: i });
                }),
            )
            .on_double_click(cx.listener(move |this, _, window, cx| {
                this.begin_rename(i, window, cx);
            }))
            .on_drag(dragged, |dragged, _offset, _window, cx| {
                cx.new(|_| dragged.clone())
            })
            .on_drag_move::<DraggedTab>(cx.listener(
                move |this, event: &gpui::DragMoveEvent<DraggedTab>, _, cx| {
                    // GPUI fires on_drag_move on EVERY element with a
                    // matching listener whenever the cursor moves
                    // anywhere — not only when the cursor is over
                    // THIS element. Filter on the element's own
                    // bounds so drop_target reflects the row the
                    // cursor is actually over.
                    let p = event.event.position;
                    let b = event.bounds;
                    if p.x < b.origin.x
                        || p.x >= b.origin.x + b.size.width
                        || p.y < b.origin.y
                        || p.y >= b.origin.y + b.size.height
                    {
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
            .child(accent_bar)
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

impl SessionSidebar {
    /// Returns the absolute peek-overlay element when the panel is
    /// collapsed and the cursor is hovering the rail.
    pub fn render_peek_overlay(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !matches!(self.mode, PanelMode::Collapsed) {
            return None;
        }
        let progress = self.peek_motion.value(window);
        if progress < 0.01 && !self.hover_peek {
            return None;
        }
        let edge_color = surface_tone(cx.theme(), 0.20);
        let body = self.render_panel_body(true, window, cx);
        // Slide the overlay in from -8 px so the motion has a small
        // horizontal offset, not just an opacity fade.
        let slide = -8.0 * (1.0 - progress);
        Some(
            div()
                .id("vertical-tabs-peek-overlay")
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(RAIL_WIDTH + slide))
                .w(px(PANEL_WIDTH + 1.0))
                .opacity(progress)
                .occlude()
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    this.set_hover_peek(*hovered, cx);
                }))
                .child(body)
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .h_full()
                        .w(px(1.0))
                        .bg(edge_color),
                )
                .into_any_element(),
        )
    }
}

impl Render for SessionSidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // GPUI doesn't expose an `on_drag_end` hook for the source
        // element, so the only place we can robustly clear the drop
        // indicator after a drag-cancel (drop-on-no-target) is the
        // next render that observes `!cx.has_active_drag()`.
        if self.drop_target.is_some() && !cx.has_active_drag() {
            self.drop_target = None;
        }

        // Drive the width animation so the rail / pinned-panel widens
        // smoothly when the user toggles. We always render the rail
        // in flow; in pinned mode we additionally render the panel
        // body inline beside it (so the terminal area gets pushed
        // right by `occupied_width()` as the tween runs).
        let _progress = self.width_motion.value(window);

        match self.mode {
            PanelMode::Pinned => {
                // Width animation: while transitioning, the panel
                // grows from RAIL_WIDTH to PANEL_WIDTH. We render
                // the rail and clip the panel body to the current
                // width via overflow_hidden.
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

/// Compose a surface color one perceptual step darker (light theme)
/// or lighter (dark theme) than `theme.background`.
fn surface_tone(theme: &gpui_component::Theme, intensity: f32) -> Hsla {
    let mut over = theme.foreground;
    over.s = 0.0;
    over.l = if theme.foreground.l < 0.5 { 0.0 } else { 1.0 };
    over.a = intensity.clamp(0.0, 1.0);
    theme.background.blend(over)
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
