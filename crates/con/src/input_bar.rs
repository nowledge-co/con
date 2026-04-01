use gpui::*;
use gpui_component::{
    input::{Input, InputEvent, InputState},
    ActiveTheme,
};

actions!(input_bar, [SubmitInput, EscapeInput]);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    Smart,
    Shell,
    Agent,
}

impl InputMode {
    fn next(self) -> Self {
        match self {
            Self::Smart => Self::Shell,
            Self::Shell => Self::Agent,
            Self::Agent => Self::Smart,
        }
    }

    fn label(&self) -> &str {
        match self {
            Self::Smart => "Auto",
            Self::Shell => "Shell",
            Self::Agent => "Agent",
        }
    }
}

/// Pane info for the pane selector
#[derive(Clone)]
pub struct PaneInfo {
    pub id: usize,
    pub name: String,
}

pub struct InputBar {
    input_state: Entity<InputState>,
    mode: InputMode,
    cwd: String,
    panes: Vec<PaneInfo>,
    selected_pane_ids: Vec<usize>,
    focused_pane_id: usize,
    _subscriptions: Vec<Subscription>,
}

impl InputBar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Type a command or ask AI...")
        });

        let _subscriptions = vec![
            cx.subscribe_in(&input_state, window, {
                let input_state = input_state.clone();
                move |_this, _, ev: &InputEvent, _window, cx| {
                    if let InputEvent::PressEnter { secondary: false } = ev {
                        let value = input_state.read(cx).value();
                        if !value.trim().is_empty() {
                            cx.emit(SubmitInput);
                        }
                    }
                }
            }),
        ];

        Self {
            input_state,
            mode: InputMode::Smart,
            cwd: "~".to_string(),
            panes: Vec::new(),
            selected_pane_ids: Vec::new(),
            focused_pane_id: 0,
            _subscriptions,
        }
    }

    pub fn take_content(&self, window: &mut Window, cx: &mut App) -> String {
        let value = self.input_state.read(cx).value().to_string();
        self.input_state.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        value
    }

    pub fn mode(&self) -> InputMode {
        self.mode
    }

    pub fn target_pane_ids(&self) -> Vec<usize> {
        if self.selected_pane_ids.is_empty() {
            vec![self.focused_pane_id]
        } else {
            self.selected_pane_ids.clone()
        }
    }

    pub fn set_cwd(&mut self, cwd: String) {
        self.cwd = cwd;
    }

    pub fn set_panes(&mut self, panes: Vec<PaneInfo>, focused_id: usize) {
        self.panes = panes;
        self.focused_pane_id = focused_id;
        let valid_ids: Vec<usize> = self.panes.iter().map(|p| p.id).collect();
        self.selected_pane_ids.retain(|id| valid_ids.contains(id));
    }

    pub fn cycle_mode(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.mode = self.mode.next();
        cx.notify();
    }

    fn toggle_pane_selection(&mut self, pane_id: usize, cx: &mut Context<Self>) {
        if let Some(pos) = self.selected_pane_ids.iter().position(|&id| id == pane_id) {
            self.selected_pane_ids.remove(pos);
        } else {
            self.selected_pane_ids.push(pane_id);
        }
        cx.notify();
    }

    fn mode_color(&self, cx: &App) -> Hsla {
        match self.mode {
            InputMode::Smart => cx.theme().muted_foreground,
            InputMode::Shell => cx.theme().success,
            InputMode::Agent => cx.theme().primary,
        }
    }

    #[allow(dead_code)]
    fn placeholder(&self) -> &str {
        match self.mode {
            InputMode::Smart => "Type a command or ask AI...",
            InputMode::Shell => "Type a shell command...",
            InputMode::Agent => "Ask the AI agent...",
        }
    }
}

impl EventEmitter<SubmitInput> for InputBar {}
impl EventEmitter<EscapeInput> for InputBar {}

impl Focusable for InputBar {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input_state.read(cx).focus_handle(cx).clone()
    }
}

impl Render for InputBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mode_label = self.mode.label().to_string();
        let mode_color = self.mode_color(cx);
        let theme = cx.theme();
        let has_multiple_panes = self.panes.len() > 1;
        let cwd = self.cwd.clone();
        let is_broadcast = self.selected_pane_ids.len() > 1;

        // All interactive controls share this height
        let control_h = 28.0;

        // Mode pill
        let mode_pill = div()
            .id("mode-pill")
            .flex()
            .items_center()
            .justify_center()
            .h(px(control_h))
            .px(px(10.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .bg(mode_color.opacity(0.10))
            .text_size(px(12.0))
            .font_weight(FontWeight::MEDIUM)
            .text_color(mode_color)
            .hover(|s| s.bg(mode_color.opacity(0.18)))
            .child(mode_label);

        // Pane pills — NO borders, only bg color
        let pane_area = if has_multiple_panes {
            let mut pills = div()
                .flex()
                .items_center()
                .gap(px(2.0))
                .h(px(control_h))
                .px(px(2.0))
                .rounded(px(6.0))
                .bg(theme.muted.opacity(0.15));

            for pane in &self.panes {
                let pane_id = pane.id;
                let is_target = if self.selected_pane_ids.is_empty() {
                    pane.id == self.focused_pane_id
                } else {
                    self.selected_pane_ids.contains(&pane.id)
                };

                let name = if pane.name.len() > 12 {
                    format!("{}…", &pane.name[..10])
                } else {
                    pane.name.clone()
                };

                let pill = div()
                    .id(SharedString::from(format!("pane-sel-{pane_id}")))
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(control_h - 4.0))
                    .px(px(8.0))
                    .rounded(px(4.0))
                    .text_size(px(11.0))
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .bg(if is_target {
                        theme.primary.opacity(0.2)
                    } else {
                        theme.transparent
                    })
                    .text_color(if is_target {
                        theme.primary
                    } else {
                        theme.muted_foreground
                    })
                    .hover(|s| if is_target {
                        s
                    } else {
                        s.bg(theme.muted.opacity(0.15))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.toggle_pane_selection(pane_id, cx);
                        }),
                    )
                    .child(name);

                pills = pills.child(pill);
            }

            // ALL badge — always occupies space, invisible when not broadcasting
            let all_badge = div()
                .flex()
                .items_center()
                .justify_center()
                .h(px(control_h - 4.0))
                .px(px(6.0))
                .rounded(px(4.0))
                .text_size(px(9.0))
                .font_weight(FontWeight::BOLD)
                .bg(if is_broadcast { theme.warning.opacity(0.12) } else { theme.transparent })
                .text_color(if is_broadcast { theme.warning } else { theme.transparent })
                .child("ALL");

            Some(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .child(pills)
                    .child(all_badge),
            )
        } else {
            None
        };

        // Send button — same height as controls
        let send_button = div()
            .id("send-button")
            .flex()
            .items_center()
            .justify_center()
            .h(px(control_h))
            .w(px(control_h))
            .rounded(px(6.0))
            .cursor_pointer()
            .bg(theme.primary)
            .hover(|s| s.bg(theme.primary_hover))
            .child(
                svg()
                    .path("phosphor/arrow-up.svg")
                    .size(px(14.0))
                    .text_color(theme.primary_foreground),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, _, _, cx| {
                    cx.emit(SubmitInput);
                }),
            );

        div()
            .flex()
            .flex_col()
            .bg(theme.title_bar)
            .on_key_down(cx.listener(|_this, event: &KeyDownEvent, _window, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(EscapeInput);
                }
            }))
            // Main row
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(48.0))
                    .px(px(12.0))
                    .gap(px(8.0))
                    .child(mode_pill)
                    .child(
                        div().flex_1().child(
                            Input::new(&self.input_state)
                                .appearance(false)
                                .cleanable(false),
                        ),
                    )
                    .children(pane_area)
                    .child(send_button),
            )
            // Status row — subtle
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(18.0))
                    .px(px(14.0))
                    .pb(px(4.0))
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.muted_foreground.opacity(0.4))
                            .child(cwd),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(theme.muted_foreground.opacity(0.35))
                            .child("↵ send"),
                    ),
            )
    }
}
