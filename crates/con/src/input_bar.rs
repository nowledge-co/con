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

    pub fn input_state(&self) -> &Entity<InputState> {
        &self.input_state
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
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(44.0))
                    .px(px(16.0))
                    .gap(px(8.0))
                    // Mode indicator
                    .child(
                        div()
                            .w(px(20.0))
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(indicator_color)
                            .child(indicator.to_string()),
                    )
                    // Input field (gpui-component Input)
                    .child(
                        div().flex_1().child(
                            Input::new(&self.input_state)
                                .appearance(false) // no border/background — we style the container
                                .cleanable(false),
                        ),
                    )
                    // Mode hint
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("Tab: mode ({mode_label})")),
                    ),
            )
    }
}
