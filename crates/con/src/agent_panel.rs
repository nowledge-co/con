use gpui::*;

use crate::theme::Theme;

/// The agent panel — shows conversation, reasoning steps, tool calls
pub struct AgentPanel {
    messages: Vec<PanelMessage>,
}

struct PanelMessage {
    role: String,
    content: String,
    steps: Vec<String>,
}

impl AgentPanel {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            messages: vec![PanelMessage {
                role: "system".to_string(),
                content: "con agent ready. Press Cmd+L to toggle this panel.".to_string(),
                steps: Vec::new(),
            }],
        }
    }

    pub fn add_message(&mut self, role: &str, content: &str, cx: &mut Context<Self>) {
        self.messages.push(PanelMessage {
            role: role.to_string(),
            content: content.to_string(),
            steps: Vec::new(),
        });
        cx.notify();
    }

    pub fn add_step(&mut self, step: &str, cx: &mut Context<Self>) {
        if let Some(last) = self.messages.last_mut() {
            last.steps.push(step.to_string());
        }
        cx.notify();
    }
}

impl Render for AgentPanel {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut messages_container = div().flex().flex_col().flex_1().p(px(12.0)).gap(px(12.0));

        for msg in &self.messages {
            let (role_color, role_label) = match msg.role.as_str() {
                "user" => (Theme::blue(), "You"),
                "assistant" => (Theme::green(), "Agent"),
                _ => (Theme::overlay0(), "System"),
            };

            let mut msg_div = div().flex().flex_col().gap(px(4.0)).child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(role_color))
                    .child(role_label.to_string()),
            ).child(
                div()
                    .text_sm()
                    .text_color(rgb(Theme::text()))
                    .child(msg.content.clone()),
            );

            for step in &msg.steps {
                msg_div = msg_div.child(
                    div()
                        .ml(px(8.0))
                        .pl(px(8.0))
                        .border_l_2()
                        .border_color(rgb(Theme::surface1()))
                        .text_xs()
                        .text_color(rgb(Theme::subtext0()))
                        .child(step.clone()),
                );
            }

            messages_container = messages_container.child(msg_div);
        }

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(Theme::mantle()))
            // Header
            .child(
                div()
                    .flex()
                    .h(px(38.0))
                    .px(px(16.0))
                    .items_center()
                    .border_b_1()
                    .border_color(rgb(Theme::surface0()))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(Theme::text()))
                            .child("Agent"),
                    ),
            )
            // Messages
            .child(messages_container)
    }
}
