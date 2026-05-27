//! Bidirectional mapping between Windows Set 1 ("XT") scancodes and the
//! `rm_macro_model::KeyCode` enum. Reference: https://wiki.osdev.org/PS/2_Keyboard
//! and the Interception SDK header. The `e0` bool corresponds to Interception's
//! `KeyState::E0` flag — needed to distinguish e.g. LCtrl (0x1D, e0=false) from
//! RCtrl (0x1D, e0=true), and to identify extended cluster keys (arrows, Insert,
//! Delete, Home/End, PageUp/PageDown — all e0=true).

use rm_macro_model::KeyCode;

pub fn scancode_to_keycode(code: u16, e0: bool) -> Option<KeyCode> {
    match (code, e0) {
        // Letter row (top, middle, bottom)
        (0x10, false) => Some(KeyCode::Q),
        (0x11, false) => Some(KeyCode::W),
        (0x12, false) => Some(KeyCode::E),
        (0x13, false) => Some(KeyCode::R),
        (0x14, false) => Some(KeyCode::T),
        (0x15, false) => Some(KeyCode::Y),
        (0x16, false) => Some(KeyCode::U),
        (0x17, false) => Some(KeyCode::I),
        (0x18, false) => Some(KeyCode::O),
        (0x19, false) => Some(KeyCode::P),
        (0x1E, false) => Some(KeyCode::A),
        (0x1F, false) => Some(KeyCode::S),
        (0x20, false) => Some(KeyCode::D),
        (0x21, false) => Some(KeyCode::F),
        (0x22, false) => Some(KeyCode::G),
        (0x23, false) => Some(KeyCode::H),
        (0x24, false) => Some(KeyCode::J),
        (0x25, false) => Some(KeyCode::K),
        (0x26, false) => Some(KeyCode::L),
        (0x2C, false) => Some(KeyCode::Z),
        (0x2D, false) => Some(KeyCode::X),
        (0x2E, false) => Some(KeyCode::C),
        (0x2F, false) => Some(KeyCode::V),
        (0x30, false) => Some(KeyCode::B),
        (0x31, false) => Some(KeyCode::N),
        (0x32, false) => Some(KeyCode::M),
        // Digit row (top of letters)
        (0x02, false) => Some(KeyCode::Num1),
        (0x03, false) => Some(KeyCode::Num2),
        (0x04, false) => Some(KeyCode::Num3),
        (0x05, false) => Some(KeyCode::Num4),
        (0x06, false) => Some(KeyCode::Num5),
        (0x07, false) => Some(KeyCode::Num6),
        (0x08, false) => Some(KeyCode::Num7),
        (0x09, false) => Some(KeyCode::Num8),
        (0x0A, false) => Some(KeyCode::Num9),
        (0x0B, false) => Some(KeyCode::Num0),
        // Function row
        (0x3B, false) => Some(KeyCode::F1),
        (0x3C, false) => Some(KeyCode::F2),
        (0x3D, false) => Some(KeyCode::F3),
        (0x3E, false) => Some(KeyCode::F4),
        (0x3F, false) => Some(KeyCode::F5),
        (0x40, false) => Some(KeyCode::F6),
        (0x41, false) => Some(KeyCode::F7),
        (0x42, false) => Some(KeyCode::F8),
        (0x43, false) => Some(KeyCode::F9),
        (0x44, false) => Some(KeyCode::F10),
        (0x57, false) => Some(KeyCode::F11),
        (0x58, false) => Some(KeyCode::F12),
        // Modifiers (Ctrl/Alt are E0-discriminated for L/R; Shift uses different codes)
        (0x2A, false) => Some(KeyCode::LShift),
        (0x36, false) => Some(KeyCode::RShift),
        (0x1D, false) => Some(KeyCode::LCtrl),
        (0x1D, true)  => Some(KeyCode::RCtrl),
        (0x38, false) => Some(KeyCode::LAlt),
        (0x38, true)  => Some(KeyCode::RAlt),
        (0x5B, true)  => Some(KeyCode::LWin),
        (0x5C, true)  => Some(KeyCode::RWin),
        // Whitespace + control
        (0x39, false) => Some(KeyCode::Space),
        (0x1C, false) => Some(KeyCode::Enter),
        (0x0F, false) => Some(KeyCode::Tab),
        (0x0E, false) => Some(KeyCode::Backspace),
        (0x01, false) => Some(KeyCode::Escape),
        (0x3A, false) => Some(KeyCode::CapsLock),
        // Arrows (all E0-extended)
        (0x48, true) => Some(KeyCode::Up),
        (0x50, true) => Some(KeyCode::Down),
        (0x4B, true) => Some(KeyCode::Left),
        (0x4D, true) => Some(KeyCode::Right),
        // Edit cluster (all E0-extended)
        (0x52, true) => Some(KeyCode::Insert),
        (0x53, true) => Some(KeyCode::Delete),
        (0x47, true) => Some(KeyCode::Home),
        (0x4F, true) => Some(KeyCode::End),
        (0x49, true) => Some(KeyCode::PageUp),
        (0x51, true) => Some(KeyCode::PageDown),
        // Punctuation (US layout)
        (0x0C, false) => Some(KeyCode::Minus),
        (0x0D, false) => Some(KeyCode::Equals),
        (0x1A, false) => Some(KeyCode::LBracket),
        (0x1B, false) => Some(KeyCode::RBracket),
        (0x2B, false) => Some(KeyCode::Backslash),
        (0x27, false) => Some(KeyCode::Semicolon),
        (0x28, false) => Some(KeyCode::Apostrophe),
        (0x29, false) => Some(KeyCode::Backtick),
        (0x33, false) => Some(KeyCode::Comma),
        (0x34, false) => Some(KeyCode::Period),
        (0x35, false) => Some(KeyCode::Slash),
        _ => None,
    }
}

pub fn keycode_to_scancode(k: KeyCode) -> (u16, bool) {
    match k {
        // Letters
        KeyCode::A => (0x1E, false), KeyCode::B => (0x30, false),
        KeyCode::C => (0x2E, false), KeyCode::D => (0x20, false),
        KeyCode::E => (0x12, false), KeyCode::F => (0x21, false),
        KeyCode::G => (0x22, false), KeyCode::H => (0x23, false),
        KeyCode::I => (0x17, false), KeyCode::J => (0x24, false),
        KeyCode::K => (0x25, false), KeyCode::L => (0x26, false),
        KeyCode::M => (0x32, false), KeyCode::N => (0x31, false),
        KeyCode::O => (0x18, false), KeyCode::P => (0x19, false),
        KeyCode::Q => (0x10, false), KeyCode::R => (0x13, false),
        KeyCode::S => (0x1F, false), KeyCode::T => (0x14, false),
        KeyCode::U => (0x16, false), KeyCode::V => (0x2F, false),
        KeyCode::W => (0x11, false), KeyCode::X => (0x2D, false),
        KeyCode::Y => (0x15, false), KeyCode::Z => (0x2C, false),
        // Digits
        KeyCode::Num0 => (0x0B, false), KeyCode::Num1 => (0x02, false),
        KeyCode::Num2 => (0x03, false), KeyCode::Num3 => (0x04, false),
        KeyCode::Num4 => (0x05, false), KeyCode::Num5 => (0x06, false),
        KeyCode::Num6 => (0x07, false), KeyCode::Num7 => (0x08, false),
        KeyCode::Num8 => (0x09, false), KeyCode::Num9 => (0x0A, false),
        // Function row
        KeyCode::F1 => (0x3B, false), KeyCode::F2 => (0x3C, false),
        KeyCode::F3 => (0x3D, false), KeyCode::F4 => (0x3E, false),
        KeyCode::F5 => (0x3F, false), KeyCode::F6 => (0x40, false),
        KeyCode::F7 => (0x41, false), KeyCode::F8 => (0x42, false),
        KeyCode::F9 => (0x43, false), KeyCode::F10 => (0x44, false),
        KeyCode::F11 => (0x57, false), KeyCode::F12 => (0x58, false),
        // Modifiers
        KeyCode::LShift => (0x2A, false), KeyCode::RShift => (0x36, false),
        KeyCode::LCtrl  => (0x1D, false), KeyCode::RCtrl  => (0x1D, true),
        KeyCode::LAlt   => (0x38, false), KeyCode::RAlt   => (0x38, true),
        KeyCode::LWin   => (0x5B, true),  KeyCode::RWin   => (0x5C, true),
        // Whitespace + control
        KeyCode::Space     => (0x39, false), KeyCode::Enter   => (0x1C, false),
        KeyCode::Tab       => (0x0F, false), KeyCode::Backspace => (0x0E, false),
        KeyCode::Escape    => (0x01, false), KeyCode::CapsLock => (0x3A, false),
        // Arrows
        KeyCode::Up    => (0x48, true), KeyCode::Down  => (0x50, true),
        KeyCode::Left  => (0x4B, true), KeyCode::Right => (0x4D, true),
        // Edit cluster
        KeyCode::Insert   => (0x52, true), KeyCode::Delete  => (0x53, true),
        KeyCode::Home     => (0x47, true), KeyCode::End     => (0x4F, true),
        KeyCode::PageUp   => (0x49, true), KeyCode::PageDown => (0x51, true),
        // Punctuation
        KeyCode::Minus      => (0x0C, false), KeyCode::Equals    => (0x0D, false),
        KeyCode::LBracket   => (0x1A, false), KeyCode::RBracket  => (0x1B, false),
        KeyCode::Backslash  => (0x2B, false), KeyCode::Semicolon => (0x27, false),
        KeyCode::Apostrophe => (0x28, false), KeyCode::Backtick  => (0x29, false),
        KeyCode::Comma      => (0x33, false), KeyCode::Period    => (0x34, false),
        KeyCode::Slash      => (0x35, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant of `KeyCode`. Update if the enum gains new variants.
    fn all_keycodes() -> Vec<KeyCode> {
        use KeyCode::*;
        vec![
            A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
            Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
            F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
            LShift, RShift, LCtrl, RCtrl, LAlt, RAlt, LWin, RWin,
            Space, Enter, Tab, Backspace, Escape, CapsLock,
            Up, Down, Left, Right,
            Insert, Delete, Home, End, PageUp, PageDown,
            Minus, Equals, LBracket, RBracket, Backslash, Semicolon,
            Apostrophe, Backtick, Comma, Period, Slash,
        ]
    }

    #[test]
    fn roundtrip_every_keycode() {
        for k in all_keycodes() {
            let (code, e0) = keycode_to_scancode(k);
            let back = scancode_to_keycode(code, e0);
            assert_eq!(back, Some(k), "roundtrip failed for {:?}", k);
        }
    }

    #[test]
    fn lctrl_and_rctrl_disambiguate_by_e0() {
        assert_eq!(scancode_to_keycode(0x1D, false), Some(KeyCode::LCtrl));
        assert_eq!(scancode_to_keycode(0x1D, true), Some(KeyCode::RCtrl));
        assert_eq!(keycode_to_scancode(KeyCode::LCtrl), (0x1D, false));
        assert_eq!(keycode_to_scancode(KeyCode::RCtrl), (0x1D, true));
    }

    #[test]
    fn arrows_are_e0_extended() {
        for k in [KeyCode::Up, KeyCode::Down, KeyCode::Left, KeyCode::Right] {
            let (_, e0) = keycode_to_scancode(k);
            assert!(e0, "{:?} must be e0-prefixed", k);
        }
    }

    #[test]
    fn unknown_scancode_returns_none() {
        assert_eq!(scancode_to_keycode(0x00, false), None);
        assert_eq!(scancode_to_keycode(0xFFFF, false), None);
    }
}
