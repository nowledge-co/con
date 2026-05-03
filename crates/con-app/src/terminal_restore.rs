use gpui::KeyDownEvent;

pub(crate) fn restored_terminal_output(lines: Option<&[String]>) -> Option<Vec<u8>> {
    con_ghostty::restored_terminal_output_text(lines?).map(String::into_bytes)
}

pub(crate) fn key_down_may_write_terminal(event: &KeyDownEvent, special_key_writes: bool) -> bool {
    let keystroke = &event.keystroke;
    if keystroke.modifiers.platform {
        return false;
    }

    if keystroke.modifiers.control
        && keystroke.modifiers.shift
        && !keystroke.modifiers.alt
        && matches!(keystroke.key.as_str(), "v")
    {
        return true;
    }

    special_key_writes
        || (keystroke.modifiers.control
            && !keystroke.modifiers.alt
            && !keystroke.modifiers.shift
            && keystroke.key.len() == 1)
        || (keystroke.modifiers.alt && !keystroke.modifiers.control)
        || (keystroke.key.len() == 1 && (keystroke.modifiers.control || keystroke.modifiers.alt))
}
