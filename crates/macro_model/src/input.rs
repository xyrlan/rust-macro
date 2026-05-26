use serde::{Deserialize, Serialize};

/// Physical key, identified by USB HID scancode where possible.
/// Plan 2 will add `From<interception::ScanCode>` impls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyCode {
    // Letters
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    // Digits (top row)
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    // Function row
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    // Modifiers
    LShift,
    RShift,
    LCtrl,
    RCtrl,
    LAlt,
    RAlt,
    LWin,
    RWin,
    // Whitespace & control
    Space,
    Enter,
    Tab,
    Backspace,
    Escape,
    CapsLock,
    // Arrows
    Up,
    Down,
    Left,
    Right,
    // Edit cluster
    Insert,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    // Punctuation (US layout)
    Minus,
    Equals,
    LBracket,
    RBracket,
    Backslash,
    Semicolon,
    Apostrophe,
    Backtick,
    Comma,
    Period,
    Slash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
    Win,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveMode {
    Absolute,
    Relative,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycode_serializes_snake_case() {
        let json = serde_json::to_string(&KeyCode::LShift).unwrap();
        assert_eq!(json, "\"l_shift\"");
    }

    #[test]
    fn keycode_roundtrip_every_variant_via_letters_sample() {
        for k in [
            KeyCode::A,
            KeyCode::Z,
            KeyCode::Num0,
            KeyCode::F12,
            KeyCode::Backslash,
            KeyCode::LWin,
            KeyCode::PageDown,
        ] {
            let s = serde_json::to_string(&k).unwrap();
            let back: KeyCode = serde_json::from_str(&s).unwrap();
            assert_eq!(k, back, "roundtrip failed for {:?}", k);
        }
    }

    #[test]
    fn point_roundtrip() {
        let p = Point { x: 100, y: -50 };
        let s = serde_json::to_string(&p).unwrap();
        let back: Point = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
        assert_eq!(s, r#"{"x":100,"y":-50}"#);
    }

    #[test]
    fn modifier_and_move_mode_roundtrip() {
        let m = Modifier::Ctrl;
        let mm = MoveMode::Relative;
        assert_eq!(serde_json::from_str::<Modifier>("\"ctrl\"").unwrap(), m);
        assert_eq!(
            serde_json::from_str::<MoveMode>("\"relative\"").unwrap(),
            mm
        );
    }

    #[test]
    fn mouse_button_x_buttons_serialize() {
        assert_eq!(serde_json::to_string(&MouseButton::X1).unwrap(), "\"x1\"");
        assert_eq!(serde_json::to_string(&MouseButton::X2).unwrap(), "\"x2\"");
    }
}
