use gpui::{
    Action, AnyElement, AsKeystroke as _, Div, IntoElement as _, Keystroke, ParentElement as _,
    Styled as _, Window, div, px,
};

fn key_label(key: &str) -> String {
    match key {
        "space" => "Space".to_string(),
        "enter" => "Return".to_string(),
        "escape" => "Esc".to_string(),
        "backspace" => "Delete".to_string(),
        "delete" => "Del".to_string(),
        "tab" => "Tab".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "-" => "-".to_string(),
        "+" => "+".to_string(),
        key if key.chars().count() == 1 => key.to_uppercase(),
        key => key
            .split('-')
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn keycap(label: String, theme: &gpui_component::Theme) -> Div {
    let wide = label.chars().count() > 1;
    div()
        .h(px(19.0))
        .min_w(if wide { px(31.0) } else { px(19.0) })
        .px(px(if wide { 6.0 } else { 0.0 }))
        .rounded(px(5.0))
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.muted.opacity(0.16))
        .font_family(theme.mono_font_family.clone())
        .text_color(theme.foreground.opacity(0.78))
        .text_size(px(10.5))
        .line_height(px(11.0))
        .font_weight(gpui::FontWeight::MEDIUM)
        .child(label)
}

pub(crate) fn keycaps_for_stroke(stroke: &Keystroke, theme: &gpui_component::Theme) -> Div {
    let mut parts = Vec::new();
    if stroke.modifiers.control {
        parts.push("⌃".to_string());
    }
    if stroke.modifiers.alt {
        parts.push("⌥".to_string());
    }
    if stroke.modifiers.shift {
        parts.push("⇧".to_string());
    }
    if stroke.modifiers.platform {
        parts.push("⌘".to_string());
    }
    parts.push(key_label(&stroke.key));

    div()
        .flex()
        .items_center()
        .justify_end()
        .gap(px(2.0))
        .children(parts.into_iter().map(|part| keycap(part, theme)))
}

pub(crate) fn keycaps_for_binding(
    binding: &str,
    theme: &gpui_component::Theme,
) -> AnyElement {
    if let Ok(stroke) = Keystroke::parse(binding) {
        keycaps_for_stroke(&stroke, theme).into_any_element()
    } else {
        div()
            .text_size(px(11.0))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(theme.muted_foreground)
            .child(binding.to_string())
            .into_any_element()
    }
}

pub(crate) fn first_action_keystroke(action: &dyn Action, window: &Window) -> Option<Keystroke> {
    let binding = window.highest_precedence_binding_for_action(action)?;
    binding
        .keystrokes()
        .first()
        .map(|keystroke| keystroke.as_keystroke().clone())
}
