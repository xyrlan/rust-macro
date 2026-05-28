# Mouse-Stream Playback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single-event-teleport replay of `Step::MouseMove` with a chunked stream so in-game mouse-look (Raw Input) receives continuous motion. Recorder captures motion duration; player emits one chunk per millisecond with exact delta distribution; legacy macros without the new field continue to teleport.

**Architecture:** Add `duration_ms: Option<u32>` to `Step::MouseMove` (with `#[serde(default)]` for transparent migration). Recorder populates it from the coalesced run's timespan. Player branches on `(mode, duration_ms)`: streaming only for `Relative + Some(d > 0)`. A new `stream_relative_move` helper in the player loop sleeps 1ms between chunks and distributes the delta exactly using integer-target arithmetic (`target_i = i * total / count`), so cumulative sums never drift.

**Tech Stack:** Rust stable MSVC, Tokio (`oneshot`, `tokio::time::sleep`), `serde_json` for migration tests, `MockDriver` (in-memory event sink) for deterministic player tests.

**Spec:** `docs/superpowers/specs/2026-05-28-mouse-stream-playback-design.md`

---

## File Structure

**Files to modify:**
- `crates/macro_model/src/macro_def.rs` — add `duration_ms: Option<u32>` to `Step::MouseMove`; new serde tests
- `crates/recorder/src/compile.rs` — populate `duration_ms` from coalesced run timespan; update existing test fixtures; new tests for duration calculation
- `crates/player/src/lib.rs` — add `stream_relative_move` helper; update match arm to branch on `(mode, duration_ms)`; new tests for streaming/teleport/stop-signal/zero-duration
- `crates/app/src/dto.rs` — mirror `duration_ms` field in `StepDto::MouseMove`; update both `From` impls; update existing fixture test
- `crates/app/ui/src/lib/types.ts` — add `duration_ms?: number` to the frontend `Step` discriminated union

**Files NOT changed:**
- `crates/driver/*` — `RawEvent::MouseMove` (the raw event) is unchanged; only the `Step` model is.
- `crates/recorder/src/lib.rs` — recording capture path doesn't touch `Step`; only `compile.rs` does.
- Frontend components — they construct `mouse_move` via the factory in `types.ts`; the field is optional so they don't need updating.

**Test boundaries:**
- `rm-macro-model` tests own serialization migration (legacy JSON → `None`).
- `rm-recorder` tests own duration-population logic from synthetic `Instant`-stamped events.
- `rm-player` tests own streaming behavior (chunk count, delta sum, stop signal, edge cases) using `MockDriver`.
- `rm-app::dto` tests own DTO round-trip including the new field.

---

## Task 1: Add `duration_ms` field to `Step::MouseMove` + cascade-fix all callsites

**Files:**
- Modify: `crates/macro_model/src/macro_def.rs`
- Modify: `crates/recorder/src/compile.rs` (callsite + test fixtures)
- Modify: `crates/player/src/lib.rs` (destructure pattern only)
- Modify: `crates/app/src/dto.rs` (`StepDto` + From impls + test fixture)

This task is mechanical: add the field, then make the workspace compile again by adding `duration_ms: None` to every existing callsite. No new logic. Real streaming/duration logic is Tasks 2 and 3.

- [ ] **Step 1: Add field to the enum variant**

In `crates/macro_model/src/macro_def.rs`, find the `Step::MouseMove` variant (around line 25) and add the field:

```rust
    MouseMove {
        to: Point,
        mode: MoveMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u32>,
    },
```

- [ ] **Step 2: Write the three new serde tests in `macro_def.rs`**

Append to the existing `#[cfg(test)] mod tests { ... }` block in `crates/macro_model/src/macro_def.rs`:

```rust
    #[test]
    fn mouse_move_legacy_json_deserializes_without_duration() {
        let legacy = r#"{"type":"mouse_move","to":{"x":10,"y":5},"mode":"relative"}"#;
        let s: Step = serde_json::from_str(legacy).unwrap();
        assert_eq!(
            s,
            Step::MouseMove {
                to: Point { x: 10, y: 5 },
                mode: MoveMode::Relative,
                duration_ms: None,
            }
        );
    }

    #[test]
    fn mouse_move_with_duration_roundtrips() {
        let s = Step::MouseMove {
            to: Point { x: 100, y: -20 },
            mode: MoveMode::Relative,
            duration_ms: Some(50),
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"duration_ms\":50"), "json was: {j}");
        let back: Step = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn mouse_move_none_duration_omits_field() {
        let s = Step::MouseMove {
            to: Point { x: 1, y: 2 },
            mode: MoveMode::Relative,
            duration_ms: None,
        };
        let j = serde_json::to_string(&s).unwrap();
        assert!(!j.contains("duration_ms"), "json was: {j}");
    }
```

- [ ] **Step 3: Run the macro_model tests to verify they fail because the workspace doesn't compile yet**

Run: `cargo test -p rm-macro-model 2>&1 | head -40`
Expected: build errors saying existing callsites in `recorder`/`player`/`app` are missing the `duration_ms` field. The new tests can't run because the workspace doesn't compile. That's expected — Steps 4-7 fix it.

- [ ] **Step 4: Update the player match arm**

In `crates/player/src/lib.rs` line ~136, change the destructure from `Step::MouseMove { to, mode: _ }` to include the new field (we'll wire real branching in Task 3 — for now just absorb it):

```rust
        Step::MouseMove { to, mode: _, duration_ms: _ } => {
            hub.send(RawEvent::MouseMove { dx: to.x, dy: to.y })
                .await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }
```

- [ ] **Step 5: Update the recorder's compile output**

In `crates/recorder/src/compile.rs` around line 114, add `duration_ms: None` to the constructed step (real duration calculation comes in Task 2):

```rust
                out.push(Step::MouseMove {
                    to: Point { x: total_dx, y: total_dy },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: None,
                });
```

- [ ] **Step 6: Update existing recorder test fixtures**

In the same file, find the four existing test sites that construct `Step::MouseMove`:
- ~line 258 (`mouse_move_passes_through` test)
- ~line 320 (`consecutive_mouse_moves_coalesce_summing_deltas` test)
- ~line 343 and ~line 350 (`mouse_move_runs_break_on_non_move_event` test, two MouseMove fixtures)

Add `duration_ms: None,` to each — for now we're keeping these tests passing with their current shape; Task 2 will rewrite the relevant ones to assert real durations.

Example shape:

```rust
            vec![Step::MouseMove {
                to: Point { x: 10, y: -5 },
                mode: rm_macro_model::MoveMode::Relative,
                duration_ms: None,
            },]
```

- [ ] **Step 7: Update the DTO mirror + From impls + test fixture in `crates/app/src/dto.rs`**

7a. Update the `StepDto` enum (around line 117) to mirror the field:

```rust
    MouseMove {
        to: PointDto,
        mode: MoveModeDto,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u32>,
    },
```

7b. Update `From<&Step> for StepDto` (around line 158):

```rust
            Step::MouseMove { to, mode, duration_ms } => StepDto::MouseMove {
                to: PointDto::from(to),
                mode: MoveModeDto::from(mode),
                duration_ms: *duration_ms,
            },
```

7c. Update `From<StepDto> for Step` (around line 180):

```rust
            StepDto::MouseMove { to, mode, duration_ms } => Step::MouseMove {
                to: to.into(),
                mode: mode.into(),
                duration_ms,
            },
```

7d. Update the fixture in `step_dto_mouse_move_with_point_roundtrips` (around line 333) and the conversion fixture (around line 355) to include `duration_ms: None`. Example:

```rust
        let s = StepDto::MouseMove {
            to: PointDto { x: 10, y: -5 },
            mode: MoveModeDto::Relative,
            duration_ms: None,
        };
```

and:

```rust
            Step::MouseMove {
                to: rm_macro_model::Point { x: 5, y: -3 },
                mode: rm_macro_model::MoveMode::Relative,
                duration_ms: None,
            },
```

- [ ] **Step 8: Run the full workspace test suite**

Run: `cargo test --workspace --no-fail-fast`
Expected: PASS — all existing tests plus the three new macro_model tests (119 total: 116 from before + 3 new). No new behavior yet beyond the field add.

- [ ] **Step 9: Commit**

```powershell
git add crates/macro_model/src/macro_def.rs crates/recorder/src/compile.rs crates/player/src/lib.rs crates/app/src/dto.rs
git commit -m "feat(macro-model): add optional duration_ms to Step::MouseMove

Field is Option<u32> with serde defaults so legacy JSON deserializes as
None and serializes back without the field. All existing callsites
updated to pass duration_ms: None — recorder population in next commit,
player streaming after that.
"
```

---

## Task 2: Recorder populates `duration_ms` from coalesced run timespan

**Files:**
- Modify: `crates/recorder/src/compile.rs`

The coalescing loop already tracks `j` (one past the last consumed move) and `last_at = raw[j-1].at`. Add a `first_at` snapshot and compute the duration with the existing `duration_ms_between` helper.

- [ ] **Step 1: Add the test for coalesced duration first (TDD)**

Append to `crates/recorder/src/compile.rs`'s `#[cfg(test)] mod tests { ... }` block:

```rust
    #[test]
    fn coalesced_moves_set_duration_ms() {
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0), RawEvent::MouseMove { dx: 5, dy: 0 }),
            ev(at(t0, 5), RawEvent::MouseMove { dx: 3, dy: 2 }),
            ev(at(t0, 10), RawEvent::MouseMove { dx: 2, dy: -1 }),
        ];
        let steps = compile_events(&raw);
        assert_eq!(
            steps,
            vec![Step::MouseMove {
                to: Point { x: 10, y: 1 },
                mode: rm_macro_model::MoveMode::Relative,
                duration_ms: Some(10),
            }]
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
    fn zero_duration_run_collapses_to_none() {
        // Two moves at the same Instant — duration is 0, recorder collapses to None.
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0), RawEvent::MouseMove { dx: 1, dy: 0 }),
            ev(at(t0, 0), RawEvent::MouseMove { dx: 1, dy: 0 }),
        ];
        let steps = compile_events(&raw);
        assert_eq!(
            steps,
            vec![Step::MouseMove {
                to: Point { x: 2, y: 0 },
                mode: rm_macro_model::MoveMode::Relative,
                duration_ms: None,
            }]
        );
    }
```

- [ ] **Step 2: Run the new tests to verify two of them fail**

Run: `cargo test -p rm-recorder coalesced_moves_set_duration_ms single_move_has_no_duration zero_duration_run_collapses_to_none -- --nocapture`

Expected:
- `single_move_has_no_duration` — PASS (recorder already produces `duration_ms: None` from Task 1's stub).
- `coalesced_moves_set_duration_ms` — FAIL (expected `Some(10)`, got `None`).
- `zero_duration_run_collapses_to_none` — PASS (collapse-to-None is the default since we hardcoded `None`).

The failing test drives the next step.

- [ ] **Step 3: Update the recorder's MouseMove arm to compute duration**

In `crates/recorder/src/compile.rs`, replace the `out.push(Step::MouseMove { ... })` block inside the `RawEvent::MouseMove` arm with the duration-aware version:

```rust
            RawEvent::MouseMove { dx, dy } => {
                let first_at = cur.at;
                // Coalesce this MouseMove and all immediately following
                // MouseMoves into one step with summed deltas.
                let mut total_dx: i32 = dx;
                let mut total_dy: i32 = dy;
                let mut j = i + 1;
                while let Some(next) = raw.get(j) {
                    if let RawEvent::MouseMove { dx: ndx, dy: ndy } = next.event {
                        total_dx = total_dx.saturating_add(ndx);
                        total_dy = total_dy.saturating_add(ndy);
                        j += 1;
                    } else {
                        break;
                    }
                }
                let dur = duration_ms_between(first_at, raw[j - 1].at);
                out.push(Step::MouseMove {
                    to: Point { x: total_dx, y: total_dy },
                    mode: rm_macro_model::MoveMode::Relative,
                    duration_ms: if dur > 0 { Some(dur) } else { None },
                });
                // Advance past the entire run; last_at is the last consumed
                // move so the next Wait reflects time-since-motion-ended.
                last_at = raw[j - 1].at;
                i = j;
                continue;
            }
```

The new pieces are the `let first_at = cur.at;` line at the top of the arm and the `let dur = duration_ms_between(...)` + `duration_ms: if dur > 0 { Some(dur) } else { None },` field. Everything else is the existing logic.

- [ ] **Step 4: Run the new tests to verify they pass**

Run: `cargo test -p rm-recorder coalesced_moves_set_duration_ms single_move_has_no_duration zero_duration_run_collapses_to_none -- --nocapture`
Expected: PASS for all three.

- [ ] **Step 5: Run all recorder tests to make sure existing ones still pass**

Run: `cargo test -p rm-recorder`
Expected: PASS — including the existing `consecutive_mouse_moves_coalesce_summing_deltas` and `mouse_move_runs_break_on_non_move_event` fixtures (which assert `duration_ms: None` from Task 1's update). These still pass because their fixtures use the same `Instant` for every event (`at(t0, 0)`, `at(t0, 4)`, etc. — wait, those use different offsets).

NB: the existing `consecutive_mouse_moves_coalesce_summing_deltas` fixture uses `t0+0`, `t0+4`, `t0+8`, `t0+12` — so its duration will now compute to 12ms, NOT None. The Task 1 fixture update set `duration_ms: None` there. **Update this test now:** change `duration_ms: None` to `duration_ms: Some(12)`.

Similarly, `mouse_move_runs_break_on_non_move_event` uses two runs:
- Run 1: t0+0, t0+5 → duration 5ms → `duration_ms: Some(5)`
- Run 2: t0+200, t0+205 → duration 5ms → `duration_ms: Some(5)`

Update both fixtures.

After these updates, run `cargo test -p rm-recorder` again. Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add crates/recorder/src/compile.rs
git commit -m "feat(recorder): populate duration_ms from coalesced MouseMove run timespan

The coalescing loop now captures first_at and computes the run's duration
via the existing duration_ms_between helper. Some(0) collapses to None
so single-event runs and same-Instant runs are indistinguishable from
hand-authored teleports.
"
```

---

## Task 3: Player streams `Step::MouseMove` chunks for `Relative + Some(d > 0)`

**Files:**
- Modify: `crates/player/src/lib.rs`

Add a `stream_relative_move` helper that distributes the total delta across `duration_ms` chunks at 1ms intervals, with integer-target arithmetic so cumulative sums equal the total exactly. Update the match arm to branch on `(mode, duration_ms)`.

- [ ] **Step 1: Write the five new player tests first (TDD)**

Append to `crates/player/src/lib.rs`'s `#[cfg(test)] mod tests { ... }` block:

```rust
    use rm_macro_model::{MoveMode, Point};

    fn count_mouse_moves(events: &[RawEvent]) -> usize {
        events.iter().filter(|e| matches!(e, RawEvent::MouseMove { .. })).count()
    }

    fn sum_mouse_deltas(events: &[RawEvent]) -> (i32, i32) {
        events.iter().fold((0i32, 0i32), |(ax, ay), e| match e {
            RawEvent::MouseMove { dx, dy } => (ax.saturating_add(*dx), ay.saturating_add(*dy)),
            _ => (ax, ay),
        })
    }

    #[tokio::test]
    async fn streamed_move_sends_chunks_summing_to_total() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![Step::MouseMove {
                to: Point { x: 100, y: 0 },
                mode: MoveMode::Relative,
                duration_ms: Some(50),
            }],
            PlaybackMode::Once,
        );
        play(hub, m).wait().await.unwrap();
        let sent = drv.drain_sent();
        let count = count_mouse_moves(&sent);
        assert!(count >= 40 && count <= 50, "expected ~50 chunks, got {count}");
        assert_eq!(sum_mouse_deltas(&sent), (100, 0));
    }

    #[tokio::test]
    async fn streamed_move_with_sparse_motion_skips_zero_chunks() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![Step::MouseMove {
                to: Point { x: 3, y: 0 },
                mode: MoveMode::Relative,
                duration_ms: Some(100),
            }],
            PlaybackMode::Once,
        );
        play(hub, m).wait().await.unwrap();
        let sent = drv.drain_sent();
        assert_eq!(count_mouse_moves(&sent), 3, "expected 3 non-zero sends");
        assert_eq!(sum_mouse_deltas(&sent), (3, 0));
    }

    #[tokio::test]
    async fn streamed_move_honors_stop_signal_mid_stream() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![Step::MouseMove {
                to: Point { x: 500, y: 500 },
                mode: MoveMode::Relative,
                duration_ms: Some(500),
            }],
            PlaybackMode::Once,
        );
        let mut h = play(hub, m);
        tokio::time::sleep(Duration::from_millis(50)).await;
        h.stop();
        h.wait().await.unwrap();
        let sent = drv.drain_sent();
        let count = count_mouse_moves(&sent);
        assert!(count < 200, "stop should have cut the stream short, got {count} chunks");
    }

    #[tokio::test]
    async fn absolute_move_ignores_duration_ms() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![Step::MouseMove {
                to: Point { x: 42, y: 17 },
                mode: MoveMode::Absolute,
                duration_ms: Some(100),
            }],
            PlaybackMode::Once,
        );
        play(hub, m).wait().await.unwrap();
        let sent = drv.drain_sent();
        assert_eq!(count_mouse_moves(&sent), 1);
        assert_eq!(sum_mouse_deltas(&sent), (42, 17));
    }

    #[tokio::test]
    async fn none_or_zero_duration_teleports() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![
                Step::MouseMove {
                    to: Point { x: 10, y: 5 },
                    mode: MoveMode::Relative,
                    duration_ms: None,
                },
                Step::MouseMove {
                    to: Point { x: -1, y: -1 },
                    mode: MoveMode::Relative,
                    duration_ms: Some(0),
                },
            ],
            PlaybackMode::Once,
        );
        play(hub, m).wait().await.unwrap();
        let sent = drv.drain_sent();
        assert_eq!(count_mouse_moves(&sent), 2, "both None and Some(0) should be single sends");
        assert_eq!(sum_mouse_deltas(&sent), (9, 4));
    }
```

- [ ] **Step 2: Run the new tests to verify the streaming ones fail**

Run: `cargo test -p rm-player streamed_move_ absolute_move_ignores_duration_ms none_or_zero_duration_teleports`

Expected:
- `none_or_zero_duration_teleports` — PASS (current behavior is teleport already).
- `absolute_move_ignores_duration_ms` — PASS (current behavior ignores duration entirely).
- `streamed_move_sends_chunks_summing_to_total` — FAIL (only one send, count=1).
- `streamed_move_with_sparse_motion_skips_zero_chunks` — FAIL (one send with `dx=3`, not three sends with `dx=1`).
- `streamed_move_honors_stop_signal_mid_stream` — FAIL (likely passes by accident since the single send completes instantly — but assertion `count < 200` might pass too; that's OK, the meaningful test is whether streaming kicks in).

The first two streaming-test failures drive the implementation.

- [ ] **Step 3: Add the `stream_relative_move` helper and update the match arm**

In `crates/player/src/lib.rs`, replace the existing `Step::MouseMove { ... }` arm (around line 136 — currently `Step::MouseMove { to, mode: _, duration_ms: _ }` after Task 1) with:

```rust
        Step::MouseMove { to, mode, duration_ms } => {
            match (mode, duration_ms) {
                (rm_macro_model::MoveMode::Absolute, _)
                | (_, None)
                | (_, Some(0)) => {
                    hub.send(RawEvent::MouseMove { dx: to.x, dy: to.y })
                        .await
                        .map_err(|e| AppError::DriverIo(e.to_string()))?;
                }
                (rm_macro_model::MoveMode::Relative, Some(dur)) => {
                    stream_relative_move(hub, to.x, to.y, *dur, stop_rx).await?;
                }
            }
        }
```

Note: this requires `run_step` to take `&mut stop_rx` so it can be passed to the streaming helper. Update the signature.

Change the `run_step` signature from:

```rust
async fn run_step(hub: &DriverHub, step: &Step) -> Result<()> {
```

to:

```rust
async fn run_step(
    hub: &DriverHub,
    step: &Step,
    stop_rx: &mut oneshot::Receiver<()>,
) -> Result<()> {
```

And update the call in `run` (around line 96) from:

```rust
            run_step(&hub, step).await?;
```

to:

```rust
            run_step(&hub, step, &mut stop_rx).await?;
```

Then add the helper function — place it directly after `run_step`:

```rust
/// Stream a relative mouse move as a sequence of small chunks at 1ms intervals.
/// The cumulative sum of all chunks equals (total_dx, total_dy) exactly,
/// using integer-target arithmetic to avoid drift. Chunks that round to (0, 0)
/// are not sent but still consume their 1ms sleep so the total elapsed time
/// honors `duration_ms`. Aborts early if `stop_rx` fires.
async fn stream_relative_move(
    hub: &DriverHub,
    total_dx: i32,
    total_dy: i32,
    duration_ms: u32,
    stop_rx: &mut oneshot::Receiver<()>,
) -> Result<()> {
    let chunk_count = (duration_ms as i64).max(1);
    let tdx = total_dx as i64;
    let tdy = total_dy as i64;
    let mut sent_x: i64 = 0;
    let mut sent_y: i64 = 0;

    for i in 1..=chunk_count {
        if stop_rx.try_recv().is_ok() {
            return Ok(());
        }

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

- [ ] **Step 4: Run the new player tests to verify they pass**

Run: `cargo test -p rm-player streamed_move_ absolute_move_ignores_duration_ms none_or_zero_duration_teleports`
Expected: PASS for all five.

- [ ] **Step 5: Run the full player test suite**

Run: `cargo test -p rm-player`
Expected: PASS — including pre-existing tests (`keypress_emits_down_then_up`, `loop_stops_on_signal`, `run_with_stop_signals_clean_exit`, etc.).

- [ ] **Step 6: Run the full workspace tests**

Run: `cargo test --workspace --no-fail-fast`
Expected: PASS. Total count should be ~124 (116 prior + 3 macro_model + 3 recorder + 5 player = 127, actually).

- [ ] **Step 7: Commit**

```powershell
git add crates/player/src/lib.rs
git commit -m "feat(player): stream Step::MouseMove chunks for Relative + Some(d > 0)

Replaces the single-send teleport with stream_relative_move when duration
is present. Chunks are emitted at 1ms intervals, with deltas distributed
via integer-target arithmetic so cumulative sums match the total exactly
even when chunk_count > |total| (sparse motion). Absolute mode, None, and
Some(0) keep the teleport path. Stop signal is checked between chunks.
"
```

---

## Task 4: Frontend type mirror

**Files:**
- Modify: `crates/app/ui/src/lib/types.ts`

The DTO now has `duration_ms: Option<u32>`. TypeScript should mirror it so type-check stays honest, even though components don't read or write it yet.

- [ ] **Step 1: Add the optional field to the `mouse_move` variant**

In `crates/app/ui/src/lib/types.ts` around line 90:

```ts
  | { type: "mouse_move"; to: PointDto; mode: MoveModeDto; duration_ms?: number }
```

The `?` marks it optional, matching the Rust `Option<u32>` + `#[serde(skip_serializing_if = "Option::is_none")]` pair.

- [ ] **Step 2: Verify the frontend build still passes**

Run:
```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: build succeeds — the existing factory `mouse_move: () => ({ type: "mouse_move", to: { x: 0, y: 0 }, mode: "relative" })` is still valid because `duration_ms` is optional.

- [ ] **Step 3: Commit**

```powershell
git add crates/app/ui/src/lib/types.ts
git commit -m "feat(app/ui): mirror optional duration_ms on Step.mouse_move"
```

---

## Task 5: Final verification

- [ ] **Step 1: Workspace tests**

Run: `cargo test --workspace --no-fail-fast`
Expected: PASS. 11 new tests total (3 macro_model + 3 recorder + 5 player).

- [ ] **Step 2: Both feature variants build**

Run:
```powershell
cargo check -p rm-app
cargo check -p rm-app --no-default-features
```
Both: PASS.

- [ ] **Step 3: Frontend build**

Run:
```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```
Expected: PASS.

- [ ] **Step 4: Manual smoke (deferred — user runs)**

Acceptance items the user runs themselves (not part of CI):

1. Record a mouse-move-heavy macro (e.g., a smooth horizontal sweep over ~500ms) and inspect the saved JSON in `{storage_root}/macros/`. Confirm: the resulting `mouse_move` step has a `duration_ms` field with a sane value (~500).
2. Play that macro back in Notepad (windowed) — cursor should glide smoothly, not teleport.
3. Play the same macro in Resident Evil in-game (mouse-look). Camera should rotate continuously matching the recorded motion. **This is the user-visible win.**
4. Load a legacy macro saved before this change (no `duration_ms` field) — should still load, still play (instant teleport — old behavior preserved).

If smoke item 3 still fails in RE, the spec's "Open Risks" section flags two follow-ups: per-chunk delta capping (split into smaller per-chunk magnitudes) or sleep-deadline-based timing (precompute end Instant). Defer until reproduced.

- [ ] **Step 5: No commit if Steps 1-3 pass**

Steps 1-4 in Tasks 1-4 committed all changes. Final verification is acceptance-only.

---

## Acceptance Checklist

- [ ] `cargo test --workspace` green with 127+ tests (was 116).
- [ ] `cargo check -p rm-app` and `cargo check -p rm-app --no-default-features` both pass.
- [ ] Frontend `npm run build` passes.
- [ ] Legacy macro JSON (without `duration_ms`) deserializes and plays unchanged.
- [ ] New recordings produce `mouse_move` steps with `duration_ms: Some(<recorded-span>)` when the run spans more than 0ms.
- [ ] Player streams Relative moves with `Some(d > 0)` at 1ms cadence; Absolute and `None`/`Some(0)` teleport as before.
- [ ] Mid-stream stop signal cuts the stream within a chunk-or-two.

---

## Open Implementation Notes

- **Sleep granularity on Windows.** `tokio::time::sleep(1ms)` relies on the system timer resolution. Tauri's main loop typically requests 1ms resolution via `timeBeginPeriod`, so we inherit it. If actual playback duration drifts noticeably above `duration_ms`, the fallback is to switch from "N iterations of sleep(1ms)" to "loop until `Instant::now() >= deadline`, sleeping `min(1ms, remaining)` each step". Defer until measured.

- **Skipped zero-chunks vs sent zero-chunks.** The implementation skips chunks that round to `(0, 0)` instead of sending no-op events. This is intentional: sending `(0, 0)` would still be processed by the hub and (in real Interception) by the kernel, costing latency for no game-visible effect. The sleep still happens, so timing is preserved.

- **Cast safety review.** `duration_ms: u32` → `i64` via `as` is lossless. `total_dx: i32` → `i64` is lossless. `i * tdx` where `i ≤ u32::MAX` and `tdx ≤ i32::MAX`: bounded by `2^32 * 2^31 = 2^63`, fits in i64 with one bit to spare. The final `(target_x - sent_x) as i32` reduces back to i32: safe because each chunk's delta is at most `total / count + 1`, which for reasonable inputs fits in i32 easily.

- **`Step::MouseMove` enum variant exhaustiveness.** Rust's pattern-match exhaustiveness check will catch any future callsite that destructures without the new field. The `cargo check` in Task 5 Step 2 confirms there are none left.
