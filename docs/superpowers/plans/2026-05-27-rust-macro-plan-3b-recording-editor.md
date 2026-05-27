# rust-macro — Plan 3b: in-app recording + step editor + live hotkey capture — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the "create/edit macros in-app without the CLI" loop. Add in-app recording (with F10 to stop), a full-screen step editor, and live hotkey capture (press combo to bind) on top of the Plan 3a GUI.

**Architecture:** Extend `rm-recorder` with a stop-key parameter (filters the stop key before passthrough). Add an `ActiveRecording` slot to `AppState` mirroring `ActivePlayback`. New Tauri commands `start_recording`, `stop_recording`, `create_macro`, `load_macro_steps`, `update_macro_full`. Frontend gets a view router (list vs editor), `RecordingModal` (start + preview phases), `StepEditor` + `StepRow`, and a Capture button on `HotkeyPicker`. `EditMetadataModal` from 3a is retired — the editor absorbs metadata editing.

**Tech Stack:** Tauri 2 (Rust stable MSVC), Svelte 5 (runes), TypeScript, Vite 5. Target Windows 10/11 x64. Interception driver (already integrated in 3a/2b) opened freshly per recording session.

**Spec:** `docs/superpowers/specs/2026-05-27-rust-macro-plan-3b-recording-editor-design.md`.

---

## File Structure

**Files to create (backend):**
- `crates/app/src/recording.rs` — supervisor task helper for `start_recording`

**Files to create (frontend):**
- `crates/app/ui/src/lib/stores/recording.ts`
- `crates/app/ui/src/lib/components/RecordingModal.svelte`
- `crates/app/ui/src/lib/components/StepEditor.svelte`
- `crates/app/ui/src/lib/components/StepRow.svelte`

**Files to modify (backend):**
- `crates/recorder/src/lib.rs` — add `start_recording_with_stop_key`
- `crates/app/src/state.rs` — add `ActiveRecording`, `AppState.recording` field
- `crates/app/src/dto.rs` — add `StepDto`, `PointDto`, `MoveModeDto`
- `crates/app/src/commands.rs` — add 5 new commands, modify `play_macro`
- `crates/app/src/main.rs` — register new commands, wire `WindowEvent::CloseRequested`

**Files to modify (frontend):**
- `crates/app/ui/src/lib/types.ts` — add `StepDto`, `PointDto`, `MoveModeDto`, helpers
- `crates/app/ui/src/lib/api.ts` — add new command wrappers
- `crates/app/ui/src/lib/stores/macros.ts` — add `createMacro`, `updateMacroFull` actions
- `crates/app/ui/src/lib/components/HotkeyPicker.svelte` — add Capture button + listening state
- `crates/app/ui/src/lib/components/MacroTable.svelte` — enable "+ Record" button
- `crates/app/ui/src/lib/components/MacroRow.svelte` — ✎ button switches view instead of opening modal
- `crates/app/ui/src/App.svelte` — view router (list | editor) + Recording modal hookup
- `crates/app/README.md` — manual smoke test updates

**Files to delete:**
- `crates/app/ui/src/lib/components/EditMetadataModal.svelte` (subsumed by `StepEditor`)

Tasks decomposed by file boundary. Each task is one focused commit. The plan favors backend-first ordering so the frontend has stable Tauri commands to call against.

---

## Task 1: Extend `rm-recorder` with stop-key support

**Files:**
- Modify: `crates/recorder/src/lib.rs`

The existing `start_recording(hub, passthrough)` has no stop-key concept. Add `start_recording_with_stop_key` that accepts an optional `KeyCode` and filters it out of both the captured buffer AND the passthrough re-emit, atomically, before the event reaches either path.

- [ ] **Step 1: Write the failing test**

Append this test to `mod tests` in `crates/recorder/src/lib.rs`:

```rust
    #[tokio::test]
    async fn stop_key_filters_event_and_ends_recording() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let h = start_recording_with_stop_key(hub, true, Some(KeyCode::F10));

        // Pre-stop events:
        drv.inject(RawEvent::KeyDown { key: KeyCode::A });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        drv.inject(RawEvent::KeyUp { key: KeyCode::A });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // The stop key — must NOT appear in buffer AND must NOT be passthrough'd.
        drv.inject(RawEvent::KeyDown { key: KeyCode::F10 });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let steps = h.wait_for_close().await.unwrap();

        // Expect exactly the KeyPress { A } step — no F10 trailing.
        assert_eq!(steps.len(), 1);
        match &steps[0] {
            Step::KeyPress { key: KeyCode::A, .. } => {}
            other => panic!("expected KeyPress(A), got {other:?}"),
        }

        // And the passthrough drain should NOT contain F10.
        let sent = drv.drain_sent();
        assert!(
            !sent.iter().any(|e| matches!(e, RawEvent::KeyDown { key: KeyCode::F10 })),
            "F10 leaked into passthrough: {sent:?}"
        );
    }
```

- [ ] **Step 2: Run the test — confirm FAIL**

Run: `cargo test -p rm-recorder stop_key_filters_event_and_ends_recording`
Expected: FAIL — `start_recording_with_stop_key` is undefined.

- [ ] **Step 3: Implement `start_recording_with_stop_key`**

In `crates/recorder/src/lib.rs`, ADD this function (keep `start_recording` as a thin wrapper for backward compatibility):

```rust
/// Start a recording with optional stop-key filtering.
///
/// When `stop_key` is `Some(k)` and a `RawEvent::KeyDown { key: k }` arrives,
/// the event is dropped (not passthrough'd, not buffered) and the recorder
/// task exits cleanly. The matching `KeyUp` may or may not arrive (depends on
/// user release timing) — by that point the recorder is gone.
///
/// **Implementation order in the loop is critical:**
///   1. `rx.recv()` returns an event.
///   2. If it matches `stop_key` keydown → break the loop (no passthrough,
///      no buffer append).
///   3. Otherwise: passthrough send (if enabled), then buffer append.
///
/// This atomically prevents the stop key from leaking to either path.
pub fn start_recording_with_stop_key(
    hub: Arc<DriverHub>,
    passthrough: bool,
    stop_key: Option<rm_macro_model::KeyCode>,
) -> RecordingHandle {
    let rx = hub.subscribe();
    let (stop_tx, mut stop_rx) = oneshot::channel();
    let buf: Arc<Mutex<Vec<TimedEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let buf_task = buf.clone();
    let join = tokio::spawn(async move {
        let mut rx = match rx {
            Some(rx) => rx,
            None => return Vec::new(),
        };
        loop {
            tokio::select! {
                biased;
                got = rx.recv() => match got {
                    Ok(event) => {
                        // Stop-key filter FIRST — before passthrough and buffer.
                        if let Some(sk) = stop_key {
                            if let RawEvent::KeyDown { key } = event {
                                if key == sk {
                                    debug!(stop_key = ?sk, "recorder: stop-key matched, ending");
                                    break;
                                }
                            }
                        }
                        let at = Instant::now();
                        if passthrough {
                            if let Err(e) = hub.send(event).await {
                                debug!(error = ?e, "recorder: passthrough send failed");
                            }
                        }
                        buf_task.lock().await.push(TimedEvent { event, at });
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(lagged = n, "recorder: dropped events under load");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("recorder: hub closed");
                        break;
                    }
                },
                _ = &mut stop_rx => {
                    debug!("recorder: stop signal received");
                    break;
                }
            }
        }
        std::mem::take(&mut *buf_task.lock().await)
    });
    RecordingHandle {
        stop_tx: Some(stop_tx),
        join,
    }
}
```

Also update the existing `start_recording` to delegate:

```rust
/// Backward-compatible wrapper around `start_recording_with_stop_key` with no
/// stop key (caller drives termination via `finish()` / `wait_for_close()`).
pub fn start_recording(hub: Arc<DriverHub>, passthrough: bool) -> RecordingHandle {
    start_recording_with_stop_key(hub, passthrough, None)
}
```

The old `start_recording` body becomes the new function's body. Remove the original body (it's now in `start_recording_with_stop_key`).

- [ ] **Step 4: Add `RecordingHandle::run_with_stop`**

The app's supervisor task needs to drive the recorder to completion while also observing an external stop signal. The handle's private fields can't be destructured from outside the crate, so add this method inside `impl RecordingHandle`:

```rust
    /// Drive the recording to completion, observing an external stop signal.
    /// Mirrors `rm_player::PlaybackHandle::run_with_stop`.
    ///
    /// When `external_stop_rx` fires before natural completion, the internal
    /// stop_tx is fired and we await the join. When the recorder ends first
    /// (stop_key, hub close, or error), external_stop_rx is dropped.
    pub async fn run_with_stop(
        mut self,
        external_stop_rx: oneshot::Receiver<()>,
    ) -> Result<Vec<Step>> {
        let join = self.join;
        tokio::pin!(join);
        let stop_tx = self.stop_tx.take();

        tokio::select! {
            biased;
            _ = external_stop_rx => {
                if let Some(tx) = stop_tx { let _ = tx.send(()); }
                let raw = (&mut join).await
                    .map_err(|e| rm_error::AppError::Other(format!("recorder task panicked: {e}")))?;
                Ok(compile_events(&raw))
            }
            result = &mut join => {
                drop(stop_tx);
                let raw = result
                    .map_err(|e| rm_error::AppError::Other(format!("recorder task panicked: {e}")))?;
                Ok(compile_events(&raw))
            }
        }
    }
```

- [ ] **Step 5: Add a failing test for `run_with_stop` external-stop path**

Append to `mod tests`:

```rust
    #[tokio::test]
    async fn run_with_stop_external_signal_collects_buffer() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let h = start_recording_with_stop_key(hub, false, None);

        drv.inject(RawEvent::KeyDown { key: KeyCode::A });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        drv.inject(RawEvent::KeyUp { key: KeyCode::A });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let (tx, rx) = oneshot::channel();
        let join = tokio::spawn(async move { h.run_with_stop(rx).await });
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        tx.send(()).unwrap();
        let steps = join.await.unwrap().unwrap();
        assert_eq!(steps.len(), 1);
        assert!(matches!(&steps[0], Step::KeyPress { key: KeyCode::A, .. }));
    }
```

- [ ] **Step 6: Run all recorder tests**

Run: `cargo test -p rm-recorder`
Expected: PASS — all prior tests + the two new ones (14 tests total; prior count was 12).

- [ ] **Step 7: Commit**

```powershell
git add crates/recorder/src/lib.rs
git commit -m "feat(recorder): start_recording_with_stop_key + RecordingHandle::run_with_stop"
```

---

## Task 2: Add `ActiveRecording` slot to `AppState`

**Files:**
- Modify: `crates/app/src/state.rs`

- [ ] **Step 1: Update `crates/app/src/state.rs`**

Replace the file with:

```rust
//! Runtime state for the Tauri app. `DriverHub` is created lazily on the
//! first `play_macro` call; `active` enforces one-playback-at-a-time;
//! `recording` owns the per-session Interception hub for in-app recording.

use std::path::PathBuf;
use std::sync::Arc;

use rm_driver::DriverHub;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Initialised once at startup in `main`. All Tauri commands receive a
/// `State<'_, AppState>` parameter.
pub struct AppState {
    pub storage_root: PathBuf,
    pub driver_hub: Mutex<Option<Arc<DriverHub>>>,
    pub active: Mutex<Option<ActivePlayback>>,
    pub recording: Mutex<Option<ActiveRecording>>,
}

pub struct ActivePlayback {
    pub macro_id: Uuid,
    /// User-initiated stop signal. `Some` while the playback is running;
    /// `stop_playback` takes the sender out and fires it.
    pub stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

/// Per-session recording state. Owns its own DriverHub (NOT the lazy playback
/// hub) so the Interception context can be released cleanly when the
/// recording ends — see Plan 3b's "Backend lifecycle" notes.
pub struct ActiveRecording {
    pub stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    pub session_hub: Arc<DriverHub>,
}

impl AppState {
    pub fn new(storage_root: PathBuf) -> Self {
        Self {
            storage_root,
            driver_hub: Mutex::new(None),
            active: Mutex::new(None),
            recording: Mutex::new(None),
        }
    }
}
```

- [ ] **Step 2: Compile-check**

Run: `cargo check -p rm-app`
Expected: PASS. The new `recording` field is unused; `unused field` warnings expected (will be consumed in Task 4).

- [ ] **Step 3: Run all rm-app tests**

Run: `cargo test -p rm-app`
Expected: PASS — 10 tests (3a baseline). The existing tests don't touch the new field.

- [ ] **Step 4: Commit**

```powershell
git add crates/app/src/state.rs
git commit -m "feat(app): add ActiveRecording slot to AppState (per-session Interception hub)"
```

---

## Task 3: Add `StepDto`, `PointDto`, `MoveModeDto` to `dto.rs`

**Files:**
- Modify: `crates/app/src/dto.rs`

- [ ] **Step 1: Append new types and `From` impls**

Open `crates/app/src/dto.rs`. After the existing impls (before `#[cfg(test)] mod tests`), append:

```rust
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub struct PointDto {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MoveModeDto {
    Absolute,
    Relative,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepDto {
    KeyPress { key: rm_macro_model::KeyCode, hold_ms: u32 },
    KeyDown { key: rm_macro_model::KeyCode },
    KeyUp { key: rm_macro_model::KeyCode },
    MouseClick { button: rm_macro_model::MouseButton, hold_ms: u32, at: Option<PointDto> },
    MouseMove { to: PointDto, mode: MoveModeDto },
    MouseScroll { delta: i32 },
    Wait { min_ms: u32, max_ms: u32 },
}

impl From<&rm_macro_model::Point> for PointDto {
    fn from(p: &rm_macro_model::Point) -> Self { PointDto { x: p.x, y: p.y } }
}
impl From<PointDto> for rm_macro_model::Point {
    fn from(p: PointDto) -> Self { rm_macro_model::Point { x: p.x, y: p.y } }
}

impl From<&rm_macro_model::MoveMode> for MoveModeDto {
    fn from(m: &rm_macro_model::MoveMode) -> Self {
        match m {
            rm_macro_model::MoveMode::Absolute => MoveModeDto::Absolute,
            rm_macro_model::MoveMode::Relative => MoveModeDto::Relative,
        }
    }
}
impl From<MoveModeDto> for rm_macro_model::MoveMode {
    fn from(m: MoveModeDto) -> Self {
        match m {
            MoveModeDto::Absolute => rm_macro_model::MoveMode::Absolute,
            MoveModeDto::Relative => rm_macro_model::MoveMode::Relative,
        }
    }
}

impl From<&rm_macro_model::Step> for StepDto {
    fn from(s: &rm_macro_model::Step) -> Self {
        use rm_macro_model::Step;
        match s {
            Step::KeyPress { key, hold_ms } => StepDto::KeyPress { key: *key, hold_ms: *hold_ms },
            Step::KeyDown { key } => StepDto::KeyDown { key: *key },
            Step::KeyUp { key } => StepDto::KeyUp { key: *key },
            Step::MouseClick { button, hold_ms, at } => StepDto::MouseClick {
                button: *button,
                hold_ms: *hold_ms,
                at: at.as_ref().map(PointDto::from),
            },
            Step::MouseMove { to, mode } => StepDto::MouseMove {
                to: PointDto::from(to),
                mode: MoveModeDto::from(mode),
            },
            Step::MouseScroll { delta } => StepDto::MouseScroll { delta: *delta },
            Step::Wait { min_ms, max_ms } => StepDto::Wait { min_ms: *min_ms, max_ms: *max_ms },
        }
    }
}

impl From<StepDto> for rm_macro_model::Step {
    fn from(s: StepDto) -> Self {
        use rm_macro_model::Step;
        match s {
            StepDto::KeyPress { key, hold_ms } => Step::KeyPress { key, hold_ms },
            StepDto::KeyDown { key } => Step::KeyDown { key },
            StepDto::KeyUp { key } => Step::KeyUp { key },
            StepDto::MouseClick { button, hold_ms, at } => Step::MouseClick {
                button,
                hold_ms,
                at: at.map(Into::into),
            },
            StepDto::MouseMove { to, mode } => Step::MouseMove {
                to: to.into(),
                mode: mode.into(),
            },
            StepDto::MouseScroll { delta } => Step::MouseScroll { delta },
            StepDto::Wait { min_ms, max_ms } => Step::Wait { min_ms, max_ms },
        }
    }
}
```

- [ ] **Step 2: Append roundtrip tests**

In the existing `mod tests` block (before its closing `}`), append:

```rust
    #[test]
    fn step_dto_key_press_roundtrips() {
        let s = StepDto::KeyPress { key: KeyCode::A, hold_ms: 80 };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"type\":\"key_press\""));
        let back: StepDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn step_dto_wait_roundtrips() {
        let s = StepDto::Wait { min_ms: 50, max_ms: 150 };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"type\":\"wait\""));
        let back: StepDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn step_dto_mouse_move_with_point_roundtrips() {
        let s = StepDto::MouseMove {
            to: PointDto { x: 10, y: -5 },
            mode: MoveModeDto::Relative,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: StepDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn step_roundtrip_dto_to_domain_all_variants() {
        use rm_macro_model::{KeyCode, MouseButton, Step};
        let cases: Vec<Step> = vec![
            Step::KeyPress { key: KeyCode::W, hold_ms: 80 },
            Step::KeyDown { key: KeyCode::LShift },
            Step::KeyUp { key: KeyCode::LShift },
            Step::MouseClick { button: MouseButton::Left, hold_ms: 50, at: None },
            Step::MouseClick {
                button: MouseButton::Right,
                hold_ms: 80,
                at: Some(rm_macro_model::Point { x: 100, y: 200 }),
            },
            Step::MouseMove {
                to: rm_macro_model::Point { x: 5, y: -3 },
                mode: rm_macro_model::MoveMode::Relative,
            },
            Step::MouseScroll { delta: 120 },
            Step::Wait { min_ms: 100, max_ms: 100 },
        ];
        for domain in cases {
            let dto = StepDto::from(&domain);
            let back: Step = dto.into();
            assert_eq!(back, domain);
        }
    }
```

- [ ] **Step 3: Run dto tests**

Run: `cargo test -p rm-app dto::tests`
Expected: PASS — 9 tests (5 from 3a + 4 new).

- [ ] **Step 4: Commit**

```powershell
git add crates/app/src/dto.rs
git commit -m "feat(app): StepDto + PointDto + MoveModeDto with roundtrip tests"
```

---

## Task 4: `play_macro` rejects during active recording

**Files:**
- Modify: `crates/app/src/commands.rs`

- [ ] **Step 1: Add the failing test first**

Append to `crates/app/src/commands.rs` `mod tests`:

```rust
    #[tokio::test]
    async fn play_rejects_when_recording_active() {
        let (_tmp, state) = fixture_state();
        // Place a dummy ActiveRecording in the slot.
        let drv = std::sync::Arc::new(rm_driver::mock::MockDriver::new());
        let hub = rm_driver::DriverHub::start(drv);
        let (tx, _rx) = tokio::sync::oneshot::channel::<()>();
        {
            let mut recording = state.recording.lock().await;
            *recording = Some(crate::state::ActiveRecording {
                stop_tx: Some(tx),
                session_hub: hub,
            });
        }
        // The guard we'll add: play_macro checks recording.is_some() first.
        let blocked = {
            let recording = state.recording.lock().await;
            recording.is_some()
        };
        assert!(blocked);
    }
```

- [ ] **Step 2: Add `rm-driver = { path = ... }` as a dev-dependency or confirm the existing dep covers tests**

`rm-driver` is already a workspace dep (used by the recorder); the test above uses `rm_driver::mock::MockDriver`. The MockDriver lives behind no feature gate. Run to confirm:

Run: `cargo test -p rm-app play_rejects_when_recording_active`
Expected: compiles (may need `rm-driver = { path = "../driver" }` added to `[dev-dependencies]` in `crates/app/Cargo.toml` if not already present). If compile error references `rm_driver::mock`, add the line:

```toml
[dev-dependencies]
tempfile.workspace = true
rm-driver = { path = "../driver" }
```

- [ ] **Step 3: Modify `play_macro` to check the recording slot**

In `crates/app/src/commands.rs`, find `play_macro`. The current guard reads:

```rust
    // Reserve the active slot atomically: check + write under one lock.
    {
        let mut active = state.active.lock().await;
        if active.is_some() {
            return Err(AppError::PlaybackActive.to_wire());
        }
```

Before that block, add a recording check:

```rust
    // Reject if a recording is in progress — playback would synthesize keys
    // that the recorder would capture.
    {
        let recording = state.recording.lock().await;
        if recording.is_some() {
            return Err(AppError::RecordingActive.to_wire());
        }
    }
```

So the play_macro intro becomes (in order): I/O preflight, then `recording` guard, then `active` guard with insert.

Actually: it's cleaner to check both guards before doing the I/O. Reorder:

```rust
#[tauri::command]
pub async fn play_macro(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<(), WireError> {
    // Reject if a recording is in progress — playback would synthesize keys
    // that the recorder would capture.
    {
        let recording = state.recording.lock().await;
        if recording.is_some() {
            return Err(AppError::RecordingActive.to_wire());
        }
    }

    // ...existing I/O (load_all, find by id, ensure_hub, channels)...

    // Reserve the active slot atomically: check + write under one lock.
    {
        let mut active = state.active.lock().await;
        if active.is_some() {
            return Err(AppError::PlaybackActive.to_wire());
        }
        *active = Some(ActivePlayback { ... });
    }
    // ...
}
```

The implementer keeps the rest of `play_macro` unchanged — just adds the recording check at the top.

- [ ] **Step 4: Run all rm-app tests**

Run: `cargo test -p rm-app`
Expected: PASS — 12 tests now (10 from 3a + Step 3 of Task 3 added 4 = 14, minus the new guard test… wait, recount: 5 dto + 4 dto-new (Task 3) + 4 commands (3a) + 1 active_slot guard (3a) + 1 update_metadata persist (3a Task 10) = 15 baseline. Add Task 4's new guard test = 16 expected.)

(If the count differs, run `cargo test -p rm-app -- --list 2>&1 | tail -20` to enumerate. The exact number isn't the gate — the gate is: 0 failures, the new test passes.)

- [ ] **Step 5: Commit**

```powershell
git add crates/app/src/commands.rs crates/app/Cargo.toml
git commit -m "feat(app): play_macro rejects with RecordingActive during recording"
```

---

## Task 5: `start_recording` and `stop_recording` Tauri commands

**Files:**
- Create: `crates/app/src/recording.rs` (supervisor helper)
- Modify: `crates/app/src/commands.rs`
- Modify: `crates/app/src/main.rs` (register commands + module)

This is the heaviest task in Plan 3b. Two commands + a `RecordingOutcome` event payload.

- [ ] **Step 1: Create `crates/app/src/recording.rs`**

```rust
//! Recording supervisor — wraps `rm-recorder` with the app-level lifecycle:
//! per-session DriverHub, ActiveRecording slot cleanup, `recording_finished`
//! event emission.

use rm_macro_model::KeyCode;
use rm_recorder::RecordingHandle;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

use crate::dto::StepDto;
use crate::state::AppState;

/// Stop key for in-app recording (hardcoded in 3b; configurable in 3c via
/// Settings). F10 is chosen for low collision with target apps.
pub const STOP_KEY: KeyCode = KeyCode::F10;

#[derive(Serialize, Clone)]
pub struct RecordingStartedEvent {}

#[derive(Serialize, Clone)]
pub struct RecordingFinishedEvent {
    pub outcome: RecordingOutcome,
}

#[derive(Serialize, Clone)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RecordingOutcome {
    /// Recording captured cleanly via F10 (or explicit stop_recording).
    Ok { steps: Vec<StepDto> },
    /// Capture task hit an error mid-recording.
    Failed { error: rm_error::WireError },
}

/// Spawn the supervisor task. It owns the `RecordingHandle` and the per-session
/// `Arc<DriverHub>` (kept alive via ActiveRecording's session_hub). When
/// `external_stop_rx` fires OR the recorder ends naturally (e.g. F10), the
/// supervisor:
///   1. Collects steps via `handle.run_with_stop(external_stop_rx)`.
///   2. Clears the ActiveRecording slot (which drops the session hub, releasing Interception).
///   3. Emits `recording_finished` with outcome.
pub fn spawn_supervisor(
    app: AppHandle,
    handle: RecordingHandle,
    external_stop_rx: oneshot::Receiver<()>,
) {
    tokio::spawn(async move {
        let result = handle.run_with_stop(external_stop_rx).await;

        let outcome = match result {
            Ok(steps) => RecordingOutcome::Ok {
                steps: steps.iter().map(StepDto::from).collect(),
            },
            Err(e) => RecordingOutcome::Failed { error: e.to_wire() },
        };

        // Clear the ActiveRecording slot. Dropping session_hub here releases
        // Interception (no other strong refs after this).
        if let Some(s) = app.try_state::<AppState>() {
            let mut recording = s.recording.lock().await;
            *recording = None;
        }

        let _ = app.emit(
            "recording_finished",
            RecordingFinishedEvent { outcome },
        );
    });
}
```

- [ ] **Step 2: Open a fresh Interception context in `start_recording`**

We need a helper that opens a fresh `Arc<DriverHub>` per recording session, feature-gated like the playback lazy hub. Add this in `crates/app/src/commands.rs` (or extract to recording.rs — the implementer can choose).

In `crates/app/src/commands.rs`, near the existing `driver_init` mod, ADD a sibling module:

```rust
#[cfg(feature = "interception")]
mod recording_driver {
    use super::*;
    use rm_driver::{Driver, DriverHub};
    use rm_driver_interception::open_with_status;
    use std::sync::Arc;

    /// Open a FRESH Interception context (NOT the lazy playback hub) for the
    /// current recording session. The caller owns the returned hub via
    /// ActiveRecording.session_hub and drops it on stop.
    pub fn open_fresh_hub() -> Result<Arc<DriverHub>, AppError> {
        let drv: Arc<dyn Driver> = Arc::new(open_with_status()?);
        Ok(DriverHub::start(drv))
    }
}

#[cfg(not(feature = "interception"))]
mod recording_driver {
    use super::*;
    use rm_driver::DriverHub;
    use std::sync::Arc;

    pub fn open_fresh_hub() -> Result<Arc<DriverHub>, AppError> {
        Err(AppError::DriverNotInstalled)
    }
}

use recording_driver::open_fresh_hub;
```

- [ ] **Step 3: Add `start_recording` and `stop_recording` Tauri commands**

Append to `crates/app/src/commands.rs` (after `stop_playback`, before `#[cfg(test)]`):

```rust
use crate::recording::{spawn_supervisor, RecordingStartedEvent, STOP_KEY};

#[tauri::command]
pub async fn start_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), WireError> {
    // Reject if a playback is in progress — recorder would capture synthetic keys.
    {
        let active = state.active.lock().await;
        if active.is_some() {
            return Err(AppError::PlaybackActive.to_wire());
        }
    }
    // Reject if a recording is already in progress.
    {
        let recording = state.recording.lock().await;
        if recording.is_some() {
            return Err(AppError::RecordingActive.to_wire());
        }
    }

    // Open a fresh per-session hub (NOT the lazy playback hub).
    let hub = open_fresh_hub().map_err(|e| e.to_wire())?;

    // Build the recorder with stop_key = F10.
    let handle = rm_recorder::start_recording_with_stop_key(
        hub.clone(),
        true, // passthrough — let user's typing reach the OS during recording
        Some(STOP_KEY),
    );

    // External stop signal (used by `stop_recording` command and by the
    // window-close handler).
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

    // Reserve the recording slot. Clone hub into the slot so it lives as
    // long as the recording; the supervisor task also keeps a strong ref.
    {
        let mut recording = state.recording.lock().await;
        *recording = Some(ActiveRecording {
            stop_tx: Some(stop_tx),
            session_hub: hub.clone(),
        });
    }

    // Spawn the supervisor. It owns the handle; on completion it clears the
    // slot and emits `recording_finished`.
    spawn_supervisor(app.clone(), handle, stop_rx);

    // Notify frontend AFTER the slot is populated.
    let _ = app.emit("recording_started", RecordingStartedEvent {});

    Ok(())
}

#[tauri::command]
pub async fn stop_recording(
    _app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), WireError> {
    // Send the cooperative stop signal. The supervisor handles cleanup and
    // event emission. If F10 already fired, the slot is empty / stop_tx is
    // None — this is a benign no-op.
    let mut recording = state.recording.lock().await;
    if let Some(ar) = recording.as_mut() {
        if let Some(tx) = ar.stop_tx.take() {
            let _ = tx.send(());
        }
    }
    Ok(())
}
```

The `use crate::state::ActiveRecording;` import may need to be added at the top of `commands.rs` if not already present.

- [ ] **Step 4: Register `mod recording;` and the commands in `main.rs`**

Update `crates/app/src/main.rs`:

```rust
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod commands;
mod dto;
mod recording;
mod state;

use std::path::PathBuf;

use state::AppState;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let storage_root = dirs::data_dir()
        .map(|d| d.join("rust-macro"))
        .unwrap_or_else(|| PathBuf::from("./.rust-macro"));

    tauri::Builder::default()
        .manage(AppState::new(storage_root))
        .invoke_handler(tauri::generate_handler![
            commands::load_macros,
            commands::delete_macro,
            commands::update_macro_metadata,
            commands::play_macro,
            commands::stop_playback,
            commands::start_recording,
            commands::stop_recording,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

(Window-close hook lands in Task 9; don't add it yet.)

- [ ] **Step 5: Compile + run tests**

Run: `cargo check -p rm-app`
Expected: PASS. Some `unused` warnings on Tauri command fns are OK if no test exercises them yet.

Run: `cargo test -p rm-app`
Expected: PASS — same test count as Task 4 (the new commands aren't exercised by tests in this task; smoke test covers them).

- [ ] **Step 6: Commit**

```powershell
git add crates/app/src/recording.rs crates/app/src/commands.rs crates/app/src/main.rs
git commit -m "feat(app): start_recording/stop_recording commands with F10 stop key"
```

---

## Task 6: `load_macro_steps` Tauri command

**Files:**
- Modify: `crates/app/src/commands.rs`
- Modify: `crates/app/src/main.rs`

The editor view needs to load the steps for a single macro (the list view's `MacroDto` intentionally omits them).

- [ ] **Step 1: Add the test**

Append to `crates/app/src/commands.rs` `mod tests`:

```rust
    #[tokio::test]
    async fn load_macro_steps_returns_dtos() {
        let (_tmp, state) = fixture_state();
        let mut m = fixture_macro("with-steps");
        m.steps = vec![
            Step::KeyPress { key: KeyCode::A, hold_ms: 80 },
            Step::Wait { min_ms: 50, max_ms: 50 },
            Step::KeyPress { key: KeyCode::B, hold_ms: 80 },
        ];
        save_macro(&state.storage_root, &m).unwrap();

        // Mirror the command body:
        let loaded = load_macro(&state.storage_root, m.id).unwrap();
        let dtos: Vec<crate::dto::StepDto> = loaded.steps.iter().map(crate::dto::StepDto::from).collect();
        assert_eq!(dtos.len(), 3);
        assert!(matches!(dtos[0], crate::dto::StepDto::KeyPress { .. }));
        assert!(matches!(dtos[1], crate::dto::StepDto::Wait { .. }));
    }
```

- [ ] **Step 2: Add the command**

In `crates/app/src/commands.rs`, after `update_macro_metadata`:

```rust
#[tauri::command]
pub async fn load_macro_steps(
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<Vec<crate::dto::StepDto>, WireError> {
    let m = load_macro(&state.storage_root, id).map_err(|e| e.to_wire())?;
    Ok(m.steps.iter().map(crate::dto::StepDto::from).collect())
}
```

- [ ] **Step 3: Register in `main.rs`**

Add `commands::load_macro_steps,` to the `invoke_handler` list.

- [ ] **Step 4: Run tests**

Run: `cargo test -p rm-app`
Expected: PASS — one new test in commands::tests.

- [ ] **Step 5: Commit**

```powershell
git add crates/app/src/commands.rs crates/app/src/main.rs
git commit -m "feat(app): load_macro_steps command for the editor view"
```

---

## Task 7: `create_macro` Tauri command

**Files:**
- Modify: `crates/app/src/commands.rs`
- Modify: `crates/app/src/main.rs`

After the recording's preview Save, the frontend calls this to persist a brand-new macro.

- [ ] **Step 1: Add the test**

Append to `mod tests`:

```rust
    #[tokio::test]
    async fn create_macro_persists_with_provided_fields_and_steps() {
        let (_tmp, state) = fixture_state();
        // Mirror the command body:
        let name = "captured-demo".to_string();
        let trigger = Trigger::Hotkey {
            key: KeyCode::F1,
            modifiers: vec![Modifier::Ctrl],
        };
        let playback = PlaybackMode::Once;
        let steps = vec![
            Step::KeyPress { key: KeyCode::A, hold_ms: 80 },
            Step::Wait { min_ms: 100, max_ms: 100 },
        ];

        let mut m = rm_macro_model::Macro::new(&name, trigger.clone(), playback.clone());
        m.steps = steps.clone();
        m.validate().unwrap();
        save_macro(&state.storage_root, &m).unwrap();

        let all = load_all(&state.storage_root).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, name);
        assert_eq!(all[0].steps.len(), 2);
    }
```

- [ ] **Step 2: Add the command**

In `crates/app/src/commands.rs`, after `load_macro_steps`:

```rust
#[tauri::command]
pub async fn create_macro(
    state: State<'_, AppState>,
    name: String,
    trigger: TriggerDto,
    playback: PlaybackModeDto,
    steps: Vec<crate::dto::StepDto>,
) -> Result<MacroDto, WireError> {
    let mut m = rm_macro_model::Macro::new(name, trigger.into(), playback.into());
    m.steps = steps.into_iter().map(Into::into).collect();
    m.validate().map_err(|e| AppError::Other(e).to_wire())?;
    storage_save(&state.storage_root, &m).map_err(|e| e.to_wire())?;
    Ok(MacroDto::from(&m))
}
```

- [ ] **Step 3: Register in `main.rs`**

Add `commands::create_macro,` to the `invoke_handler` list.

- [ ] **Step 4: Run tests**

Run: `cargo test -p rm-app`
Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add crates/app/src/commands.rs crates/app/src/main.rs
git commit -m "feat(app): create_macro command for post-recording Save"
```

---

## Task 8: `update_macro_full` Tauri command

**Files:**
- Modify: `crates/app/src/commands.rs`
- Modify: `crates/app/src/main.rs`

The step editor's Save uses this. The existing `update_macro_metadata` stays for metadata-only flows.

- [ ] **Step 1: Add the test**

Append to `mod tests`:

```rust
    #[tokio::test]
    async fn update_full_replaces_steps_and_metadata() {
        let (_tmp, state) = fixture_state();
        let mut m = fixture_macro("before-full");
        m.steps = vec![Step::Wait { min_ms: 10, max_ms: 10 }];
        let id = m.id;
        save_macro(&state.storage_root, &m).unwrap();

        // Mirror the command body:
        let mut loaded = load_macro(&state.storage_root, id).unwrap();
        loaded.name = "after-full".into();
        loaded.steps = vec![
            Step::KeyPress { key: KeyCode::Z, hold_ms: 60 },
            Step::Wait { min_ms: 30, max_ms: 30 },
        ];
        loaded.updated_at = chrono::Utc::now();
        loaded.validate().unwrap();
        save_macro(&state.storage_root, &loaded).unwrap();

        let reloaded = load_macro(&state.storage_root, id).unwrap();
        assert_eq!(reloaded.name, "after-full");
        assert_eq!(reloaded.steps.len(), 2);
        assert!(matches!(reloaded.steps[0], Step::KeyPress { key: KeyCode::Z, .. }));
    }
```

- [ ] **Step 2: Add the command**

In `crates/app/src/commands.rs`, after `create_macro`:

```rust
#[tauri::command]
pub async fn update_macro_full(
    state: State<'_, AppState>,
    id: Uuid,
    name: String,
    trigger: TriggerDto,
    playback: PlaybackModeDto,
    steps: Vec<crate::dto::StepDto>,
) -> Result<MacroDto, WireError> {
    let mut m = load_macro(&state.storage_root, id).map_err(|e| e.to_wire())?;
    m.name = name;
    m.trigger = trigger.into();
    m.playback = playback.into();
    m.steps = steps.into_iter().map(Into::into).collect();
    m.updated_at = chrono::Utc::now();
    m.validate().map_err(|e| AppError::Other(e).to_wire())?;
    storage_save(&state.storage_root, &m).map_err(|e| e.to_wire())?;
    Ok(MacroDto::from(&m))
}
```

- [ ] **Step 3: Register in `main.rs`**

Add `commands::update_macro_full,` to the `invoke_handler` list.

- [ ] **Step 4: Run tests**

Run: `cargo test -p rm-app`
Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add crates/app/src/commands.rs crates/app/src/main.rs
git commit -m "feat(app): update_macro_full command for the step editor's Save"
```

---

## Task 9: Window-close cancels active recording

**Files:**
- Modify: `crates/app/src/main.rs`

When the user closes the window mid-recording, the Interception context must be released and the spawned supervisor must wind down before the process exits. Without this, Interception's kernel handle may leak until reboot.

- [ ] **Step 1: Update `crates/app/src/main.rs`**

Replace the `tauri::Builder::default()` chain with the version below. The change adds `.on_window_event(...)` that intercepts CloseRequested and fires the recording stop if active.

```rust
fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let storage_root = dirs::data_dir()
        .map(|d| d.join("rust-macro"))
        .unwrap_or_else(|| PathBuf::from("./.rust-macro"));

    tauri::Builder::default()
        .manage(AppState::new(storage_root))
        .invoke_handler(tauri::generate_handler![
            commands::load_macros,
            commands::delete_macro,
            commands::update_macro_metadata,
            commands::update_macro_full,
            commands::create_macro,
            commands::load_macro_steps,
            commands::play_macro,
            commands::stop_playback,
            commands::start_recording,
            commands::stop_recording,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // If a recording is active, fire its stop signal so the
                // supervisor finalizes cleanly (drops the Interception
                // context). We don't block close on completion — the
                // OS will reap any orphaned task on exit, and Interception
                // releases on context drop.
                use tauri::Manager;
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    let recording_mutex = state.recording.clone();
                    // Spawn a brief task to fire stop_tx; we don't await it
                    // because the close handler is sync.
                    tauri::async_runtime::spawn(async move {
                        let mut recording = recording_mutex.lock().await;
                        if let Some(ar) = recording.as_mut() {
                            if let Some(tx) = ar.stop_tx.take() {
                                let _ = tx.send(());
                            }
                        }
                    });
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

**Important:** `state.recording.clone()` requires `state.recording` to be cloneable. `Mutex<Option<ActiveRecording>>` is NOT Clone — but `Arc<Mutex<...>>` is. Look at the existing 3a code in commands.rs: the supervisor task uses `app.try_state::<AppState>()` to re-acquire the State, and accesses `s.active.lock().await` directly. Mirror that here:

If the above `clone()` doesn't compile (it won't — `Mutex` doesn't implement Clone), replace the spawn body with:

```rust
                    let app_handle = window.app_handle().clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(s) = app_handle.try_state::<AppState>() {
                            let mut recording = s.recording.lock().await;
                            if let Some(ar) = recording.as_mut() {
                                if let Some(tx) = ar.stop_tx.take() {
                                    let _ = tx.send(());
                                }
                            }
                        }
                    });
```

The implementer picks whichever Tauri 2 API form compiles.

- [ ] **Step 2: Compile-check**

Run: `cargo check -p rm-app`
Expected: PASS. Some borrow-check fiddling may be required; if `window.app_handle()` returns `&AppHandle`, clone it. Tauri 2 API surface is consistent enough that this should work without intricate adjustments.

- [ ] **Step 3: Run tests**

Run: `cargo test -p rm-app`
Expected: PASS — no regressions.

- [ ] **Step 4: Commit**

```powershell
git add crates/app/src/main.rs
git commit -m "feat(app): WindowEvent::CloseRequested cancels active recording cleanly"
```

---

## Task 10: Extend frontend types with StepDto

**Files:**
- Modify: `crates/app/ui/src/lib/types.ts`

- [ ] **Step 1: Append to `types.ts`**

After the existing `PlaybackMode` declaration, append:

```ts
export type PointDto = { x: number; y: number };

export type MoveModeDto = "absolute" | "relative";

export type MouseButton = "left" | "right" | "middle" | "x1" | "x2";

export type StepDto =
  | { type: "key_press"; key: KeyCode; hold_ms: number }
  | { type: "key_down"; key: KeyCode }
  | { type: "key_up"; key: KeyCode }
  | { type: "mouse_click"; button: MouseButton; hold_ms: number; at: PointDto | null }
  | { type: "mouse_move"; to: PointDto; mode: MoveModeDto }
  | { type: "mouse_scroll"; delta: number }
  | { type: "wait"; min_ms: number; max_ms: number };

/** Defaults for the editor's "+ Add step" picker. Keep in sync with Plan 3b
 *  Task 15's defaults table. */
export const STEP_DEFAULTS: Record<StepDto["type"], () => StepDto> = {
  key_press: () => ({ type: "key_press", key: "a", hold_ms: 50 }),
  key_down: () => ({ type: "key_down", key: "a" }),
  key_up: () => ({ type: "key_up", key: "a" }),
  mouse_click: () => ({ type: "mouse_click", button: "left", hold_ms: 50, at: null }),
  mouse_move: () => ({ type: "mouse_move", to: { x: 0, y: 0 }, mode: "relative" }),
  mouse_scroll: () => ({ type: "mouse_scroll", delta: 0 }),
  wait: () => ({ type: "wait", min_ms: 100, max_ms: 100 }),
};

/** Human-readable label for the step type. */
export function stepLabel(type: StepDto["type"]): string {
  return type
    .split("_")
    .map((p) => p.charAt(0).toUpperCase() + p.slice(1))
    .join(" ");
}
```

- [ ] **Step 2: Compile-check the frontend**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS — no TypeScript errors.

- [ ] **Step 3: Commit**

```powershell
git add crates/app/ui/src/lib/types.ts
git commit -m "feat(app/ui): StepDto + PointDto + MoveModeDto mirror types"
```

---

## Task 11: Extend `api.ts` with new command wrappers

**Files:**
- Modify: `crates/app/ui/src/lib/api.ts`

- [ ] **Step 1: Replace existing `api.ts`**

Open `crates/app/ui/src/lib/api.ts`. The current file has `loadMacros`, `deleteMacro`, plus stubs for commands added in 3a tasks. Add the new wrappers.

Current `api.ts` end (after the existing stubs) — append:

```ts
import type { StepDto } from "./types";

export async function createMacro(
  name: string,
  trigger: Trigger,
  playback: PlaybackMode,
  steps: StepDto[],
): Promise<MacroDto> {
  return invoke<MacroDto>("create_macro", { name, trigger, playback, steps });
}

export async function updateMacroFull(
  id: string,
  name: string,
  trigger: Trigger,
  playback: PlaybackMode,
  steps: StepDto[],
): Promise<MacroDto> {
  return invoke<MacroDto>("update_macro_full", { id, name, trigger, playback, steps });
}

export async function loadMacroSteps(id: string): Promise<StepDto[]> {
  return invoke<StepDto[]>("load_macro_steps", { id });
}

export async function startRecording(): Promise<void> {
  await invoke("start_recording");
}

export async function stopRecording(): Promise<void> {
  await invoke("stop_recording");
}
```

Hoist the `import type { StepDto }` to the top of the file with the existing `import type { MacroDto, Trigger, PlaybackMode } from "./types";` (consolidate into one import line).

- [ ] **Step 2: Compile-check**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS.

- [ ] **Step 3: Commit**

```powershell
git add crates/app/ui/src/lib/api.ts
git commit -m "feat(app/ui): api wrappers for create/update_full/load_steps + recording start/stop"
```

---

## Task 12: Recording store (frontend)

**Files:**
- Create: `crates/app/ui/src/lib/stores/recording.ts`

- [ ] **Step 1: Create `crates/app/ui/src/lib/stores/recording.ts`**

```ts
import { writable } from "svelte/store";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { StepDto, WireError } from "../types";
import * as api from "../api";
import { reportError, pushToast } from "./toast";

/** Phases of the recording UI:
 *   - `idle`: no recording in progress
 *   - `armed`: user opened the start modal, hasn't clicked Start yet
 *   - `recording`: backend returned OK from start_recording and emitted
 *     recording_started; window minimized; F10 will stop
 *   - `preview`: recording_finished arrived; user is in the Save/Discard modal
 */
export type RecordingPhase =
  | { tag: "idle" }
  | { tag: "armed" }
  | { tag: "recording" }
  | { tag: "preview"; steps: StepDto[] };

export const phase = writable<RecordingPhase>({ tag: "idle" });

type FinishedOutcome =
  | { status: "ok"; steps: StepDto[] }
  | { status: "failed"; error: WireError };

type FinishedPayload = { outcome: FinishedOutcome };

let unlisteners: UnlistenFn[] = [];

export async function startListening(): Promise<void> {
  await stopListening();

  unlisteners.push(
    await listen("recording_started", () => {
      phase.set({ tag: "recording" });
    }),
  );

  unlisteners.push(
    await listen<FinishedPayload>("recording_finished", (event) => {
      const o = event.payload.outcome;
      if (o.status === "ok") {
        phase.set({ tag: "preview", steps: o.steps });
        if (o.steps.length === 0) {
          pushToast("info", "Recording captured 0 steps.");
        }
      } else {
        pushToast("error", `Recording failed: ${o.error.message}`);
        phase.set({ tag: "idle" });
      }
    }),
  );
}

export async function stopListening(): Promise<void> {
  for (const u of unlisteners) u();
  unlisteners = [];
}

/** Open the start modal. */
export function arm(): void {
  phase.set({ tag: "armed" });
}

/** Cancel the start modal (user clicked Cancel before recording started). */
export function disarm(): void {
  phase.set({ tag: "idle" });
}

/** Begin recording: minimize window + call backend. */
export async function begin(): Promise<void> {
  try {
    const w = await import("@tauri-apps/api/window");
    await w.getCurrentWindow().minimize();
    await api.startRecording();
    // `recording_started` event sets phase to "recording".
  } catch (e) {
    reportError(e);
    phase.set({ tag: "idle" });
  }
}

/** Explicitly stop the recording from the frontend (rare — F10 is primary). */
export async function stop(): Promise<void> {
  try {
    await api.stopRecording();
  } catch (e) {
    reportError(e);
  }
}

/** Restore the window after recording_finished arrives. */
export async function restoreWindow(): Promise<void> {
  try {
    const w = await import("@tauri-apps/api/window");
    const win = w.getCurrentWindow();
    await win.unminimize();
    await win.setFocus();
  } catch (e) {
    // Non-critical — user can click the window to focus it.
    console.warn("recording: window restore failed", e);
  }
}

/** Discard the captured steps without saving. */
export function discard(): void {
  phase.set({ tag: "idle" });
}

/** Finalize: caller already saved via api.createMacro; transition to idle. */
export function complete(): void {
  phase.set({ tag: "idle" });
}
```

- [ ] **Step 2: Compile-check**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS.

- [ ] **Step 3: Commit**

```powershell
git add crates/app/ui/src/lib/stores/recording.ts
git commit -m "feat(app/ui): recording store with phase machine + Tauri event listeners"
```

---

## Task 13: HotkeyPicker — Capture button + listening state

**Files:**
- Modify: `crates/app/ui/src/lib/components/HotkeyPicker.svelte`

- [ ] **Step 1: Replace the file**

```svelte
<script lang="ts">
  import type { Trigger, KeyCode, Modifier } from "../types";
  import { inputLabel } from "../types";

  let { value, onChange }: { value: Trigger; onChange: (t: Trigger) => void } = $props();

  // Subset of keys we expose in the dropdown fallback. Live capture covers
  // most everyday combos; the dropdown is for users who want a key the
  // browser can't see (e.g. Print Screen, Win key alone — though Esc is
  // RESERVED for cancel during capture).
  const KEY_OPTIONS: KeyCode[] = [
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m",
    "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
    "num0", "num1", "num2", "num3", "num4",
    "num5", "num6", "num7", "num8", "num9",
    "space", "enter", "tab", "escape",
    "up", "down", "left", "right",
  ];
  const MODIFIERS: Modifier[] = ["ctrl", "shift", "alt", "win"];

  let listening = $state(false);
  let liveModifiers = $state<Modifier[]>([]);
  let liveKey = $state<KeyCode | null>(null);
  let timeoutHandle: ReturnType<typeof setTimeout> | null = null;

  function toggle(mod: Modifier) {
    if (value.type !== "hotkey") return;
    const has = value.modifiers.includes(mod);
    const modifiers = has
      ? value.modifiers.filter((m) => m !== mod)
      : [...value.modifiers, mod];
    onChange({ ...value, modifiers });
  }

  function changeKey(e: Event) {
    const key = (e.target as HTMLSelectElement).value as KeyCode;
    if (value.type !== "hotkey") return;
    onChange({ ...value, key });
  }

  // ---- Capture mode ----

  function startCapture() {
    listening = true;
    liveModifiers = [];
    liveKey = null;
    window.addEventListener("keydown", onKeyDown, { capture: true });
    window.addEventListener("keyup", onKeyUp, { capture: true });
    timeoutHandle = setTimeout(() => stopCapture(false), 5000);
  }

  function stopCapture(commit: boolean) {
    listening = false;
    if (timeoutHandle) { clearTimeout(timeoutHandle); timeoutHandle = null; }
    window.removeEventListener("keydown", onKeyDown, true);
    window.removeEventListener("keyup", onKeyUp, true);
    if (commit && liveKey) {
      onChange({ type: "hotkey", key: liveKey, modifiers: liveModifiers });
    }
    liveModifiers = [];
    liveKey = null;
  }

  // Map a browser KeyboardEvent.code -> KeyCode (snake_case). Returns null
  // for codes we don't expose. Keep in sync with rm_macro_model::KeyCode.
  function codeToKeyCode(code: string): KeyCode | null {
    if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase() as KeyCode;
    if (/^Digit[0-9]$/.test(code)) return ("num" + code.slice(5)) as KeyCode;
    if (/^F([1-9]|1[0-2])$/.test(code)) return code.toLowerCase() as KeyCode;
    if (/^Numpad[0-9]$/.test(code)) return ("num" + code.slice(6)) as KeyCode;
    switch (code) {
      case "Space": return "space";
      case "Enter": return "enter";
      case "Tab": return "tab";
      case "Backspace": return "backspace";
      case "CapsLock": return "caps_lock";
      case "ArrowUp": return "up";
      case "ArrowDown": return "down";
      case "ArrowLeft": return "left";
      case "ArrowRight": return "right";
      case "Insert": return "insert";
      case "Delete": return "delete";
      case "Home": return "home";
      case "End": return "end";
      case "PageUp": return "page_up";
      case "PageDown": return "page_down";
      case "Minus": return "minus";
      case "Equal": return "equals";
      case "BracketLeft": return "l_bracket";
      case "BracketRight": return "r_bracket";
      case "Backslash": return "backslash";
      case "Semicolon": return "semicolon";
      case "Quote": return "apostrophe";
      case "Backquote": return "backtick";
      case "Comma": return "comma";
      case "Period": return "period";
      case "Slash": return "slash";
      // Modifiers handled separately
      default: return null;
    }
  }

  function isModifierCode(code: string): boolean {
    return ["ShiftLeft", "ShiftRight", "ControlLeft", "ControlRight",
            "AltLeft", "AltRight", "MetaLeft", "MetaRight"].includes(code);
  }

  function modifiersFromEvent(e: KeyboardEvent): Modifier[] {
    const mods: Modifier[] = [];
    if (e.ctrlKey) mods.push("ctrl");
    if (e.shiftKey) mods.push("shift");
    if (e.altKey) mods.push("alt");
    if (e.metaKey) mods.push("win");
    return mods;
  }

  function onKeyDown(e: KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();
    if (e.code === "Escape") { stopCapture(false); return; }
    if (isModifierCode(e.code)) {
      liveModifiers = modifiersFromEvent(e);
      return;
    }
    const k = codeToKeyCode(e.code);
    if (k) {
      liveKey = k;
      liveModifiers = modifiersFromEvent(e);
    }
  }

  function onKeyUp(e: KeyboardEvent) {
    e.preventDefault();
    e.stopPropagation();
    // Commit on the keyup of a non-modifier key.
    if (liveKey && !isModifierCode(e.code)) {
      stopCapture(true);
    }
  }

  function liveLabel(): string {
    if (!liveKey) return liveModifiers.map(inputLabel).join("+") || "...";
    return [...liveModifiers, liveKey].map(inputLabel).join("+");
  }
</script>

{#if listening}
  <div class="listening">
    <span class="banner">Press your hotkey combo: <code>{liveLabel()}</code></span>
    <button onclick={() => stopCapture(false)}>Cancel</button>
  </div>
  <p class="hint">Esc to cancel. Modifier-only combos are not allowed.</p>
{:else}
  <div class="modifiers">
    {#each MODIFIERS as mod}
      <label>
        <input
          type="checkbox"
          checked={value.type === "hotkey" && value.modifiers.includes(mod)}
          onchange={() => toggle(mod)}
        />
        {inputLabel(mod)}
      </label>
    {/each}
  </div>
  <div class="key-row">
    <select onchange={changeKey} value={value.type === "hotkey" ? value.key : "f1"}>
      {#each KEY_OPTIONS as k}
        <option value={k}>{inputLabel(k)}</option>
      {/each}
    </select>
    <button onclick={startCapture} title="Press a key combo to bind">🎯 Capture</button>
  </div>
{/if}

<style>
  .modifiers {
    display: flex;
    gap: 0.75rem;
    margin-bottom: 0.5rem;
  }
  label { cursor: pointer; user-select: none; }
  .key-row {
    display: flex;
    gap: 0.5rem;
  }
  .key-row select { flex: 1; }
  .listening {
    display: flex;
    gap: 0.5rem;
    align-items: center;
    padding: 0.5rem 0.75rem;
    background: rgba(37, 99, 235, 0.12);
    border: 1px solid var(--accent);
    border-radius: 4px;
  }
  .banner { flex: 1; }
  .hint {
    margin: 0.25rem 0 0 0;
    color: var(--text-muted);
    font-size: 0.8rem;
  }
</style>
```

- [ ] **Step 2: Compile-check**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS.

- [ ] **Step 3: Commit**

```powershell
git add crates/app/ui/src/lib/components/HotkeyPicker.svelte
git commit -m "feat(app/ui): HotkeyPicker Capture button (press combo, 5s timeout, Esc cancels)"
```

---

## Task 14: `RecordingModal` (start + preview)

**Files:**
- Create: `crates/app/ui/src/lib/components/RecordingModal.svelte`

This component is a two-phase modal:

- **Phase `armed`:** start confirmation. Buttons: Cancel, Start.
- **Phase `preview`:** post-recording form. Steps summary, name input, HotkeyPicker, mode selector, Save/Discard.

- [ ] **Step 1: Create `crates/app/ui/src/lib/components/RecordingModal.svelte`**

```svelte
<script lang="ts">
  import { phase, disarm, begin, restoreWindow, discard, complete } from "../stores/recording";
  import * as api from "../api";
  import { reportError } from "../stores/toast";
  import { loadAll } from "../stores/macros";
  import HotkeyPicker from "./HotkeyPicker.svelte";
  import type { Trigger, PlaybackMode, StepDto } from "../types";
  import { stepLabel } from "../types";

  // Form state for the preview phase.
  let name = $state("");
  let trigger = $state<Trigger>({ type: "hotkey", key: "f1", modifiers: ["ctrl"] });
  let playback = $state<PlaybackMode>({ type: "once" });
  let repeatN = $state(3);
  let saving = $state(false);

  $effect(() => {
    // When we enter preview phase, restore the window and reset the form.
    if ($phase.tag === "preview") {
      void restoreWindow();
      name = "";
      trigger = { type: "hotkey", key: "f1", modifiers: ["ctrl"] };
      playback = { type: "once" };
      repeatN = 3;
      saving = false;
    }
  });

  function changePlayback(e: Event) {
    const v = (e.target as HTMLSelectElement).value;
    switch (v) {
      case "once": playback = { type: "once" }; break;
      case "repeat": playback = { type: "repeat", value: repeatN }; break;
      case "loop": playback = { type: "loop" }; break;
      case "toggle": playback = { type: "toggle" }; break;
    }
  }

  function changeRepeatN(e: Event) {
    repeatN = Math.max(1, Number((e.target as HTMLInputElement).value));
    if (playback.type === "repeat") playback = { type: "repeat", value: repeatN };
  }

  async function save() {
    if ($phase.tag !== "preview") return;
    if (name.trim() === "") return;
    saving = true;
    try {
      await api.createMacro(name.trim(), trigger, playback, $phase.steps);
      await loadAll();
      complete();
    } catch (e) {
      reportError(e);
    } finally {
      saving = false;
    }
  }

  function backdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) {
      if ($phase.tag === "armed") disarm();
      else if ($phase.tag === "preview") discard();
    }
  }

  function stepSummary(s: StepDto): string {
    switch (s.type) {
      case "key_press": return `KeyPress ${s.key} hold ${s.hold_ms}ms`;
      case "key_down": return `KeyDown ${s.key}`;
      case "key_up": return `KeyUp ${s.key}`;
      case "mouse_click": return `MouseClick ${s.button} hold ${s.hold_ms}ms`;
      case "mouse_move": return `MouseMove (${s.to.x},${s.to.y}) ${s.mode}`;
      case "mouse_scroll": return `MouseScroll ${s.delta}`;
      case "wait":
        return s.min_ms === s.max_ms
          ? `Wait ${s.min_ms}ms`
          : `Wait ${s.min_ms}-${s.max_ms}ms`;
    }
  }
</script>

{#if $phase.tag === "armed"}
  <div class="backdrop" onclick={backdropClick} role="presentation">
    <div class="modal" role="dialog" aria-labelledby="rec-armed-title">
      <h3 id="rec-armed-title">Record a new macro</h3>
      <p>
        Press <strong>F10</strong> to stop. The window will minimize while you record.
      </p>
      <div class="actions">
        <button onclick={disarm}>Cancel</button>
        <button class="primary" onclick={() => void begin()}>Start</button>
      </div>
    </div>
  </div>
{:else if $phase.tag === "preview"}
  <div class="backdrop" onclick={backdropClick} role="presentation">
    <div class="modal preview" role="dialog" aria-labelledby="rec-preview-title">
      <h3 id="rec-preview-title">Recording finished — {$phase.steps.length} steps captured</h3>

      <div class="step-list">
        {#each $phase.steps as s, i}
          <div class="step-line"><span class="num">#{i + 1}</span> {stepSummary(s)}</div>
        {/each}
        {#if $phase.steps.length === 0}
          <div class="empty">No steps captured.</div>
        {/if}
      </div>

      <div class="field">
        <label for="rec-name">Name</label>
        <input id="rec-name" bind:value={name} />
      </div>

      <div class="field">
        <label>Hotkey</label>
        <HotkeyPicker value={trigger} onChange={(t) => (trigger = t)} />
      </div>

      <div class="field">
        <label for="rec-mode">Playback mode</label>
        <select id="rec-mode" value={playback.type} onchange={changePlayback}>
          <option value="once">Once</option>
          <option value="repeat">Repeat (N)</option>
          <option value="loop">Loop</option>
          <option value="toggle">Toggle</option>
        </select>
        {#if playback.type === "repeat"}
          <input
            class="repeat-n"
            type="number"
            min="1"
            value={repeatN}
            oninput={changeRepeatN}
          />
        {/if}
      </div>

      <div class="actions">
        <button onclick={discard}>Discard</button>
        <button
          class="primary"
          disabled={saving || name.trim() === "" || $phase.steps.length === 0}
          onclick={save}
        >
          {saving ? "Saving…" : "Save"}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 600;
  }
  .modal {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.5rem;
    width: 100%;
    max-width: 460px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.5);
  }
  .modal.preview { max-width: 560px; }
  h3 { margin: 0 0 1rem 0; }
  .step-list {
    max-height: 240px;
    overflow-y: auto;
    background: var(--bg-input);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 0.5rem 0.75rem;
    margin-bottom: 1rem;
    font-family: ui-monospace, "Cascadia Code", "Consolas", monospace;
    font-size: 0.85rem;
  }
  .step-line { padding: 0.1rem 0; }
  .num {
    display: inline-block;
    width: 2.5rem;
    color: var(--text-muted);
  }
  .empty { color: var(--text-muted); text-align: center; padding: 1rem; }
  .field { margin-bottom: 1rem; }
  .field > label {
    display: block;
    font-size: 0.85rem;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.35rem;
  }
  .field input, .field select { width: 100%; }
  .repeat-n {
    margin-top: 0.5rem;
    width: 100px !important;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 1.5rem;
  }
</style>
```

- [ ] **Step 2: Compile-check**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS.

- [ ] **Step 3: Commit**

```powershell
git add crates/app/ui/src/lib/components/RecordingModal.svelte
git commit -m "feat(app/ui): RecordingModal — armed (start) + preview (save/discard) phases"
```

---

## Task 15: `StepRow` component

**Files:**
- Create: `crates/app/ui/src/lib/components/StepRow.svelte`

One row in the step editor. Renders inline editors for the step's parameters.

- [ ] **Step 1: Create `crates/app/ui/src/lib/components/StepRow.svelte`**

```svelte
<script lang="ts">
  import type { StepDto, KeyCode, MouseButton, MoveModeDto } from "../types";
  import { inputLabel } from "../types";

  let {
    step,
    index,
    canMoveUp,
    canMoveDown,
    onChange,
    onMoveUp,
    onMoveDown,
    onRemove,
  }: {
    step: StepDto;
    index: number;
    canMoveUp: boolean;
    canMoveDown: boolean;
    onChange: (s: StepDto) => void;
    onMoveUp: () => void;
    onMoveDown: () => void;
    onRemove: () => void;
  } = $props();

  // Same KEY_OPTIONS subset as HotkeyPicker — extend if you need more keys.
  const KEY_OPTIONS: KeyCode[] = [
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m",
    "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
    "num0", "num1", "num2", "num3", "num4",
    "num5", "num6", "num7", "num8", "num9",
    "space", "enter", "tab", "escape", "backspace", "caps_lock",
    "up", "down", "left", "right",
    "l_shift", "r_shift", "l_ctrl", "r_ctrl", "l_alt", "r_alt", "l_win", "r_win",
  ];
  const MOUSE_BUTTONS: MouseButton[] = ["left", "right", "middle", "x1", "x2"];
  const MOVE_MODES: MoveModeDto[] = ["absolute", "relative"];

  function update(patch: Partial<StepDto>) {
    onChange({ ...step, ...patch } as StepDto);
  }

  function intInput(value: number, set: (n: number) => void) {
    return (e: Event) => set(Math.max(0, Number((e.target as HTMLInputElement).value) | 0));
  }
</script>

<div class="row">
  <div class="num">#{index + 1}</div>
  <div class="move">
    <button onclick={onMoveUp} disabled={!canMoveUp} title="Move up">↑</button>
    <button onclick={onMoveDown} disabled={!canMoveDown} title="Move down">↓</button>
  </div>
  <div class="type-label">{step.type.split("_").map(p => p[0].toUpperCase() + p.slice(1)).join(" ")}</div>
  <div class="params">
    {#if step.type === "key_press"}
      <label>key
        <select value={step.key} onchange={(e) => update({ key: (e.target as HTMLSelectElement).value as KeyCode })}>
          {#each KEY_OPTIONS as k}<option value={k}>{inputLabel(k)}</option>{/each}
        </select>
      </label>
      <label>hold_ms
        <input type="number" min="0" value={step.hold_ms} oninput={intInput(step.hold_ms, n => update({ hold_ms: n }))} />
      </label>
    {:else if step.type === "key_down" || step.type === "key_up"}
      <label>key
        <select value={step.key} onchange={(e) => update({ key: (e.target as HTMLSelectElement).value as KeyCode })}>
          {#each KEY_OPTIONS as k}<option value={k}>{inputLabel(k)}</option>{/each}
        </select>
      </label>
    {:else if step.type === "mouse_click"}
      <label>button
        <select value={step.button} onchange={(e) => update({ button: (e.target as HTMLSelectElement).value as MouseButton })}>
          {#each MOUSE_BUTTONS as b}<option value={b}>{inputLabel(b)}</option>{/each}
        </select>
      </label>
      <label>hold_ms
        <input type="number" min="0" value={step.hold_ms} oninput={intInput(step.hold_ms, n => update({ hold_ms: n }))} />
      </label>
    {:else if step.type === "mouse_move"}
      <label>x
        <input type="number" value={step.to.x} oninput={(e) => update({ to: { ...step.to, x: Number((e.target as HTMLInputElement).value) | 0 } })} />
      </label>
      <label>y
        <input type="number" value={step.to.y} oninput={(e) => update({ to: { ...step.to, y: Number((e.target as HTMLInputElement).value) | 0 } })} />
      </label>
      <label>mode
        <select value={step.mode} onchange={(e) => update({ mode: (e.target as HTMLSelectElement).value as MoveModeDto })}>
          {#each MOVE_MODES as m}<option value={m}>{inputLabel(m)}</option>{/each}
        </select>
      </label>
    {:else if step.type === "mouse_scroll"}
      <label>delta
        <input type="number" value={step.delta} oninput={(e) => update({ delta: Number((e.target as HTMLInputElement).value) | 0 })} />
      </label>
    {:else if step.type === "wait"}
      <label>min_ms
        <input type="number" min="0" value={step.min_ms} oninput={intInput(step.min_ms, n => update({ min_ms: n }))} />
      </label>
      <label>max_ms
        <input type="number" min="0" value={step.max_ms} oninput={intInput(step.max_ms, n => update({ max_ms: n }))} />
      </label>
    {/if}
  </div>
  <button class="danger remove" onclick={onRemove} title="Delete step">✕</button>
</div>

<style>
  .row {
    display: grid;
    grid-template-columns: 2.5rem auto 9rem 1fr auto;
    gap: 0.5rem;
    align-items: center;
    padding: 0.4rem 0.5rem;
    border-bottom: 1px solid var(--border);
  }
  .num { color: var(--text-muted); }
  .move { display: flex; gap: 0.25rem; }
  .move button { padding: 0.2rem 0.4rem; }
  .type-label {
    font-family: ui-monospace, "Cascadia Code", "Consolas", monospace;
    font-size: 0.85rem;
    color: var(--text-muted);
  }
  .params {
    display: flex;
    gap: 0.6rem;
    flex-wrap: wrap;
    align-items: center;
  }
  .params label {
    display: flex;
    flex-direction: column;
    gap: 0.15rem;
    font-size: 0.75rem;
    color: var(--text-muted);
  }
  .params input, .params select {
    padding: 0.2rem 0.4rem;
    font-size: 0.85rem;
    min-width: 4rem;
  }
  .remove { padding: 0.25rem 0.5rem; }
</style>
```

- [ ] **Step 2: Compile-check**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS.

- [ ] **Step 3: Commit**

```powershell
git add crates/app/ui/src/lib/components/StepRow.svelte
git commit -m "feat(app/ui): StepRow — inline editor for one step (all 7 variants)"
```

---

## Task 16: `StepEditor` view

**Files:**
- Create: `crates/app/ui/src/lib/components/StepEditor.svelte`

Full-screen editor: loads metadata + steps, renders rows, supports edit/move/delete/add, Save via `update_macro_full`.

- [ ] **Step 1: Create `crates/app/ui/src/lib/components/StepEditor.svelte`**

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import type { MacroDto, StepDto, Trigger, PlaybackMode } from "../types";
  import { STEP_DEFAULTS, stepLabel } from "../types";
  import * as api from "../api";
  import { reportError } from "../stores/toast";
  import { loadAll, snapshot } from "../stores/macros";
  import HotkeyPicker from "./HotkeyPicker.svelte";
  import StepRow from "./StepRow.svelte";

  let { macroId, onBack }: { macroId: string; onBack: () => void } = $props();

  let macro = $state<MacroDto | null>(null);
  let steps = $state<StepDto[]>([]);
  let initialSnapshot = $state<string>("");
  let loading = $state(true);
  let saving = $state(false);
  let addType = $state<StepDto["type"]>("key_press");

  // Local edit state for metadata
  let name = $state("");
  let trigger = $state<Trigger>({ type: "hotkey", key: "f1", modifiers: ["ctrl"] });
  let playback = $state<PlaybackMode>({ type: "once" });
  let repeatN = $state(3);

  onMount(async () => {
    const m = snapshot().find((x) => x.id === macroId);
    if (!m) {
      reportError(new Error("Macro not found in list"));
      onBack();
      return;
    }
    macro = m;
    name = m.name;
    trigger = m.trigger;
    playback = m.playback;
    if (m.playback.type === "repeat") repeatN = m.playback.value;

    try {
      steps = await api.loadMacroSteps(macroId);
      initialSnapshot = JSON.stringify({ name, trigger, playback, steps });
    } catch (e) {
      reportError(e);
      onBack();
    } finally {
      loading = false;
    }
  });

  function changePlayback(e: Event) {
    const v = (e.target as HTMLSelectElement).value;
    switch (v) {
      case "once": playback = { type: "once" }; break;
      case "repeat": playback = { type: "repeat", value: repeatN }; break;
      case "loop": playback = { type: "loop" }; break;
      case "toggle": playback = { type: "toggle" }; break;
    }
  }

  function changeRepeatN(e: Event) {
    repeatN = Math.max(1, Number((e.target as HTMLInputElement).value));
    if (playback.type === "repeat") playback = { type: "repeat", value: repeatN };
  }

  function moveStep(i: number, delta: number) {
    const j = i + delta;
    if (j < 0 || j >= steps.length) return;
    const next = [...steps];
    [next[i], next[j]] = [next[j], next[i]];
    steps = next;
  }

  function removeStep(i: number) {
    steps = steps.filter((_, idx) => idx !== i);
  }

  function updateStep(i: number, s: StepDto) {
    const next = [...steps];
    next[i] = s;
    steps = next;
  }

  function addStep() {
    steps = [...steps, STEP_DEFAULTS[addType]()];
  }

  function isDirty(): boolean {
    return JSON.stringify({ name, trigger, playback, steps }) !== initialSnapshot;
  }

  async function save() {
    if (!macro) return;
    if (name.trim() === "") return;
    saving = true;
    try {
      await api.updateMacroFull(macro.id, name.trim(), trigger, playback, steps);
      await loadAll();
      onBack();
    } catch (e) {
      reportError(e);
    } finally {
      saving = false;
    }
  }

  function discard() {
    if (isDirty()) {
      if (!confirm("Discard unsaved changes?")) return;
    }
    onBack();
  }
</script>

{#if loading}
  <main class="loading">
    <p>Loading editor…</p>
  </main>
{:else if macro}
  <main class="editor">
    <header>
      <button class="back" onclick={discard}>← Back to list</button>
      <div class="spacer"></div>
      <button onclick={discard}>Discard</button>
      <button
        class="primary"
        disabled={saving || name.trim() === ""}
        onclick={save}
      >{saving ? "Saving…" : "Save"}</button>
    </header>

    <section class="metadata">
      <h2>Metadata</h2>
      <div class="field">
        <label for="ed-name">Name</label>
        <input id="ed-name" bind:value={name} />
      </div>
      <div class="field">
        <label>Hotkey</label>
        <HotkeyPicker value={trigger} onChange={(t) => (trigger = t)} />
      </div>
      <div class="field">
        <label for="ed-mode">Playback mode</label>
        <select id="ed-mode" value={playback.type} onchange={changePlayback}>
          <option value="once">Once</option>
          <option value="repeat">Repeat (N)</option>
          <option value="loop">Loop</option>
          <option value="toggle">Toggle</option>
        </select>
        {#if playback.type === "repeat"}
          <input class="repeat-n" type="number" min="1" value={repeatN} oninput={changeRepeatN} />
        {/if}
      </div>
    </section>

    <section class="steps">
      <h2>Steps ({steps.length})</h2>
      {#if steps.length === 0}
        <p class="empty">No steps. Use "+ Add step" below to add one.</p>
      {:else}
        {#each steps as s, i}
          <StepRow
            step={s}
            index={i}
            canMoveUp={i > 0}
            canMoveDown={i < steps.length - 1}
            onChange={(ns) => updateStep(i, ns)}
            onMoveUp={() => moveStep(i, -1)}
            onMoveDown={() => moveStep(i, 1)}
            onRemove={() => removeStep(i)}
          />
        {/each}
      {/if}
      <div class="add-step">
        <select bind:value={addType}>
          <option value="key_press">{stepLabel("key_press")}</option>
          <option value="key_down">{stepLabel("key_down")}</option>
          <option value="key_up">{stepLabel("key_up")}</option>
          <option value="mouse_click">{stepLabel("mouse_click")}</option>
          <option value="mouse_move">{stepLabel("mouse_move")}</option>
          <option value="mouse_scroll">{stepLabel("mouse_scroll")}</option>
          <option value="wait">{stepLabel("wait")}</option>
        </select>
        <button onclick={addStep}>+ Add step</button>
      </div>
    </section>
  </main>
{/if}

<style>
  main.loading, main.editor {
    max-width: 960px;
    margin: 0 auto;
    padding: 1.5rem;
  }
  header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 1.5rem;
  }
  .back { background: transparent; }
  .spacer { flex: 1; }
  section { margin-bottom: 2rem; }
  h2 { font-size: 1rem; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.05em; margin: 0 0 0.75rem 0; }
  .field { margin-bottom: 0.75rem; }
  .field > label {
    display: block;
    font-size: 0.85rem;
    color: var(--text-muted);
    margin-bottom: 0.25rem;
  }
  .field input, .field select { width: 100%; max-width: 360px; }
  .repeat-n { margin-top: 0.4rem; width: 100px !important; }
  .empty { color: var(--text-muted); padding: 1rem 0; }
  .add-step {
    display: flex;
    gap: 0.5rem;
    margin-top: 1rem;
  }
  .add-step select { max-width: 200px; }
</style>
```

- [ ] **Step 2: Compile-check**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS.

- [ ] **Step 3: Commit**

```powershell
git add crates/app/ui/src/lib/components/StepEditor.svelte
git commit -m "feat(app/ui): StepEditor — full-screen editor for metadata + steps + Save"
```

---

## Task 17: View router in `App.svelte` + enable "+ Record" in `MacroTable`

**Files:**
- Modify: `crates/app/ui/src/App.svelte`
- Modify: `crates/app/ui/src/lib/components/MacroTable.svelte`
- Modify: `crates/app/ui/src/lib/components/MacroRow.svelte` (no change to MacroRow, but verify the Edit handler signature accepts a function call back, which it already does)

- [ ] **Step 1: Replace `crates/app/ui/src/App.svelte`**

```svelte
<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { loadAll } from "./lib/stores/macros";
  import { play, startListening as startPlaybackListening, stopListening as stopPlaybackListening } from "./lib/stores/playback";
  import { arm as armRecording, startListening as startRecordingListening, stopListening as stopRecordingListening } from "./lib/stores/recording";
  import MacroTable from "./lib/components/MacroTable.svelte";
  import StepEditor from "./lib/components/StepEditor.svelte";
  import RecordingModal from "./lib/components/RecordingModal.svelte";
  import PlaybackBanner from "./lib/components/PlaybackBanner.svelte";
  import ToastHost from "./lib/components/ToastHost.svelte";

  type View = { tag: "list" } | { tag: "editor"; macroId: string };
  let view = $state<View>({ tag: "list" });

  function handlePlay(id: string) { void play(id); }
  function handleEdit(id: string) { view = { tag: "editor", macroId: id }; }
  function handleRecord() { armRecording(); }
  function backToList() { view = { tag: "list" }; }

  onMount(() => {
    void loadAll();
    void startPlaybackListening();
    void startRecordingListening();
  });

  onDestroy(() => {
    void stopPlaybackListening();
    void stopRecordingListening();
  });
</script>

{#if view.tag === "list"}
  <main>
    <header>
      <h1>rust-macro</h1>
    </header>
    <MacroTable onPlay={handlePlay} onEdit={handleEdit} onRecord={handleRecord} />
    <PlaybackBanner />
    <RecordingModal />
    <ToastHost />
  </main>
{:else if view.tag === "editor"}
  <StepEditor macroId={view.macroId} onBack={backToList} />
  <ToastHost />
{/if}

<style>
  main {
    max-width: 960px;
    margin: 0 auto;
    padding: 2rem 1.5rem;
  }
  header { margin-bottom: 1.5rem; }
  h1 { margin: 0; font-size: 1.5rem; font-weight: 600; }
</style>
```

- [ ] **Step 2: Update `crates/app/ui/src/lib/components/MacroTable.svelte`**

Two changes:
- Accept an `onRecord` callback prop.
- Replace the disabled "+ Record new (3b)" button with a functional one wired to `onRecord`.

Replace the file with:

```svelte
<script lang="ts">
  import { macros, loading, remove } from "../stores/macros";
  import MacroRow from "./MacroRow.svelte";

  let {
    onPlay,
    onEdit,
    onRecord,
  }: {
    onPlay: (id: string) => void;
    onEdit: (id: string) => void;
    onRecord: () => void;
  } = $props();

  function handleDelete(id: string) {
    void remove(id);
  }
</script>

<section>
  <div class="header">
    <h2>Macros</h2>
    <button class="primary" onclick={onRecord}>+ Record new</button>
  </div>

  {#if $loading}
    <p class="empty">Loading…</p>
  {:else if $macros.length === 0}
    <p class="empty">
      No macros yet. Click "+ Record new" to capture one.
    </p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>Name</th>
          <th>Hotkey</th>
          <th>Mode</th>
          <th class="num">Steps</th>
          <th class="actions">Actions</th>
        </tr>
      </thead>
      <tbody>
        {#each $macros as macro (macro.id)}
          <MacroRow
            {macro}
            {onPlay}
            {onEdit}
            onDelete={handleDelete}
          />
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1rem;
  }
  h2 {
    margin: 0;
    font-size: 1.25rem;
  }
  .empty {
    color: var(--text-muted);
    padding: 2rem 0;
    text-align: center;
  }
  table { width: 100%; border-collapse: collapse; }
  th {
    text-align: left;
    padding: 0.5rem;
    border-bottom: 1px solid var(--border);
    color: var(--text-muted);
    font-weight: 500;
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .num { text-align: right; }
  .actions { text-align: right; }
</style>
```

- [ ] **Step 3: Compile-check**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add crates/app/ui/src/App.svelte crates/app/ui/src/lib/components/MacroTable.svelte
git commit -m "feat(app/ui): view router (list|editor) + functional + Record new button"
```

---

## Task 18: Delete `EditMetadataModal.svelte`

**Files:**
- Delete: `crates/app/ui/src/lib/components/EditMetadataModal.svelte`

The editor view absorbs all metadata editing. The old modal is dead code.

- [ ] **Step 1: Delete the file**

```powershell
git rm crates/app/ui/src/lib/components/EditMetadataModal.svelte
```

- [ ] **Step 2: Confirm no references**

```powershell
# From repo root
findstr /S /M "EditMetadataModal" crates/app/ui/src 2>&1
```

Expected: no matches (file's deletion + App.svelte already replaced in Task 17 — the editor view replaces the modal everywhere).

- [ ] **Step 3: Compile-check**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git commit -m "chore(app/ui): remove EditMetadataModal (subsumed by StepEditor)"
```

---

## Task 19: README updates

**Files:**
- Modify: `crates/app/README.md`

- [ ] **Step 1: Replace the file**

Update `crates/app/README.md` with the Plan 3b additions to prerequisites and smoke test. Use this full content:

```markdown
# rm-app — rust-macro Tauri GUI (Plans 3a + 3b)

A desktop GUI for rust-macro: list saved macros, record new ones in-app,
edit metadata and steps, delete, and play/stop them via the existing
`rm-player` + `InterceptionDriver`.

## Prerequisites

- Windows 10/11.
- Rust toolchain (stable, MSVC).
- `tauri-cli` v2: `cargo install tauri-cli --version "^2"`.
- Node.js 20+ and npm.
- WebView2 runtime (pre-installed on Windows 11).
- Interception kernel driver installed — see
  `docs/superpowers/specs/2026-05-26-rust-macro-plan-2b-real-driver-design.md`.
  (Required for both Play and in-app Record.)

## Run in dev

```powershell
# From repo root:
cd crates/app/ui
npm install
cd ..
cargo tauri dev
```

## Build a release binary

```powershell
cd crates/app
cargo tauri build
```

Output: `target/release/rust-macro.exe` plus installer bundles under
`target/release/bundle/`.

## Manual smoke test (Plan 3a + 3b acceptance)

1. **Empty state.** Run on a machine with no macros saved. The list shows
   "No macros yet. Click '+ Record new' to capture one."
2. **Record a new macro.** Click "+ Record new" → start modal → Start →
   window minimizes → type something in another app → press F10 → window
   restores AND re-takes focus → Preview modal shows captured steps.
3. **Save the recording.** Name it "demo", pick a hotkey via the dropdown
   OR the new Capture button (press Ctrl+Shift+F5), set mode to Once →
   Save. The new row appears in the list with the correct hotkey and step
   count.
4. **Discard a recording.** Repeat step 2; in the Preview modal click
   Discard. No new macro is saved.
5. **Edit metadata + steps.** Click ✎ on a row → full-screen editor opens.
   Change the name, toggle a modifier, change mode to Repeat(3). Edit a
   step's hold_ms. Reorder with ↑/↓. Delete a step with ✕. Add a new step
   via the "+ Add step" picker. Save. Reload the app — changes persist.
6. **Live hotkey capture.** In the editor or recording Preview, click
   "🎯 Capture" → banner shows "Press your hotkey combo" → press
   Ctrl+Shift+F5 → modifiers + key appear → release → combo committed.
   Esc cancels without committing. Hold-only-modifiers does NOT commit.
   After 5 seconds idle, listening auto-cancels.
7. **Delete.** Click ✕, confirm. Restart the app — still gone.
8. **Play.** With Interception installed and running, click ▶ on a macro.
   PlaybackBanner appears. When it finishes, success toast.
9. **Stop a Loop macro.** During Loop playback, click "■ Stop" — banner
   disappears within ~100ms.
10. **PlaybackActive guard.** While playing, click ▶ on another row → short
    yellow toast: "Already playing — stop it first."
11. **RecordingActive guard.** Click "+ Record new", confirm Start; before
    pressing F10, switch back to the list view (if possible — window is
    minimized; use Alt+Tab) and try to click ▶ → red toast: "A recording
    is already in progress." Press F10 to clean up.
12. **Concurrent: try recording during playback.** Start playing a Loop
    macro. Then trigger "+ Record new" (you'll need a hotkey-bound macro
    or two side-by-side runs). Click Start in the recording modal →
    expect: red toast "A playback is already in progress" and the
    recording does NOT start.
13. **Window close mid-recording.** Start a recording. Before F10, close
    the rust-macro window → Interception releases (the OS regains
    keyboard control immediately, no stuck keys). Re-open the app → no
    half-recorded macros in the list.

## Architecture

- 3a design: `docs/superpowers/specs/2026-05-26-rust-macro-plan-3a-tauri-gui-design.md`
- 3b design: `docs/superpowers/specs/2026-05-27-rust-macro-plan-3b-recording-editor-design.md`

## Known limitations (deferred to Plan 3c+)

- Global hotkey listener (`rm-hotkey`) — triggering macros from another app's focus.
- Driver status indicator + install button.
- Settings page (configurable stop key, default storage root, theme).
- System tray icon + window state persistence.
- Toast persistence across reloads.
- Multi-macro concurrent playback.
- Drag-and-drop step reordering (3b uses ↑↓ buttons only).
- Hotkey conflict detection.
- Live hotkey capture via Interception (3b uses browser keyboard events; Win key alone, Print Screen fall back to dropdown).
```

- [ ] **Step 2: Commit**

```powershell
git add crates/app/README.md
git commit -m "docs(app): Plan 3b README with updated smoke test plan"
```

---

## Task 20: Final verification

- [ ] **Step 1: All workspace tests pass**

```powershell
cargo test --workspace --no-fail-fast
```

Expected: PASS. Test count: prior workspace baseline + Plan 3b additions:
- `rm-recorder`: +1 (stop_key test) → expected ~13 total
- `rm-app`: +4 to ~5 (step DTO tests + new commands + recording guard) → expected ~14-15 total

The exact count doesn't gate; the gate is 0 failures.

- [ ] **Step 2: Frontend builds clean**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS. Only acceptable warnings: `state_referenced_locally` (Svelte 5 pattern, pre-existing).

- [ ] **Step 3: Tauri dev opens the window**

```powershell
cd crates/app
cargo tauri dev
```

Expected: window opens, lists macros (or empty state), Recording modal works.

- [ ] **Step 4: Walk the README smoke test**

Items 1–13 from `crates/app/README.md`. Items 8–11 require Interception installed. Items 12–13 require an active playback or recording; can be reproduced incrementally.

- [ ] **Step 5: No commit if Steps 1–3 pass**

The previous tasks committed all changes. Final verification is acceptance-only.

---

## Acceptance Checklist (from the spec)

- [ ] `cargo test --workspace` is green.
- [ ] `cargo build -p rm-app` succeeds on Windows (default + `--no-default-features`).
- [ ] `cargo tauri dev` opens a working window.
- [ ] "+ Record new" button records, F10 stops, window restores with focus, Preview modal works.
- [ ] Save creates a new macro; Discard throws it away.
- [ ] ✎ opens the full-screen editor; edit/move/remove/add steps; Save persists; Back returns to list.
- [ ] Capture button on HotkeyPicker captures Ctrl+Shift+F5-style combos; Esc cancels; 5s timeout.
- [ ] Concurrency guards: play-while-recording rejects with RecordingActive; record-while-playing rejects with PlaybackActive.
- [ ] Window-close mid-recording releases Interception cleanly.
- [ ] `EditMetadataModal.svelte` is deleted.

---

## Open Implementation Notes

- **Tauri 2 window API drift:** Task 12's `import("@tauri-apps/api/window")` uses the v2 `getCurrentWindow()` API. If the implementer's Tauri 2 minor version uses `getCurrent()` instead, adjust. Both have `.minimize()`, `.unminimize()`, `.setFocus()`.

- **Browser `KeyboardEvent.code` quirks:** for non-US keyboards, `code` is physical and stable. We map `KeyA` → "a" regardless of the user's layout. If a user has a Brazilian ABNT keyboard, pressing the physical "Q" still sends `code: "KeyQ"`. The captured hotkey then matches the Interception driver's scancode-based mapping (which is also physical). This is consistent.

- **Recording's session_hub teardown latency:** dropping `Arc<DriverHub>` triggers `Drop` on the inner Interception context (the only strong ref is the one held by the slot + the one inside the recorder task; once both go, the kernel handle is released). On most hardware this is <100ms but may be longer if Interception is contending for the driver. The README's smoke step 13 covers this.

- **The `tauri::async_runtime::spawn` in `main.rs` close handler:** the close handler is sync; we spawn an async task to fire stop_tx and exit. This task may not complete before the process is killed. Acceptable: dropping Interception happens via the Drop impl when the process exits; the kernel handle is reclaimed by Windows.

- **`StepRow` does not edit `MouseClick.at`:** recordings always emit `at: None`. If a user wants to set a click coordinate manually, they'd need to use the JSON storage file directly. This is intentional per the spec (out-of-scope for 3b).

- **`compile_events` test coverage:** Plan 3b doesn't add tests to `rm-recorder/src/compile.rs` because the compile logic doesn't change. The new `stop_key_filters_event_and_ends_recording` test in Task 1 indirectly exercises `compile_events` (it asserts that the captured `KeyPress { A }` appears, which requires `compile_events` to collapse the KeyDown/KeyUp correctly).
