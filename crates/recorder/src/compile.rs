use std::time::Instant;

use rm_driver::RawEvent;
use rm_macro_model::{Point, Step};

/// Minimum Wait duration that survives compilation. Sub-threshold gaps are
/// dropped — humans don't perceive them, and they bloat step lists during
/// dense input. If you need precise timing, edit the macro JSON directly.
pub const MIN_WAIT_MS: u32 = 20;

/// One raw event paired with its capture timestamp.
#[derive(Debug, Clone)]
pub struct TimedEvent {
    pub event: RawEvent,
    pub at: Instant,
}

/// Compile a sequence of raw timed events into a high-level `Vec<Step>`:
///   * **Adjacent** `KeyDown(k) → KeyUp(k)` (no events between) collapses into
///     `KeyPress { key: k, hold_ms: delta }`. If anything (even another key)
///     happens between, the events are kept as raw `KeyDown` / `KeyUp` — this
///     is the honest representation for overlapping inputs.
///   * Same rule for `MouseDown(b) → MouseUp(b)`.
///   * **Each `MouseMove` becomes its own `Step::MouseMove { to: dx,dy,
///     duration_ms }`**, with `duration_ms` set to the interval until the
///     next event (any kind), capped at 100ms. We DO NOT coalesce consecutive
///     moves — a coalesced sum replays as a straight line, losing the curve
///     the user actually traced. Storage grows ~N× where N is the HID poll
///     rate of the mouse (typically 1000Hz for gaming mice), but the
///     trajectory is preserved exactly.
///   * `MouseWheel` becomes `Step::MouseScroll`.
///   * Inter-event gaps become `Step::Wait { min_ms: gap, max_ms: gap }`.
///   * Lone / orphan key/mouse-up events emit a literal `Step::KeyUp` etc.
pub fn compile_events(raw: &[TimedEvent]) -> Vec<Step> {
    if raw.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut i = 0;
    let mut last_at = raw[0].at;
    while i < raw.len() {
        let cur = &raw[i];
        // Emit a Wait for the gap since the previously emitted step's end.
        let gap = cur.at.duration_since(last_at);
        let ms = gap.as_millis().min(u32::MAX as u128) as u32;
        if ms >= MIN_WAIT_MS {
            out.push(Step::Wait {
                min_ms: ms,
                max_ms: ms,
            });
        }
        match cur.event {
            RawEvent::KeyDown { key } => {
                // Collapse to KeyPress ONLY if the immediately next event is
                // the matching KeyUp. Otherwise emit a literal KeyDown — any
                // overlap or interleaving stays honest in the step list.
                if let Some(next) = raw.get(i + 1) {
                    if let RawEvent::KeyUp { key: uk } = next.event {
                        if uk == key {
                            let hold = duration_ms_between(cur.at, next.at);
                            out.push(Step::KeyPress { key, hold_ms: hold });
                            last_at = next.at;
                            i += 2;
                            continue;
                        }
                    }
                }
                out.push(Step::KeyDown { key });
            }
            RawEvent::KeyUp { key } => {
                out.push(Step::KeyUp { key });
            }
            RawEvent::MouseDown { button } => {
                if let Some(next) = raw.get(i + 1) {
                    if let RawEvent::MouseUp { button: ub } = next.event {
                        if ub == button {
                            let hold = duration_ms_between(cur.at, next.at);
                            out.push(Step::MouseClick {
                                button,
                                hold_ms: hold,
                                at: None,
                            });
                            last_at = next.at;
                            i += 2;
                            continue;
                        }
                    }
                }
                out.push(Step::MouseClick {
                    button,
                    hold_ms: 0,
                    at: None,
                });
            }
            RawEvent::MouseUp { .. } => {
                // Orphan up — drop silently. (Always paired with the preceding
                // MouseDown when adjacent; orphans imply caller noise.)
            }
            RawEvent::MouseMove { dx, dy } => {
                // No coalescing: each HID poll's delta becomes its own Step
                // so playback follows the recorded trajectory exactly. A
                // coalesced sum replays as a straight line from start to
                // end, losing the curve the user actually drew.
                //
                // duration_ms = interval to the next MouseMove only (so the
                // player paces the trajectory at original cadence). If the
                // next event is non-MouseMove, duration_ms is None — the
                // motion ends here and the gap becomes a Wait step instead
                // (the existing Wait insertion logic handles it; using
                // duration_ms here would double-count the gap).
                let dur = match raw.get(i + 1) {
                    Some(next) if matches!(next.event, RawEvent::MouseMove { .. }) => {
                        duration_ms_between(cur.at, next.at).min(100)
                    }
                    _ => 0,
                };
                out.push(Step::MouseMove {
                    to: Point { x: dx, y: dy },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: if dur > 0 { Some(dur) } else { None },
                });
            }
            RawEvent::MouseWheel { delta } => {
                out.push(Step::MouseScroll { delta });
            }
        }
        last_at = cur.at;
        i += 1;
    }
    out
}

fn duration_ms_between(a: Instant, b: Instant) -> u32 {
    b.saturating_duration_since(a)
        .as_millis()
        .min(u32::MAX as u128) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use rm_macro_model::{KeyCode, MouseButton};

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn ev(at: Instant, e: RawEvent) -> TimedEvent {
        TimedEvent { event: e, at }
    }

    #[test]
    fn empty_returns_empty() {
        assert!(compile_events(&[]).is_empty());
    }

    #[test]
    fn keydown_keyup_collapses_to_keypress() {
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0), RawEvent::KeyDown { key: KeyCode::W }),
            ev(at(t0, 250), RawEvent::KeyUp { key: KeyCode::W }),
        ];
        let steps = compile_events(&raw);
        assert_eq!(
            steps,
            vec![Step::KeyPress {
                key: KeyCode::W,
                hold_ms: 250
            }]
        );
    }

    #[test]
    fn gap_between_keys_emits_wait() {
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0), RawEvent::KeyDown { key: KeyCode::A }),
            ev(at(t0, 80), RawEvent::KeyUp { key: KeyCode::A }),
            ev(at(t0, 230), RawEvent::KeyDown { key: KeyCode::B }),
            ev(at(t0, 310), RawEvent::KeyUp { key: KeyCode::B }),
        ];
        let steps = compile_events(&raw);
        assert_eq!(
            steps,
            vec![
                Step::KeyPress {
                    key: KeyCode::A,
                    hold_ms: 80
                },
                Step::Wait {
                    min_ms: 150,
                    max_ms: 150
                },
                Step::KeyPress {
                    key: KeyCode::B,
                    hold_ms: 80
                },
            ]
        );
    }

    #[test]
    fn lone_keydown_without_keyup_emits_keydown() {
        let t0 = Instant::now();
        let raw = vec![ev(
            at(t0, 0),
            RawEvent::KeyDown {
                key: KeyCode::LShift,
            },
        )];
        let steps = compile_events(&raw);
        assert_eq!(
            steps,
            vec![Step::KeyDown {
                key: KeyCode::LShift
            }]
        );
    }

    #[test]
    fn mouse_down_up_collapses() {
        let t0 = Instant::now();
        let raw = vec![
            ev(
                at(t0, 0),
                RawEvent::MouseDown {
                    button: MouseButton::Left,
                },
            ),
            ev(
                at(t0, 60),
                RawEvent::MouseUp {
                    button: MouseButton::Left,
                },
            ),
        ];
        let steps = compile_events(&raw);
        assert_eq!(
            steps,
            vec![Step::MouseClick {
                button: MouseButton::Left,
                hold_ms: 60,
                at: None
            },]
        );
    }

    #[test]
    fn mouse_move_passes_through() {
        let t0 = Instant::now();
        let raw = vec![ev(at(t0, 0), RawEvent::MouseMove { dx: 10, dy: -5 })];
        let steps = compile_events(&raw);
        assert_eq!(
            steps,
            vec![Step::MouseMove {
                to: Point { x: 10, y: -5 },
                mode: rm_macro_model::MoveMode::Relative,
                duration_ms: None,
            },]
        );
    }

    #[test]
    fn mouse_wheel_passes_through() {
        let t0 = Instant::now();
        let raw = vec![ev(at(t0, 0), RawEvent::MouseWheel { delta: 120 })];
        assert_eq!(compile_events(&raw), vec![Step::MouseScroll { delta: 120 }]);
    }

    #[test]
    fn overlapping_keys_emit_raw_down_up() {
        // Down A, Down B, Up A, Up B → no collapse, because A's KeyUp is not
        // adjacent to A's KeyDown (B's KeyDown is between them). Honest
        // representation: keep raw Down/Up + Wait gaps.
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0), RawEvent::KeyDown { key: KeyCode::A }),
            ev(at(t0, 50), RawEvent::KeyDown { key: KeyCode::B }),
            ev(at(t0, 100), RawEvent::KeyUp { key: KeyCode::A }),
            ev(at(t0, 150), RawEvent::KeyUp { key: KeyCode::B }),
        ];
        let steps = compile_events(&raw);
        assert_eq!(
            steps,
            vec![
                Step::KeyDown { key: KeyCode::A },
                Step::Wait {
                    min_ms: 50,
                    max_ms: 50
                },
                Step::KeyDown { key: KeyCode::B },
                Step::Wait {
                    min_ms: 50,
                    max_ms: 50
                },
                Step::KeyUp { key: KeyCode::A },
                Step::Wait {
                    min_ms: 50,
                    max_ms: 50
                },
                Step::KeyUp { key: KeyCode::B },
            ]
        );
    }

    #[test]
    fn consecutive_mouse_moves_emit_one_step_per_event() {
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0), RawEvent::MouseMove { dx: 5, dy: 0 }),
            ev(at(t0, 4), RawEvent::MouseMove { dx: 3, dy: 2 }),
            ev(at(t0, 8), RawEvent::MouseMove { dx: -1, dy: 4 }),
            ev(at(t0, 12), RawEvent::MouseMove { dx: 10, dy: -3 }),
        ];
        let steps = compile_events(&raw);
        assert_eq!(
            steps,
            vec![
                Step::MouseMove {
                    to: Point { x: 5, y: 0 },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: Some(4),
                },
                Step::MouseMove {
                    to: Point { x: 3, y: 2 },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: Some(4),
                },
                Step::MouseMove {
                    to: Point { x: -1, y: 4 },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: Some(4),
                },
                Step::MouseMove {
                    to: Point { x: 10, y: -3 },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: None,
                },
            ]
        );
    }

    #[test]
    fn mouse_move_followed_by_non_move_uses_none_duration() {
        // Move, Move, KeyDown, Move → each move is its own Step; the move
        // immediately before a non-MouseMove event has duration_ms=None so
        // the inter-event gap becomes a Wait without double-counting.
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0), RawEvent::MouseMove { dx: 5, dy: 0 }),
            ev(at(t0, 5), RawEvent::MouseMove { dx: 3, dy: 0 }),
            ev(at(t0, 50), RawEvent::KeyDown { key: KeyCode::A }),
            ev(at(t0, 100), RawEvent::KeyUp { key: KeyCode::A }),
            ev(at(t0, 200), RawEvent::MouseMove { dx: -2, dy: 4 }),
            ev(at(t0, 205), RawEvent::MouseMove { dx: 1, dy: 1 }),
        ];
        let steps = compile_events(&raw);
        assert_eq!(
            steps,
            vec![
                Step::MouseMove {
                    to: Point { x: 5, y: 0 },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: Some(5),
                },
                Step::MouseMove {
                    to: Point { x: 3, y: 0 },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: None,
                },
                Step::Wait { min_ms: 45, max_ms: 45 },
                Step::KeyPress { key: KeyCode::A, hold_ms: 50 },
                Step::Wait { min_ms: 100, max_ms: 100 },
                Step::MouseMove {
                    to: Point { x: -2, y: 4 },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: Some(5),
                },
                Step::MouseMove {
                    to: Point { x: 1, y: 1 },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: None,
                },
            ]
        );
    }

    #[test]
    fn non_overlapping_keys_collapse_to_keypress() {
        // Down A, Up A, Down B, Up B → both collapse cleanly.
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0), RawEvent::KeyDown { key: KeyCode::A }),
            ev(at(t0, 100), RawEvent::KeyUp { key: KeyCode::A }),
            ev(at(t0, 200), RawEvent::KeyDown { key: KeyCode::B }),
            ev(at(t0, 300), RawEvent::KeyUp { key: KeyCode::B }),
        ];
        let steps = compile_events(&raw);
        assert_eq!(
            steps,
            vec![
                Step::KeyPress {
                    key: KeyCode::A,
                    hold_ms: 100
                },
                Step::Wait {
                    min_ms: 100,
                    max_ms: 100
                },
                Step::KeyPress {
                    key: KeyCode::B,
                    hold_ms: 100
                },
            ]
        );
    }

    #[test]
    fn waits_below_threshold_are_dropped() {
        // Gap of 15ms between two key presses should be filtered.
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0), RawEvent::KeyDown { key: KeyCode::A }),
            ev(at(t0, 50), RawEvent::KeyUp { key: KeyCode::A }),
            ev(at(t0, 65), RawEvent::KeyDown { key: KeyCode::B }),
            ev(at(t0, 115), RawEvent::KeyUp { key: KeyCode::B }),
        ];
        let steps = compile_events(&raw);
        // 15ms gap dropped — adjacent KeyPresses, no Wait between.
        assert_eq!(
            steps,
            vec![
                Step::KeyPress { key: KeyCode::A, hold_ms: 50 },
                Step::KeyPress { key: KeyCode::B, hold_ms: 50 },
            ]
        );
    }

    #[test]
    fn waits_at_or_above_threshold_are_kept() {
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0), RawEvent::KeyDown { key: KeyCode::A }),
            ev(at(t0, 50), RawEvent::KeyUp { key: KeyCode::A }),
            ev(at(t0, 80), RawEvent::KeyDown { key: KeyCode::B }),
            ev(at(t0, 130), RawEvent::KeyUp { key: KeyCode::B }),
        ];
        let steps = compile_events(&raw);
        // 30ms gap kept.
        assert_eq!(
            steps,
            vec![
                Step::KeyPress { key: KeyCode::A, hold_ms: 50 },
                Step::Wait { min_ms: 30, max_ms: 30 },
                Step::KeyPress { key: KeyCode::B, hold_ms: 50 },
            ]
        );
    }

    #[test]
    fn mouse_move_duration_is_interval_to_next_move() {
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0), RawEvent::MouseMove { dx: 5, dy: 0 }),
            ev(at(t0, 5), RawEvent::MouseMove { dx: 3, dy: 2 }),
            ev(at(t0, 10), RawEvent::MouseMove { dx: 2, dy: -1 }),
        ];
        let steps = compile_events(&raw);
        assert_eq!(
            steps,
            vec![
                Step::MouseMove {
                    to: Point { x: 5, y: 0 },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: Some(5),
                },
                Step::MouseMove {
                    to: Point { x: 3, y: 2 },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: Some(5),
                },
                Step::MouseMove {
                    to: Point { x: 2, y: -1 },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: None,
                },
            ]
        );
    }

    #[test]
    fn single_move_has_no_duration() {
        let t0 = Instant::now();
        let raw = vec![ev(at(t0, 0), RawEvent::MouseMove { dx: 10, dy: -5 })];
        let steps = compile_events(&raw);
        assert_eq!(
            steps,
            vec![Step::MouseMove {
                to: Point { x: 10, y: -5 },
                mode: rm_macro_model::MoveMode::Relative,
                duration_ms: None,
            }]
        );
    }

    #[test]
    fn same_instant_moves_both_get_none_duration() {
        // Two moves at the same Instant — duration_ms_between is 0, so both
        // collapse to duration_ms=None (the chunked stream path is skipped).
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0), RawEvent::MouseMove { dx: 1, dy: 0 }),
            ev(at(t0, 0), RawEvent::MouseMove { dx: 1, dy: 0 }),
        ];
        let steps = compile_events(&raw);
        assert_eq!(
            steps,
            vec![
                Step::MouseMove {
                    to: Point { x: 1, y: 0 },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: None,
                },
                Step::MouseMove {
                    to: Point { x: 1, y: 0 },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: None,
                },
            ]
        );
    }
}
