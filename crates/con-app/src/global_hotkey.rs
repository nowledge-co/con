use con_core::config::KeybindingConfig;
use gpui::{App, AsyncApp, Keystroke};
use std::cell::RefCell;

unsafe extern "C" {
    fn con_register_global_hotkey(
        key_code: u32,
        shift: bool,
        control: bool,
        alt: bool,
        command: bool,
        callback: extern "C" fn(),
    ) -> bool;
    fn con_unregister_global_hotkey();
    fn con_register_quick_terminal_hotkey(
        key_code: u32,
        shift: bool,
        control: bool,
        alt: bool,
        command: bool,
        callback: extern "C" fn(),
    ) -> bool;
    fn con_unregister_quick_terminal_hotkey();
    fn con_app_is_active() -> bool;
}

thread_local! {
    static GLOBAL_HOTKEY_APP: RefCell<Option<AsyncApp>> = const { RefCell::new(None) };
    static HOTKEYS_SUSPENDED: RefCell<bool> = const { RefCell::new(false) };
}

pub fn init(cx: &App, keybindings: &KeybindingConfig) {
    GLOBAL_HOTKEY_APP.with(|app| {
        *app.borrow_mut() = Some(cx.to_async());
    });
    update_from_keybindings(keybindings);
}

pub fn update_from_keybindings(keybindings: &KeybindingConfig) {
    register_hotkey(
        keybindings.global_summon_enabled,
        &keybindings.global_summon,
        "global hotkey",
        || unsafe { con_unregister_global_hotkey() },
        |key_code, shift, control, alt, command, callback| unsafe {
            con_register_global_hotkey(key_code, shift, control, alt, command, callback)
        },
        on_global_hotkey_pressed,
    );
    register_hotkey(
        keybindings.quick_terminal_enabled,
        &keybindings.quick_terminal,
        "quick terminal",
        || unsafe { con_unregister_quick_terminal_hotkey() },
        |key_code, shift, control, alt, command, callback| unsafe {
            con_register_quick_terminal_hotkey(key_code, shift, control, alt, command, callback)
        },
        on_quick_terminal_pressed,
    );
}

fn register_hotkey(
    enabled: bool,
    binding: &str,
    label: &str,
    unregister: impl Fn(),
    register: impl Fn(u32, bool, bool, bool, bool, extern "C" fn()) -> bool,
    callback: extern "C" fn(),
) {
    if !enabled {
        unregister();
        return;
    }

    let Some(keystroke) = parse_global_hotkey(binding) else {
        unregister();
        if !binding.trim().is_empty() {
            log::warn!("{label}: unsupported binding {:?}, disabling", binding);
        }
        return;
    };

    let Some(key_code) = gpui_key_to_keycode(&keystroke.key) else {
        unregister();
        log::warn!(
            "{label}: unsupported key {:?} in binding {:?}, disabling",
            keystroke.key,
            binding
        );
        return;
    };

    let ok = register(
        key_code,
        keystroke.modifiers.shift,
        keystroke.modifiers.control,
        keystroke.modifiers.alt,
        keystroke.modifiers.platform,
        callback,
    );

    if !ok {
        unregister();
        log::warn!("{label}: failed to register binding {:?}", binding);
    }
}

fn parse_global_hotkey(binding: &str) -> Option<Keystroke> {
    if binding.trim().is_empty() {
        return None;
    }

    let keystroke = Keystroke::parse(binding).ok()?;
    if keystroke.key.is_empty() || !keystroke.modifiers.modified() || keystroke.modifiers.function {
        return None;
    }

    Some(keystroke)
}

extern "C" fn on_global_hotkey_pressed() {
    GLOBAL_HOTKEY_APP.with(|app| {
        let Some(app) = app.borrow().clone() else {
            return;
        };

        app.update(|cx| {
            crate::toggle_global_summon(cx);
        });
    });
}

extern "C" fn on_quick_terminal_pressed() {
    GLOBAL_HOTKEY_APP.with(|app| {
        let Some(app) = app.borrow().clone() else {
            return;
        };

        app.spawn(async move |cx| {
            cx.update(|cx| {
                crate::quick_terminal::toggle(cx);
            });
        })
        .detach();
    });
}

pub fn is_app_active() -> bool {
    unsafe { con_app_is_active() }
}

pub fn suspend_global_hotkeys(_keybindings: &KeybindingConfig) {
    unsafe {
        con_unregister_global_hotkey();
        con_unregister_quick_terminal_hotkey();
    }
    HOTKEYS_SUSPENDED.with(|s| *s.borrow_mut() = true);
}

pub fn resume_global_hotkeys(keybindings: &KeybindingConfig) {
    update_from_keybindings(keybindings);
    HOTKEYS_SUSPENDED.with(|s| *s.borrow_mut() = false);
}

#[cfg(test)]
pub fn is_suspended() -> bool {
    HOTKEYS_SUSPENDED.with(|s| *s.borrow())
}

// Keep this in sync with ghostty_view.rs.
fn gpui_key_to_keycode(key: &str) -> Option<u32> {
    Some(match key {
        "a" => 0x00,
        "s" => 0x01,
        "d" => 0x02,
        "f" => 0x03,
        "h" => 0x04,
        "g" => 0x05,
        "z" => 0x06,
        "x" => 0x07,
        "c" => 0x08,
        "v" => 0x09,
        "b" => 0x0B,
        "q" => 0x0C,
        "w" => 0x0D,
        "e" => 0x0E,
        "r" => 0x0F,
        "y" => 0x10,
        "t" => 0x11,
        "o" => 0x1F,
        "u" => 0x20,
        "i" => 0x22,
        "p" => 0x23,
        "l" => 0x25,
        "j" => 0x26,
        "k" => 0x28,
        "n" => 0x2D,
        "m" => 0x2E,
        "1" => 0x12,
        "2" => 0x13,
        "3" => 0x14,
        "4" => 0x15,
        "5" => 0x17,
        "6" => 0x16,
        "7" => 0x1A,
        "8" => 0x1C,
        "9" => 0x19,
        "0" => 0x1D,
        "-" => 0x1B,
        "=" => 0x18,
        "[" => 0x21,
        "]" => 0x1E,
        "\\" => 0x2A,
        ";" => 0x29,
        "'" => 0x27,
        "`" => 0x32,
        "," => 0x2B,
        "." => 0x2F,
        "/" => 0x2C,
        "enter" | "return" => 0x24,
        "tab" => 0x30,
        "space" => 0x31,
        "backspace" => 0x33,
        "escape" => 0x35,
        "delete" => 0x75,
        "home" => 0x73,
        "end" => 0x77,
        "pageup" => 0x74,
        "pagedown" => 0x79,
        "up" => 0x7E,
        "down" => 0x7D,
        "left" => 0x7B,
        "right" => 0x7C,
        "f1" => 0x7A,
        "f2" => 0x78,
        "f3" => 0x63,
        "f4" => 0x76,
        "f5" => 0x60,
        "f6" => 0x61,
        "f7" => 0x62,
        "f8" => 0x64,
        "f9" => 0x65,
        "f10" => 0x6D,
        "f11" => 0x67,
        "f12" => 0x6F,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use con_core::config::KeybindingConfig;

    #[test]
    fn parses_default_summon_hotkey() {
        let keystroke = parse_global_hotkey("alt-space").expect("alt-space should parse");
        assert!(keystroke.modifiers.alt);
        assert_eq!(keystroke.key, "space");
    }

    #[test]
    fn parses_default_quick_terminal_hotkey() {
        let keystroke = parse_global_hotkey("cmd-\\").expect("cmd-\\ should parse");
        assert!(keystroke.modifiers.platform);
        assert_eq!(keystroke.key, "\\");
    }

    #[test]
    fn rejects_unmodified_keys() {
        assert!(parse_global_hotkey("space").is_none());
    }

    #[test]
    fn suspend_global_hotkeys_sets_flag_and_allows_resume() {
        let kb = KeybindingConfig::default();
        assert!(!is_suspended());
        suspend_global_hotkeys(&kb);
        assert!(is_suspended());
        resume_global_hotkeys(&kb);
        assert!(!is_suspended());
    }
}
