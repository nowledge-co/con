use gpui::{Action, KeyDownEvent, Window};

/// Returns true when the key event is the first stroke of an active app
/// action binding. Terminal views use this to avoid forwarding configured
/// app shortcuts into the shell while still respecting user keybinding edits.
pub(crate) fn key_down_starts_action_binding(
    event: &KeyDownEvent,
    window: &Window,
    action: &dyn Action,
) -> bool {
    let typed = std::slice::from_ref(&event.keystroke);
    window
        .bindings_for_action(action)
        .iter()
        .any(|binding| binding.match_keystrokes(typed).is_some())
}
