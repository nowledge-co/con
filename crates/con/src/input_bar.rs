use gpui::*;

use crate::theme::Theme;

// Action emitted when user presses Enter
actions!(input_bar, [SubmitInput, EscapeInput, CycleMode, FocusInput]);

/// The smart input bar — detects NLP vs shell commands, shows skill completions
pub struct InputBar {
    content: String,
    focus_handle: FocusHandle,
    mode: InputMode,
}

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
}

impl InputBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            content: String::new(),
            focus_handle: cx.focus_handle(),
            mode: InputMode::Smart,
        }
    }

    /// Take the current content, clearing the input bar
    pub fn take_content(&mut self) -> String {
        std::mem::take(&mut self.content)
    }

    pub fn mode(&self) -> InputMode {
        self.mode
    }

    fn mode_indicator(&self) -> (&str, u32) {
        match self.mode {
            InputMode::Smart => ("~", Theme::overlay0()),
            InputMode::Shell => ("$", Theme::green()),
            InputMode::Agent => ("@", Theme::blue()),
        }
    }

    fn placeholder(&self) -> &str {
        match self.mode {
            InputMode::Smart => "Type a command or ask AI... (/ for skills)",
            InputMode::Shell => "Shell command...",
            InputMode::Agent => "Ask the AI agent...",
        }
    }

    fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        // Don't handle Cmd+key (let those bubble up to workspace)
        if event.keystroke.modifiers.platform {
            return;
        }

        match event.keystroke.key.as_str() {
            "tab" => {
                self.mode = self.mode.next();
                cx.notify();
            }
            "enter" => {
                if !self.content.trim().is_empty() {
                    cx.emit(SubmitInput);
                }
            }
            "escape" => {
                // Clear content and notify (parent can handle re-focusing terminal)
                self.content.clear();
                cx.emit(EscapeInput);
                cx.notify();
            }
            "backspace" => {
                self.content.pop();
                cx.notify();
            }
            key if key.len() == 1 => {
                let ch = if event.keystroke.modifiers.shift {
                    key.to_uppercase()
                } else {
                    key.to_string()
                };
                self.content.push_str(&ch);
                cx.notify();
            }
            "space" => {
                self.content.push(' ');
                cx.notify();
            }
            _ => {}
        }
    }
}

impl EventEmitter<SubmitInput> for InputBar {}
impl EventEmitter<EscapeInput> for InputBar {}

impl Focusable for InputBar {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for InputBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (indicator, indicator_color) = self.mode_indicator();
        let placeholder = self.placeholder().to_string();
        let is_focused = self.focus_handle.is_focused(window);

        let text_child: Div = if self.content.is_empty() {
            div()
                .flex_1()
                .text_sm()
                .text_color(rgb(Theme::overlay0()))
                .child(placeholder)
        } else {
            div()
                .flex_1()
                .text_sm()
                .text_color(rgb(Theme::text()))
                .child(format!(
                    "{}{}",
                    self.content,
                    if is_focused { "▎" } else { "" }
                ))
        };

        div()
            .flex()
            .flex_col()
            .bg(rgb(Theme::mantle()))
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                this.handle_key(event, cx);
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, window, _cx| {
                window.focus(&this.focus_handle);
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(44.0))
                    .px(px(16.0))
                    .gap(px(8.0))
                    // Focus ring on left border
                    .border_l_2()
                    .border_color(rgb(if is_focused {
                        Theme::blue()
                    } else {
                        Theme::mantle() // invisible when not focused
                    }))
                    // Mode indicator
                    .child(
                        div()
                            .w(px(20.0))
                            .text_sm()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(indicator_color))
                            .child(indicator.to_string()),
                    )
                    // Input text
                    .child(text_child)
                    // Mode hint
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(Theme::surface2()))
                            .child("Tab: mode · Enter: send · Esc: terminal"),
                    ),
            )
    }
}
