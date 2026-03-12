use gpui::*;

use crate::theme::Theme;

/// The smart input bar — detects NLP vs shell commands, shows skill completions
pub struct InputBar {
    content: String,
    focus_handle: FocusHandle,
    mode: InputMode,
    skills: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    Smart,
    Shell,
    Agent,
}

impl InputBar {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            content: String::new(),
            focus_handle: cx.focus_handle(),
            mode: InputMode::Smart,
            skills: vec![
                "explain".into(),
                "fix".into(),
                "commit".into(),
                "test".into(),
                "review".into(),
            ],
        }
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
}

impl Focusable for InputBar {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for InputBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (indicator, indicator_color) = self.mode_indicator();
        let placeholder = self.placeholder().to_string();

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
                .child(self.content.clone())
        };

        div()
            .flex()
            .flex_col()
            .bg(rgb(Theme::mantle()))
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
                            .child("Tab: switch mode"),
                    ),
            )
    }
}
