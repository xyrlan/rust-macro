//! Decompose Interception `MouseStroke`s into 0..N `RawEvent`s. One stroke can
//! carry multiple button bits + wheel + movement simultaneously; we emit in a
//! stable order: buttons (L, R, M, X1, X2), then wheel, then movement.

use rm_driver::RawEvent;
use rm_macro_model::MouseButton;

/// Decomposed events for a single Interception stroke. Returned by value to
/// avoid a heap allocation per event; consumers iterate `events.iter().flatten()`.
/// Sized at 6 to cover the worst case of a mouse stroke carrying every button
/// bit + wheel + move simultaneously (extremely rare but theoretically possible).
#[derive(Debug, Default, Clone, Copy)]
pub struct StrokeEvents {
    pub events: [Option<RawEvent>; 6],
}

impl StrokeEvents {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn iter(&self) -> impl Iterator<Item = RawEvent> + '_ {
        self.events.iter().filter_map(|o| *o)
    }
}

/// Inputs mirror the relevant fields from `kanata_interception::Stroke::Mouse`.
/// We accept the bit-flag types as plain integers so this module is testable
/// without depending on the Interception bitflag constants in unit tests.
///
/// Bit semantics (matches `interception.h`):
///   state bits — 0x01=L_DOWN, 0x02=L_UP, 0x04=R_DOWN, 0x08=R_UP,
///                0x10=M_DOWN, 0x20=M_UP, 0x40=B4_DOWN, 0x80=B4_UP,
///                0x100=B5_DOWN, 0x200=B5_UP, 0x400=WHEEL, 0x800=HWHEEL
///   flags bit  — 0x01=MOVE_RELATIVE (default), 0x02=MOVE_ABSOLUTE,
///                0x04=VIRTUAL_DESKTOP, 0x08=ATTRIBUTES_CHANGED,
///                0x10=MOVE_NOCOALESCE, 0x20=TERMSRV_SRC_SHADOW
pub fn convert_mouse(state: u16, flags: u16, rolling: i16, x: i32, y: i32) -> StrokeEvents {
    let mut out = StrokeEvents::empty();
    let mut n = 0usize;
    let push = |slot: &mut StrokeEvents, n: &mut usize, ev: RawEvent| {
        if *n < slot.events.len() {
            slot.events[*n] = Some(ev);
            *n += 1;
        }
    };

    // Buttons (left, right, middle, X1, X2 — down before up within each button).
    if state & 0x0001 != 0 { push(&mut out, &mut n, RawEvent::MouseDown { button: MouseButton::Left }); }
    if state & 0x0002 != 0 { push(&mut out, &mut n, RawEvent::MouseUp   { button: MouseButton::Left }); }
    if state & 0x0004 != 0 { push(&mut out, &mut n, RawEvent::MouseDown { button: MouseButton::Right }); }
    if state & 0x0008 != 0 { push(&mut out, &mut n, RawEvent::MouseUp   { button: MouseButton::Right }); }
    if state & 0x0010 != 0 { push(&mut out, &mut n, RawEvent::MouseDown { button: MouseButton::Middle }); }
    if state & 0x0020 != 0 { push(&mut out, &mut n, RawEvent::MouseUp   { button: MouseButton::Middle }); }
    if state & 0x0040 != 0 { push(&mut out, &mut n, RawEvent::MouseDown { button: MouseButton::X1 }); }
    if state & 0x0080 != 0 { push(&mut out, &mut n, RawEvent::MouseUp   { button: MouseButton::X1 }); }
    if state & 0x0100 != 0 { push(&mut out, &mut n, RawEvent::MouseDown { button: MouseButton::X2 }); }
    if state & 0x0200 != 0 { push(&mut out, &mut n, RawEvent::MouseUp   { button: MouseButton::X2 }); }

    // Vertical wheel (v1 — horizontal wheel deferred).
    if state & 0x0400 != 0 && rolling != 0 {
        push(&mut out, &mut n, RawEvent::MouseWheel { delta: rolling as i32 });
    }

    // Movement. `MOVE_ABSOLUTE` (flags & 0x02) is rare on raw hardware; if seen,
    // we log and pass through unchanged. RawEvent::MouseMove is relative by
    // definition.
    if x != 0 || y != 0 {
        if flags & 0x0002 != 0 {
            tracing::debug!(x, y, "interception: absolute mouse movement converted as relative");
        }
        push(&mut out, &mut n, RawEvent::MouseMove { dx: x, dy: y });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(s: &StrokeEvents) -> Vec<RawEvent> {
        s.iter().collect()
    }

    #[test]
    fn left_button_down_then_up() {
        let s = convert_mouse(0x0001 | 0x0002, 0, 0, 0, 0);
        assert_eq!(events(&s), vec![
            RawEvent::MouseDown { button: MouseButton::Left },
            RawEvent::MouseUp   { button: MouseButton::Left },
        ]);
    }

    #[test]
    fn each_button_bit_maps_correctly() {
        use MouseButton::*;
        let cases = [
            (0x0001, RawEvent::MouseDown { button: Left }),
            (0x0002, RawEvent::MouseUp   { button: Left }),
            (0x0004, RawEvent::MouseDown { button: Right }),
            (0x0008, RawEvent::MouseUp   { button: Right }),
            (0x0010, RawEvent::MouseDown { button: Middle }),
            (0x0020, RawEvent::MouseUp   { button: Middle }),
            (0x0040, RawEvent::MouseDown { button: X1 }),
            (0x0080, RawEvent::MouseUp   { button: X1 }),
            (0x0100, RawEvent::MouseDown { button: X2 }),
            (0x0200, RawEvent::MouseUp   { button: X2 }),
        ];
        for (state, expected) in cases {
            let s = convert_mouse(state, 0, 0, 0, 0);
            assert_eq!(events(&s), vec![expected], "state={:#06x}", state);
        }
    }

    #[test]
    fn wheel_emits_event_with_rolling_value() {
        let s = convert_mouse(0x0400, 0, 120, 0, 0);
        assert_eq!(events(&s), vec![RawEvent::MouseWheel { delta: 120 }]);
    }

    #[test]
    fn wheel_bit_without_rolling_emits_nothing() {
        let s = convert_mouse(0x0400, 0, 0, 0, 0);
        assert!(events(&s).is_empty());
    }

    #[test]
    fn zero_movement_emits_no_move_event() {
        let s = convert_mouse(0, 0, 0, 0, 0);
        assert!(events(&s).is_empty());
    }

    #[test]
    fn nonzero_movement_emits_relative_move() {
        let s = convert_mouse(0, 0x01, 0, 5, -3);
        assert_eq!(events(&s), vec![RawEvent::MouseMove { dx: 5, dy: -3 }]);
    }

    #[test]
    fn combined_button_wheel_and_move_emit_in_order() {
        // Left-down + wheel down + movement, all in one stroke.
        let s = convert_mouse(0x0001 | 0x0400, 0x01, -120, 10, 20);
        assert_eq!(events(&s), vec![
            RawEvent::MouseDown { button: MouseButton::Left },
            RawEvent::MouseWheel { delta: -120 },
            RawEvent::MouseMove { dx: 10, dy: 20 },
        ]);
    }
}
