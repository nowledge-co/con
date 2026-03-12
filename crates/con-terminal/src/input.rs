/// Encodes keyboard input into escape sequences for the PTY.
///
/// This handles the translation from GPUI key events to the byte sequences
/// that terminal applications expect.
pub struct InputEncoder;

impl InputEncoder {
    /// Encode a key press into bytes to send to the PTY
    pub fn encode_key(key: &str, modifiers: Modifiers) -> Option<Vec<u8>> {
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
            "up" => encode_arrow(b'A', &modifiers),
            "down" => encode_arrow(b'B', &modifiers),
            "right" => encode_arrow(b'C', &modifiers),
            "left" => encode_arrow(b'D', &modifiers),
            "home" => b"\x1b[H".to_vec(),
            "end" => b"\x1b[F".to_vec(),
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

fn encode_arrow(arrow: u8, mods: &Modifiers) -> Vec<u8> {
    let modifier_code = modifier_param(mods);
    if modifier_code > 1 {
        format!("\x1b[1;{}{}", modifier_code, arrow as char).into_bytes()
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
