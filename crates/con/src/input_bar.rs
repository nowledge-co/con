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

    fn indicator(&self) -> &str {
        match self {
            Self::Smart => "~",
            Self::Shell => "$",
            Self::Agent => "@",
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

/// The smart input bar — composable input with mode switching
pub struct InputBar {
    input_state: Entity<InputState>,
    mode: InputMode,
    _subscriptions: Vec<Subscription>,
}

impl InputBar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Type a command or ask AI...")
        });

        let _subscriptions = vec![
            // Listen for Enter key
            cx.subscribe_in(&input_state, window, {
                let input_state = input_state.clone();
                move |_this, _, ev: &InputEvent, _window, cx| {
                    match ev {
                        InputEvent::PressEnter { secondary: false } => {
                            let value = input_state.read(cx).value();
                            if !value.trim().is_empty() {
                                cx.emit(SubmitInput);
                            }
                        }
                        _ => {}
                    }
                }
            }),
        ];

        Self {
            input_state,
            mode: InputMode::Smart,
            _subscriptions,
        }
    }

    /// Take the current content, clearing the input
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

    fn indicator_color(&self, cx: &App) -> Hsla {
        match self.mode {
            InputMode::Smart => cx.theme().muted_foreground,
            InputMode::Shell => cx.theme().success,
            InputMode::Agent => cx.theme().primary,
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
        let indicator = self.mode.indicator();
        let mode_label = self.mode.label();
        let indicator_color = self.indicator_color(cx);
        let theme = cx.theme();

        div()
            .flex()
            .flex_col()
            .bg(theme.title_bar)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                // Tab cycles modes (don't send to Input)
                if event.keystroke.key == "tab" && !event.keystroke.modifiers.shift {
                    this.mode = this.mode.next();
                    cx.notify();
                }
                // Escape clears and emits escape event
                if event.keystroke.key == "escape" {
                    cx.emit(EscapeInput);
                }
            }))
            // Status line (mode, cwd hint)
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(24.0))
                    .px(px(16.0))
                    .gap(px(8.0))
                    .border_b_1()
                    .border_color(theme.border)
                    // Mode pill
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .px(px(6.0))
                            .py(px(2.0))
                            .rounded(px(4.0))
                            .bg(indicator_color.opacity(0.15))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(indicator_color)
                                    .child(format!("{indicator} {mode_label}")),
                            ),
                    )
                    // CWD hint
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .child(
                                svg()
                                    .path("phosphor/folder.svg")
                                    .size(px(12.0))
                                    .text_color(theme.muted_foreground),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child("~"),
                            ),
                    )
                    .child(div().flex_1())
                    // Tab hint
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Tab: mode"),
                    ),
            )
            // Input row
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(44.0))
                    .px(px(16.0))
                    .gap(px(8.0))
                    // Input field
                    .child(
                        div().flex_1().child(
                            Input::new(&self.input_state)
                                .appearance(false)
                                .cleanable(false),
                        ),
                    )
                    // Send button (circular, Apple Messages style)
                    .child(
                        div()
                            .id("send-button")
                            .size(px(28.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .cursor_pointer()
                            .bg(theme.primary)
                            .hover(|s| s.bg(theme.primary_hover))
                            .child(
                                svg()
                                    .path("phosphor/arrow-up.svg")
                                    .size(px(16.0))
                                    .text_color(theme.primary_foreground),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|_this, _, _, cx| {
                                    cx.emit(SubmitInput);
                                }),
                            ),
                    ),
            )
    }
}
