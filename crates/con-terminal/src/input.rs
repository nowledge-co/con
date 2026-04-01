/// Encodes keyboard input into escape sequences for the PTY.
///
/// This handles the translation from GPUI key events to the byte sequences
/// that terminal applications expect.
pub struct InputEncoder;

impl InputEncoder {
    /// Encode a key press into bytes to send to the PTY.
    ///
    /// When `application_cursor_keys` is true (DECCKM mode 1 set),
    /// arrow keys send SS3 sequences (\x1bOA) instead of CSI (\x1b[A).
    /// This is required for vim, less, top, and other full-screen apps.
    pub fn encode_key(
        key: &str,
        modifiers: Modifiers,
        application_cursor_keys: bool,
    ) -> Option<Vec<u8>> {
        // Special keys
        let seq = match key {
            "enter" | "return" => b"\r".to_vec(),
            "tab" => {
                if modifiers.shift {
                    b"\x1b[Z".to_vec()
                } else {
                    b"\t".to_vec()
                }
            }
            "escape" => b"\x1b".to_vec(),
            "backspace" => {
                if modifiers.alt {
                    b"\x1b\x7f".to_vec()
                } else {
                    b"\x7f".to_vec()
                }
            }
            "delete" => b"\x1b[3~".to_vec(),
            "up" => encode_arrow(b'A', &modifiers, application_cursor_keys),
            "down" => encode_arrow(b'B', &modifiers, application_cursor_keys),
            "right" => encode_arrow(b'C', &modifiers, application_cursor_keys),
            "left" => encode_arrow(b'D', &modifiers, application_cursor_keys),
            "home" => {
                if application_cursor_keys {
                    b"\x1bOH".to_vec()
                } else {
                    b"\x1b[H".to_vec()
                }
            }
            "end" => {
                if application_cursor_keys {
                    b"\x1bOF".to_vec()
                } else {
                    b"\x1b[F".to_vec()
                }
            }
            "pageup" => b"\x1b[5~".to_vec(),
            "pagedown" => b"\x1b[6~".to_vec(),
            "f1" => b"\x1bOP".to_vec(),
            "f2" => b"\x1bOQ".to_vec(),
            "f3" => b"\x1bOR".to_vec(),
            "f4" => b"\x1bOS".to_vec(),
            "f5" => b"\x1b[15~".to_vec(),
            "f6" => b"\x1b[17~".to_vec(),
            "f7" => b"\x1b[18~".to_vec(),
            "f8" => b"\x1b[19~".to_vec(),
            "f9" => b"\x1b[20~".to_vec(),
            "f10" => b"\x1b[21~".to_vec(),
            "f11" => b"\x1b[23~".to_vec(),
            "f12" => b"\x1b[24~".to_vec(),
            "space" => {
                if modifiers.ctrl {
                    vec![0x00] // Ctrl+Space = NUL
                } else {
                    b" ".to_vec()
                }
            }
            _ => {
                // Single character
                if key.len() == 1 {
                    let ch = key.chars().next().unwrap();
                    if modifiers.ctrl {
                        // Ctrl+A through Ctrl+Z
                        if ch.is_ascii_lowercase() {
                            vec![(ch as u8) - b'a' + 1]
                        } else if ch.is_ascii_uppercase() {
                            vec![(ch as u8) - b'A' + 1]
                        } else {
                            return None;
                        }
                    } else if modifiers.alt {
                        let mut bytes = vec![0x1b];
                        bytes.extend_from_slice(ch.to_string().as_bytes());
                        bytes
                    } else {
                        ch.to_string().into_bytes()
                    }
                } else {
                    // Multi-char string (e.g. pasted text)
                    return Some(key.as_bytes().to_vec());
                }
            }
        };

        Some(seq)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub cmd: bool,
}

fn encode_arrow(arrow: u8, mods: &Modifiers, application_cursor_keys: bool) -> Vec<u8> {
    let modifier_code = modifier_param(mods);
    if modifier_code > 1 {
        // Modified arrows always use CSI format
        format!("\x1b[1;{}{}", modifier_code, arrow as char).into_bytes()
    } else if application_cursor_keys {
        // DECCKM: unmodified arrows use SS3 (\x1bO)
        vec![0x1b, b'O', arrow]
    } else {
        vec![0x1b, b'[', arrow]
    }
}

fn modifier_param(mods: &Modifiers) -> u8 {
    let mut code: u8 = 1;
    if mods.shift {
        code += 1;
    }
    if mods.alt {
        code += 2;
    }
    if mods.ctrl {
        code += 4;
    }
    code
}

impl InputEncoder {
    /// Encode a key using the Kitty keyboard protocol (CSI u format).
    /// Returns None if the key should fall through to standard encoding.
    pub fn encode_key_kitty(key: &str, modifiers: Modifiers) -> Option<Vec<u8>> {
        let keycode = kitty_keycode(key)?;
        let mods = modifier_param(&modifiers);

        if mods > 1 {
            Some(format!("\x1b[{};{}u", keycode, mods).into_bytes())
        } else {
            Some(format!("\x1b[{}u", keycode).into_bytes())
        }
    }
}

/// Map GPUI key names to Kitty protocol key codes (Unicode codepoints)
fn kitty_keycode(key: &str) -> Option<u32> {
    match key {
        "escape" => Some(27),
        "enter" | "return" => Some(13),
        "tab" => Some(9),
        "backspace" => Some(127),
        "delete" => Some(57423),
        "insert" => Some(57425),
        "left" => Some(57419),
        "right" => Some(57421),
        "up" => Some(57417),
        "down" => Some(57420),
        "pageup" => Some(57422),
        "pagedown" => Some(57424),
        "home" => Some(57418),
        "end" => Some(57416),
        "space" => Some(32),
        "f1" => Some(57364),
        "f2" => Some(57365),
        "f3" => Some(57366),
        "f4" => Some(57367),
        "f5" => Some(57368),
        "f6" => Some(57369),
        "f7" => Some(57370),
        "f8" => Some(57371),
        "f9" => Some(57372),
        "f10" => Some(57373),
        "f11" => Some(57374),
        "f12" => Some(57375),
        _ => {
            // Single character — use Unicode codepoint
            if key.len() == 1 {
                key.chars().next().map(|c| c as u32)
            } else {
                None
            }
        }
    }
}
