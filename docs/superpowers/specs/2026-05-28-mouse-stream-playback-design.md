# Mouse-Stream Playback — Design Spec

**Date:** 2026-05-28
**Status:** Approved, ready for implementation plan
**Scope:** `rm-macro-model`, `rm-recorder`, `rm-player`

## Problem

`Step::MouseMove` currently replays as a single `RawEvent::MouseMove { dx, dy }` with the coalesced summed deltas of all consecutive recorded MouseMoves. The motion's internal duration is dropped on the floor — replay teleports to the destination instantly.

This works in any context that reads cursor *state* (Windows menus, browser UIs, dialog boxes — anywhere `WM_MOUSEMOVE` / `GetCursorPos` is sufficient). It fails silently in any context that reads raw mouse *events* per frame:

- 3D game mouse-look (`RegisterRawInputDevices` / `WM_INPUT` / `RAWMOUSE.lLastX,lLastY`)
- DirectInput / DInput8 polling
- Any custom HID consumer

Two compounding mechanisms inside those consumers cause the teleport to be dropped or clamped:

1. **Per-frame delta clamping.** Engines limit max delta per frame (typical: ±50 to ±200 units) to filter input anomalies. A coalesced flick of ±2000 either clips or is rejected.
2. **Single-event-per-frame sampling.** A game running at 60 FPS samples input once per frame; a single huge event arrives in one frame, while a real mouse at 1000Hz delivers ~16 deltas per frame in a continuous stream.

User-reported repro: Resident Evil (RE Engine). Mouse macros work in the pause menu (cursor-driven UI) but do nothing in-game (Raw Input camera).

## Solution

Replace the single-event teleport with a chunked stream during playback. The stream emits one `RawEvent::MouseMove` per millisecond for the duration the original motion took to record, with deltas distributed across chunks so the cumulative sum equals the recorded total.

The data model gains an optional `duration_ms` on `Step::MouseMove`. The recorder fills it from the timespan of the coalesced run. The player chooses streaming vs teleport based on this field. Old saved macros without the field deserialize as `None` and continue to teleport (current behavior, no surprise regression).

## Data Model — `crates/macro_model/src/macro_def.rs`

```rust
MouseMove {
    to: Point,
    mode: MoveMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u32>,
},
```

- `#[serde(default)]` — JSONs from before this change deserialize cleanly with `duration_ms: None`.
- `skip_serializing_if = "Option::is_none"` — when the player or step editor produces a step with no duration (e.g., a hand-authored teleport, or a single-event recording where coalescing didn't apply), the field is absent from saved JSON. Keeps storage stable.
- `MoveMode::Absolute` ignores `duration_ms` at playback time. Absolute moves are intentional teleports to a screen coordinate; streaming them makes no sense and would re-introduce relative-to-absolute conversion bugs.

No new validation. Any `u32` (or `None`) is acceptable.

## Recorder — `crates/recorder/src/compile.rs`

Inside the existing `RawEvent::MouseMove` arm of `compile_events`, after the inner coalescing loop computes `total_dx`, `total_dy`, and `j` (one past the last consumed move):

```rust
let first_at = cur.at;
let last_at = raw[j - 1].at;
let dur = duration_ms_between(first_at, last_at);  // helper already exists

out.push(Step::MouseMove {
    to: Point { x: total_dx, y: total_dy },
    mode: MoveMode::Relative,
    duration_ms: if dur > 0 { Some(dur) } else { None },
});
```

- `duration_ms_between` is already defined in `compile.rs` and computes `(b - a).as_millis()` saturating to `u32`.
- `Some(0)` collapses to `None` — no ambiguity between "intentional instant" and "single-event coalescing run with no internal time". The player treats both identically (teleport).
- `last_at` continues to drive the next-Wait calculation (unchanged from current code), so the motion's internal time is consumed once — never replayed as both a stream AND a following Wait.

## Player — `crates/player/src/lib.rs`

Replace the existing `Step::MouseMove { to, mode: _ }` arm with:

```rust
Step::MouseMove { to, mode, duration_ms } => {
    match (mode, duration_ms) {
        (MoveMode::Absolute, _) | (_, None) | (_, Some(0)) => {
            hub.send(RawEvent::MouseMove { dx: to.x, dy: to.y })
                .await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }
        (MoveMode::Relative, Some(dur)) => {
            stream_relative_move(&hub, to.x, to.y, *dur, &mut stop_rx).await?;
        }
    }
}
```

`stream_relative_move(hub, total_dx, total_dy, duration_ms, stop_rx)`:

```rust
async fn stream_relative_move(
    hub: &Arc<DriverHub>,
    total_dx: i32,
    total_dy: i32,
    duration_ms: u32,
    stop_rx: &mut oneshot::Receiver<()>,
) -> Result<()> {
    let chunk_count = duration_ms.max(1) as i64;  // at least 1 chunk
    let tdx = total_dx as i64;
    let tdy = total_dy as i64;
    let mut sent_x: i64 = 0;
    let mut sent_y: i64 = 0;

    for i in 1..=chunk_count {
        if stop_rx.try_recv().is_ok() {
            return Ok(());  // graceful early exit
        }

        // Distribute remainder exactly: target_at_i = i * total / count, rounded.
        let target_x = i * tdx / chunk_count;
        let target_y = i * tdy / chunk_count;
        let chunk_dx = (target_x - sent_x) as i32;
        let chunk_dy = (target_y - sent_y) as i32;
        sent_x = target_x;
        sent_y = target_y;

        if chunk_dx != 0 || chunk_dy != 0 {
            hub.send(RawEvent::MouseMove { dx: chunk_dx, dy: chunk_dy })
                .await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    Ok(())
}
```

Notes:
- **Exact distribution.** After the loop, `sent_x == total_dx` and `sent_y == total_dy` by integer arithmetic — no lost units even when `chunk_count > |total|`. E.g., `dx=3, duration_ms=100` produces 100 iterations where only 3 of them emit a non-zero send (`dx=1`).
- **Zero-delta chunks skipped.** Don't spam the hub when the chunk math rounds to (0, 0). Sleep still happens — the duration must be honored even when motion is sparse.
- **Stop signal between chunks.** Same `try_recv()` pattern the outer loop uses between steps. 1ms granularity is more than enough for a user-initiated stop.
- **Cast safety.** `duration_ms: u32` → `i64`: lossless. `total_dx: i32` → `i64`: lossless. The intermediate `i * tdx` could theoretically overflow if `duration_ms * total_dx > i64::MAX` — physically impossible (would require `total_dx > 2^31` AND `duration_ms > 2^31`, both bounded by u32/i32).
- **Hub throughput.** 1000 sends/sec through `DriverHub` is well within capacity. Interception's `send` is ~tens of microseconds; the 1ms sleep dominates.

## Tests

**`rm-macro-model`** — append to existing `tests` module in `macro_def.rs`:
- `mouse_move_legacy_json_deserializes_without_duration` — feed `{"type":"mouse_move","to":{"x":10,"y":5},"mode":"relative"}` to `serde_json::from_str`, assert `duration_ms == None`.
- `mouse_move_with_duration_roundtrips` — serialize `Step::MouseMove { ..., duration_ms: Some(50) }`, parse back, assert equal.
- `mouse_move_none_duration_omits_field` — serialize `duration_ms: None`, assert string does not contain `"duration_ms"`.

**`rm-recorder`** — append to existing `compile::tests`:
- `coalesced_moves_set_duration_ms` — sequence of 3 MouseMoves at t=0, t=5ms, t=10ms produces one Step with `duration_ms == Some(10)`.
- `single_move_has_no_duration` — one MouseMove produces a Step with `duration_ms == None`.
- `zero_duration_run_collapses_to_none` — two MouseMoves at the same `Instant` produce a Step with `duration_ms == None`.

**`rm-player`** — append to existing `tests` (uses `MockDriver` + `DriverHub`):
- `streamed_move_sends_chunks_summing_to_total` — `dx=100, dy=0, duration_ms=50` produces ~50 send events with cumulative dx = 100, dy = 0.
- `streamed_move_with_sparse_motion_skips_zero_chunks` — `dx=3, duration_ms=100` produces exactly 3 non-zero sends (one per millisecond at indices ~33, 66, 99).
- `streamed_move_honors_stop_signal_mid_stream` — start a long stream (`duration_ms=500`), fire stop after ~50ms, assert send count is bounded (e.g., ≤ ~60) and the player resolves without error.
- `absolute_move_ignores_duration_ms` — `MoveMode::Absolute` with `duration_ms=Some(100)` produces a single send (no streaming).
- `none_or_zero_duration_teleports` — `duration_ms: None` and `Some(0)` both produce exactly one send.

Existing tests: the current `Step::MouseMove` callsites in tests, recorder fixtures, and player fixtures need updating to include the new field. Most can pass `duration_ms: None` explicitly.

## Out of Scope

Deferred to follow-up work:
- Step editor UI for editing `duration_ms` by hand.
- Per-macro "Game compatibility mode" toggle that forces streaming on all moves.
- User-configurable chunk interval (currently hardcoded to 1ms).
- Adaptive duration estimation for legacy macros lacking the field.

## Open Risks

- **Chunk sleep granularity.** `tokio::time::sleep(1ms)` is not guaranteed to be exactly 1ms — the tokio timer wheel has its own resolution, plus OS scheduler jitter. On Windows, default timer resolution is ~15ms unless something has called `timeBeginPeriod(1)`. **Tauri's main loop calls `timeBeginPeriod(1)`** so we inherit 1ms resolution — but this is worth verifying at smoke-test time. If sleep drifts, the actual playback duration could exceed `duration_ms` significantly. Mitigation if observed: precompute a deadline (`Instant::now() + duration_ms`) and break the loop early once reached, accepting fewer chunks in exchange for honoring duration.

- **Game-specific clamping behavior.** Some engines clamp per-event delta independently of polling rate. If a single 1ms chunk still produces e.g. `dx=50` (a fast flick of 5000 units in 100ms = 50 per chunk), some engines might still clamp. Mitigation if observed in testing: cap individual chunk magnitude to e.g. ±20 and extend duration. **Defer until reproduced** — premature optimization.

- **Empty motions.** `Step::MouseMove { to: (0,0), duration_ms: Some(N) }` would sleep N ms emitting nothing. Acceptable — that's accurate replay of "user held cursor still for N ms". The follow-up Wait shouldn't double-count because the recorder consumes time once.
