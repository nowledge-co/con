use gpui::*;
use gpui_component::ActiveTheme;

/// Entry representing a terminal session in the sidebar.
pub struct SessionEntry {
    pub name: String,
    pub is_ssh: bool,
}

/// Session sidebar — lists terminal sessions with collapse/expand.
pub struct SessionSidebar {
    collapsed: bool,
    sessions: Vec<SessionEntry>,
    active_session: usize,
}

/// Emitted when user clicks a session entry
pub struct SidebarSelect {
    pub index: usize,
}

/// Emitted when user clicks the new session button
pub struct NewSession;

impl EventEmitter<SidebarSelect> for SessionSidebar {}
impl EventEmitter<NewSession> for SessionSidebar {}

impl SessionSidebar {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            collapsed: false,
            sessions: vec![SessionEntry {
                name: "Terminal".to_string(),
                is_ssh: false,
            }],
            active_session: 0,
        }
    }

    pub fn toggle_collapsed(&mut self, cx: &mut Context<Self>) {
        self.collapsed = !self.collapsed;
        cx.notify();
    }

    /// Update session list from workspace tabs
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
}

impl Render for SessionSidebar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        if self.collapsed {
            return div()
                .w(px(44.0))
                .h_full()
                .bg(theme.sidebar)
                .flex()
                .flex_col()
                .items_center()
                .pt(px(44.0)) // align below traffic lights
                // Expand button
                .child(
                    div()
                        .id("sidebar-expand")
                        .size(px(28.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(6.0))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.secondary))
                        .child(
                            svg()
                                .path("phosphor/caret-right.svg")
                                .size(px(14.0))
                                .text_color(theme.muted_foreground),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.toggle_collapsed(cx)),
                        ),
                );
        }

        let mut session_list = div()
            .flex()
            .flex_col()
            .flex_1()
            .px(px(6.0))
            .pt(px(4.0))
            .gap(px(1.0));

        for (i, session) in self.sessions.iter().enumerate() {
            let is_active = i == self.active_session;
            let icon_path = if session.is_ssh {
                "phosphor/globe.svg"
            } else {
                "phosphor/terminal.svg"
            };

            // Truncate long session names
            let display_name = if session.name.len() > 20 {
                format!("{}…", &session.name[..session.name.floor_char_boundary(18)])
            } else {
                session.name.clone()
            };

            session_list = session_list.child(
                div()
                    .id(SharedString::from(format!("session-{i}")))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(8.0))
                    .h(px(32.0))
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .overflow_x_hidden()
                    .bg(if is_active {
                        theme.list_active
                    } else {
                        gpui::transparent_black()
                    })
                    .text_color(if is_active {
                        theme.foreground
                    } else {
                        theme.sidebar_foreground
                    })
                    .hover(|s| {
                        if is_active {
                            s
                        } else {
                            s.bg(theme.secondary)
                        }
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |_this, _, _, cx| {
                            cx.emit(SidebarSelect { index: i });
                        }),
                    )
                    .child(
                        svg()
                            .path(icon_path)
                            .size(px(14.0))
                            .flex_shrink_0()
                            .text_color(if is_active {
                                theme.primary
                            } else {
                                theme.muted_foreground
                            }),
                    )
                    .child(display_name),
            );
        }

        div()
            .w(px(200.0))
            .h_full()
            .bg(theme.sidebar)
            .flex()
            .flex_col()
            // Header — aligns with tab bar height
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(38.0))
                    .px(px(12.0))
                    .child(
                        div()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground.opacity(0.6))
                            .child("SESSIONS"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(2.0))
                            // New session button
                            .child(
                                div()
                                    .id("sidebar-new-session")
                                    .size(px(24.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(5.0))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.secondary))
                                    .child(
                                        svg()
                                            .path("phosphor/plus.svg")
                                            .size(px(13.0))
                                            .text_color(theme.muted_foreground),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|_this, _, _, cx| {
                                            cx.emit(NewSession);
                                        }),
                                    ),
                            )
                            // Collapse button
                            .child(
                                div()
                                    .id("sidebar-collapse")
                                    .size(px(24.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(5.0))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.secondary))
                                    .child(
                                        svg()
                                            .path("phosphor/arrows-in-simple.svg")
                                            .size(px(13.0))
                                            .text_color(theme.muted_foreground),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| this.toggle_collapsed(cx)),
                                    ),
                            ),
                    ),
            )
            // Session list
            .child(session_list)
    }
}
