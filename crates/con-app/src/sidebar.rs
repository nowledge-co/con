//! Vertical tabs side panel (Chrome vertical-tabs design).
//!
//! When `appearance.tabs_orientation = "vertical"` is set, the workspace
//! hides the historical horizontal tab strip and instead renders this
//! panel along the leading edge of the main area. Two states:
//!
//! - **Collapsed** (default): a narrow icon rail (~44 px). Each tab is
//!   a single terminal icon stacked vertically. The active tab gets a
//!   foreground-tinted pill; tabs that have produced unread output get
//!   a small dot in the corner. A `+` sits at the top, an expand
//!   chevron at the bottom.
//!
//! - **Pinned** (expanded): a full panel (~220 px) with icon, full
//!   title, and a hover-X close affordance. The collapse chevron
//!   returns to the rail.
//!
//! - **Hover-peek** (in-between): when collapsed, hovering the rail
//!   floats out the expanded view *over* the terminal area as an
//!   overlay; mouse leave returns to the rail. Click the chevron to
//!   pin. This matches Chrome's behavior and means hovering does not
//!   re-flow the terminal area.
//!
//! The panel emits `SidebarSelect` and `NewSession` events to the
//! workspace, which translates them into `activate_tab` / `new_tab`.
//! Per-tab close uses `SidebarCloseTab`.

use gpui::*;
use gpui_component::ActiveTheme;

/// Width of the always-visible icon rail in collapsed / hover-peek modes.
pub const RAIL_WIDTH: f32 = 44.0;
/// Width of the full panel in pinned mode (and the hover-peek overlay).
pub const PANEL_WIDTH: f32 = 220.0;

/// One row in the vertical tabs panel — mirrors the workspace's
/// per-tab UI state. The workspace recomputes and pushes this list on
/// every change via `sync_sessions`.
pub struct SessionEntry {
    pub name: String,
    pub is_ssh: bool,
    pub needs_attention: bool,
}

/// Visual state of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelMode {
    /// Narrow icon rail; the terminal area owns the full leading edge.
    Collapsed,
    /// Full panel; the terminal area is shifted right.
    Pinned,
}

/// Vertical tabs side panel. Lives next to the terminal area when the
/// workspace is in vertical-tabs mode. Stays constructed even in
/// horizontal mode — workspace just doesn't add it to the tree —
/// so toggling orientation at runtime is cheap.
pub struct SessionSidebar {
    mode: PanelMode,
    hover_peek: bool,
    sessions: Vec<SessionEntry>,
    active_session: usize,
    /// macOS reserves the leading 78 px of the window for the
    /// traffic-light cluster the OS paints. When the panel covers the
    /// leading edge of the window, its top must therefore be padded
    /// past that cluster instead of starting at y=0. Workspace flips
    /// this on macOS so the panel stays a self-contained widget.
    leading_top_pad: f32,
}

/// Emitted when the user activates a tab from the panel.
pub struct SidebarSelect {
    pub index: usize,
}

/// Emitted when the user clicks the `+` to spawn a new tab.
pub struct NewSession;

/// Emitted when the user clicks the close affordance on a panel row.
pub struct SidebarCloseTab {
    pub index: usize,
}

impl EventEmitter<SidebarSelect> for SessionSidebar {}
impl EventEmitter<NewSession> for SessionSidebar {}
impl EventEmitter<SidebarCloseTab> for SessionSidebar {}

impl SessionSidebar {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            mode: PanelMode::Collapsed,
            hover_peek: false,
            sessions: Vec::new(),
            active_session: 0,
            leading_top_pad: if cfg!(target_os = "macos") { 36.0 } else { 6.0 },
        }
    }

    /// Restore the pinned state from a previous session.
    pub fn set_pinned(&mut self, pinned: bool, cx: &mut Context<Self>) {
        let new_mode = if pinned {
            PanelMode::Pinned
        } else {
            PanelMode::Collapsed
        };
        if self.mode != new_mode {
            self.mode = new_mode;
            self.hover_peek = false;
            cx.notify();
        }
    }

    /// Whether the panel is currently pinned open.
    pub fn is_pinned(&self) -> bool {
        matches!(self.mode, PanelMode::Pinned)
    }

    /// How wide the rail/panel currently *occupies* in the workspace
    /// flex row. The hover-peek overlay sits on top of the terminal
    /// area and does NOT contribute to occupied width — that's the
    /// whole point: hovering doesn't reflow the terminal.
    pub fn occupied_width(&self) -> f32 {
        match self.mode {
            PanelMode::Collapsed => RAIL_WIDTH,
            PanelMode::Pinned => PANEL_WIDTH,
        }
    }

    pub fn toggle_pinned(&mut self, cx: &mut Context<Self>) {
        self.mode = match self.mode {
            PanelMode::Pinned => PanelMode::Collapsed,
            PanelMode::Collapsed => PanelMode::Pinned,
        };
        // Pinning always clears the hover-peek; unpinning lets the user
        // hover again to peek before clicking elsewhere.
        self.hover_peek = false;
        cx.notify();
    }

    fn set_hover_peek(&mut self, peek: bool, cx: &mut Context<Self>) {
        if matches!(self.mode, PanelMode::Pinned) {
            return;
        }
        if self.hover_peek != peek {
            self.hover_peek = peek;
            cx.notify();
        }
    }

    /// Update session list from workspace tabs.
    pub fn sync_sessions(
        &mut self,
        sessions: Vec<SessionEntry>,
        active: usize,
        cx: &mut Context<Self>,
    ) {
        self.sessions = sessions;
        self.active_session = active;
        cx.notify();
    }

    fn render_rail(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
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
            // New tab button at the top.
            .child(rail_icon_button(
                "vertical-tabs-rail-new",
                "phosphor/plus.svg",
                theme.muted_foreground,
                theme,
                cx.listener(|_, _, _, cx| cx.emit(NewSession)),
            ))
            // Thin separator (opacity, not border).
            .child(
                div()
                    .h(px(1.0))
                    .w(px(20.0))
                    .my(px(2.0))
                    .bg(theme.muted_foreground.opacity(0.18)),
            );

        for (i, session) in self.sessions.iter().enumerate() {
            let is_active = i == self.active_session;
            let icon_path = if session.is_ssh {
                "phosphor/globe.svg"
            } else {
                "phosphor/terminal.svg"
            };

            let active_bg = theme.background;
            let hover_bg = surface_tone(theme, 0.06);

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
                .hover(|s| if is_active { s } else { s.bg(hover_bg) })
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
                        .path(icon_path)
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
                        .top(px(4.0))
                        .right(px(4.0))
                        .size(px(6.0))
                        .rounded_full()
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

    fn render_panel_body(
        &mut self,
        is_overlay: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let theme = cx.theme();

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
                    .text_color(theme.muted_foreground.opacity(0.62))
                    .child("TABS"),
            )
            .child(
                div()
                    .flex()
                    .gap(px(2.0))
                    .child(panel_icon_button(
                        "vertical-tabs-panel-new",
                        "phosphor/plus.svg",
                        theme,
                        cx.listener(|_this, _, _, cx| cx.emit(NewSession)),
                    ))
                    .child(panel_icon_button(
                        if is_overlay {
                            "vertical-tabs-overlay-pin"
                        } else {
                            "vertical-tabs-panel-collapse"
                        },
                        "phosphor/sidebar-simple.svg",
                        theme,
                        cx.listener(|this, _, _, cx| this.toggle_pinned(cx)),
                    )),
            );

        let mut list = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .px(px(6.0))
            .pt(px(2.0))
            .gap(px(1.0));

        for (i, session) in self.sessions.iter().enumerate() {
            let is_active = i == self.active_session;
            let icon_path = if session.is_ssh {
                "phosphor/globe.svg"
            } else {
                "phosphor/terminal.svg"
            };

            let display_name = if session.name.len() > 28 {
                let cut = session.name.floor_char_boundary(26);
                format!("{}…", &session.name[..cut])
            } else {
                session.name.clone()
            };

            let mut icon_stack = div()
                .relative()
                .flex_shrink_0()
                .child(
                    svg()
                        .path(icon_path)
                        .size(px(14.0))
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
                        .top(px(-1.0))
                        .right(px(-1.0))
                        .size(px(5.0))
                        .rounded_full()
                        .bg(theme.primary),
                );
            }

            let row_group = SharedString::from(format!("panel-tab-row-{i}"));

            let mut close_btn = div()
                .id(SharedString::from(format!("panel-tab-close-{i}")))
                .invisible()
                .group_hover(row_group.clone(), |s| s.visible())
                .size(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.0))
                .cursor_pointer()
                .text_color(theme.muted_foreground.opacity(0.55))
                .hover(|s| {
                    s.bg(theme.muted.opacity(0.16))
                        .text_color(theme.foreground)
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_this, _, _, cx| {
                        cx.emit(SidebarCloseTab { index: i });
                    }),
                )
                .child(svg().path("phosphor/x.svg").size(px(10.0)));
            if is_active {
                close_btn = close_btn.visible();
            }

            let row = div()
                .id(SharedString::from(format!("panel-tab-{i}")))
                .group(row_group)
                .flex()
                .items_center()
                .gap(px(8.0))
                .pl(px(8.0))
                .pr(px(4.0))
                .h(px(30.0))
                .rounded(px(7.0))
                .cursor_pointer()
                .text_size(px(12.0))
                .overflow_x_hidden()
                .bg(if is_active {
                    theme.background
                } else {
                    gpui::transparent_black()
                })
                .text_color(if is_active {
                    theme.foreground
                } else {
                    theme.muted_foreground
                })
                .hover(|s| {
                    if is_active {
                        s
                    } else {
                        s.bg(surface_tone(theme, 0.04))
                    }
                })
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
                .child(icon_stack)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .child(display_name),
                )
                .child(close_btn);
            list = list.child(row);
        }

        // Both the pinned panel and the hover-peek overlay use the
        // same elevated tone — slightly darker than the rail so the
        // tab list reads as a layered surface above the rail (Chrome
        // uses the same delta in vertical-tabs mode). Theme-agnostic
        // via `surface_tone`: blends a small amount of foreground into
        // background so we get a visible step on both light and dark
        // themes without picking a per-theme palette token that may
        // collapse into background on some themes.
        let body_bg = surface_tone(theme, 0.18);
        div()
            .flex()
            .flex_col()
            .h_full()
            .w(px(PANEL_WIDTH))
            .flex_shrink_0()
            .bg(body_bg)
            .font_family(theme.mono_font_family.clone())
            .child(header)
            .child(list)
    }
}

impl Render for SessionSidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The hover-peek overlay is rendered separately by the
        // workspace via `render_peek_overlay`. This function only
        // returns the in-flow piece of chrome (the rail in collapsed
        // mode, the full panel body in pinned mode) so the workspace
        // can stack the overlay above the terminal pane in the root
        // tree — embedding the overlay inside this entity's render
        // tree puts it BEHIND the (translucent) terminal pane, which
        // muddies the panel surface against the desktop backdrop.
        match self.mode {
            PanelMode::Pinned => self.render_panel_body(false, cx).into_any_element(),
            PanelMode::Collapsed => self.render_rail(cx).into_any_element(),
        }
    }
}

impl SessionSidebar {
    /// Returns the absolute peek-overlay element when the panel is
    /// collapsed *and* the cursor is hovering the rail, otherwise
    /// returns `None`. Workspace stacks the result on top of every
    /// other workspace child so the overlay reads clearly against
    /// the terminal pane behind it.
    pub fn render_peek_overlay(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !matches!(self.mode, PanelMode::Collapsed) || !self.hover_peek {
            return None;
        }
        let edge_color = surface_tone(cx.theme(), 0.18);
        let body = self.render_panel_body(true, cx);
        Some(
            div()
                .id("vertical-tabs-peek-overlay")
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(RAIL_WIDTH))
                .w(px(PANEL_WIDTH + 1.0))
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

/// Compose a surface color one perceptual step darker (light theme)
/// or lighter (dark theme) than `theme.background`, by blending the
/// theme's `foreground` into `background` at the requested intensity.
///
/// Light flexoki: `foreground` is near-black, so blending lowers
/// luminance — perfect for an "elevated" panel surface.
/// Dark flexoki: `foreground` is near-white, so blending raises
/// luminance — same outcome (a panel that visibly steps off the
/// terminal pane background).
///
/// Picking a fixed token like `theme.title_bar` or `theme.muted` was
/// the first attempt, but those tokens are theme-author choices and
/// some themes set them so close to `background` that the panel
/// vanished against the terminal pane.
fn surface_tone(theme: &gpui_component::Theme, intensity: f32) -> gpui::Hsla {
    // theme.foreground on light themes is a low-luminance ink; on dark
    // themes a high-luminance light. Either way, blending it into
    // `background` shifts the surface visibly off the terminal pane —
    // which is what we want, since the terminal pane uses the
    // **terminal theme's** background (Flexoki "paper") and not
    // necessarily the same color as the GPUI theme's `background`.
    //
    // Force a desaturated, extreme-luminance overlay so even themes
    // whose `foreground` is tinted (e.g. warm grays) still produce a
    // monochromatic step that reads on both sides.
    let mut over = theme.foreground;
    over.s = 0.0;
    over.l = if theme.foreground.l < 0.5 { 0.0 } else { 1.0 };
    over.a = intensity.clamp(0.0, 1.0);
    theme.background.blend(over)
}

fn rail_icon_button<F>(
    id: &'static str,
    icon: &'static str,
    icon_color: gpui::Hsla,
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
