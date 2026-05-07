/// Map legacy terminal Ctrl-key aliases to the C0/DEL byte they emit.
///
/// This intentionally covers the classic byte layer only. Modern protocols
/// such as Kitty keyboard / modifyOtherKeys belong in libghostty-vt's key
/// encoder, not in a growing Rust-side terminal keyboard table.
pub fn ctrl_key_to_c0(key: &str) -> Option<u8> {
    let key = key.trim();
    if key.eq_ignore_ascii_case("space") {
        return Some(0x00);
    }

    let mut chars = key.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }

    match ch {
        '@' => Some(0x00),
        '2' => Some(0x00),
        '3' => Some(0x1b),
        '4' => Some(0x1c),
        '5' => Some(0x1d),
        '6' => Some(0x1e),
        '7' => Some(0x1f),
        '8' => Some(0x7f),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' => Some(0x1e),
        '_' => Some(0x1f),
        '/' => Some(0x1f),
        '~' => Some(0x1e),
        '?' => Some(0x7f),
        ch if ch.is_ascii_alphabetic() => Some(ch.to_ascii_uppercase() as u8 - b'@'),
        _ => None,
    }
}

pub fn ctrl_chord_to_c0(key: &str) -> Option<u8> {
    let key = key.trim();
    let lower = key.to_ascii_lowercase();
    for prefix in ["ctrl-", "control-", "c-"] {
        if lower.starts_with(prefix) {
            return ctrl_key_to_c0(&key[prefix.len()..]);
        }
    }
    None
}

#[cfg(any(target_os = "windows", target_os = "linux", test))]
pub fn ctrl_keystroke_to_c0(key: &str, key_char: Option<&str>, shift: bool) -> Option<u8> {
    if shift {
        return key_char
            .filter(|text| !text.chars().all(|ch| ch.is_ascii_alphabetic()))
            .and_then(ctrl_key_to_c0)
            .or_else(|| match key {
                "@" | "^" | "_" | "?" => ctrl_key_to_c0(key),
                _ => None,
            });
    }

    ctrl_key_to_c0(key).or_else(|| key_char.and_then(ctrl_key_to_c0))
}

#[cfg(test)]
mod tests {
    use super::{ctrl_chord_to_c0, ctrl_key_to_c0, ctrl_keystroke_to_c0};

    #[test]
    fn ctrl_key_maps_letters_to_c0() {
        assert_eq!(ctrl_key_to_c0("a"), Some(0x01));
        assert_eq!(ctrl_key_to_c0("Z"), Some(0x1a));
    }

    #[test]
    fn ctrl_key_maps_defined_ascii_punctuation_to_c0() {
        assert_eq!(ctrl_key_to_c0("space"), Some(0x00));
        assert_eq!(ctrl_key_to_c0("@"), Some(0x00));
        assert_eq!(ctrl_key_to_c0("2"), Some(0x00));
        assert_eq!(ctrl_key_to_c0("3"), Some(0x1b));
        assert_eq!(ctrl_key_to_c0("4"), Some(0x1c));
        assert_eq!(ctrl_key_to_c0("5"), Some(0x1d));
        assert_eq!(ctrl_key_to_c0("6"), Some(0x1e));
        assert_eq!(ctrl_key_to_c0("7"), Some(0x1f));
        assert_eq!(ctrl_key_to_c0("8"), Some(0x7f));
        assert_eq!(ctrl_key_to_c0("["), Some(0x1b));
        assert_eq!(ctrl_key_to_c0("\\"), Some(0x1c));
        assert_eq!(ctrl_key_to_c0("]"), Some(0x1d));
        assert_eq!(ctrl_key_to_c0("^"), Some(0x1e));
        assert_eq!(ctrl_key_to_c0("_"), Some(0x1f));
        assert_eq!(ctrl_key_to_c0("/"), Some(0x1f));
        assert_eq!(ctrl_key_to_c0("~"), Some(0x1e));
        assert_eq!(ctrl_key_to_c0("?"), Some(0x7f));
    }

    #[test]
    fn ctrl_key_rejects_undefined_ascii_punctuation() {
        assert_eq!(ctrl_key_to_c0("}"), None);
        assert_eq!(ctrl_key_to_c0("1"), None);
        assert_eq!(ctrl_key_to_c0("0"), None);
        assert_eq!(ctrl_key_to_c0("9"), None);
        assert_eq!(ctrl_key_to_c0("enter"), None);
    }

    #[test]
    fn ctrl_chord_accepts_terminal_spellings() {
        assert_eq!(ctrl_chord_to_c0("Ctrl-C"), Some(0x03));
        assert_eq!(ctrl_chord_to_c0("control-]"), Some(0x1d));
        assert_eq!(ctrl_chord_to_c0("C-\\"), Some(0x1c));
        assert_eq!(ctrl_chord_to_c0("C-/"), Some(0x1f));
        assert_eq!(ctrl_chord_to_c0("ctrl-2"), Some(0x00));
    }

    #[test]
    fn ctrl_keystroke_uses_shifted_key_char_without_breaking_tab_shortcuts() {
        assert_eq!(ctrl_keystroke_to_c0("6", Some("^"), true), Some(0x1e));
        assert_eq!(ctrl_keystroke_to_c0("-", Some("_"), true), Some(0x1f));
        assert_eq!(ctrl_keystroke_to_c0("`", Some("~"), true), Some(0x1e));
        assert_eq!(ctrl_keystroke_to_c0("a", Some("A"), true), None);
        assert_eq!(ctrl_keystroke_to_c0("]", Some("}"), true), None);
        assert_eq!(ctrl_keystroke_to_c0("]", None, false), Some(0x1d));
        assert_eq!(ctrl_keystroke_to_c0("/", Some("/"), false), Some(0x1f));
    }
}
