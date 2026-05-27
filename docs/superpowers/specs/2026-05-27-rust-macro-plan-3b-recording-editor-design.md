# rust-macro — Plan 3b: in-app recording + step editor + live hotkey capture (Design)

**Status:** spec draft 2026-05-27. Successor to Plan 3a (Tauri GUI shipped 2026-05-27).

**Goal:** Close the loop "create and edit macros entirely inside the GUI, without the CLI". Plan 3a delivered the macro manager (list, edit metadata, delete, play/stop). Plan 3b adds the three missing pieces that make the GUI self-sufficient for macro authoring.

**Scope (three features):**

1. **In-app recording** — record a new macro from inside the app
2. **Step editor** — edit the steps of an existing macro (not just metadata)
3. **Live hotkey capture** — bind a hotkey by pressing the combo, not picking from a dropdown

**Deferred to Plan 3c+:** global hotkey integration (`rm-hotkey`), driver status UI / install button, settings page, tray icon, window state persistence, toast persistence across reloads, multi-macro concurrent playback.

**Predecessor spec:** `docs/superpowers/specs/2026-05-26-rust-macro-plan-3a-tauri-gui-design.md`.

---

## Architecture overview

Plan 3a's architecture stays as-is. Plan 3b adds:

- **Routing in the frontend** — hand-rolled `currentView` state in `App.svelte`. Two views: `list` (today's macro manager) and `editor { macroId }` (new full-screen edit view). No router library — overkill for two views.
- **`ActiveRecording` slot** in `AppState` — mirrors `ActivePlayback`. Single-recording-at-a-time enforced by a `Mutex<Option<ActiveRecording>>`.
- **Tauri events** — `recording_started`, `recording_finished { steps }`. Same lifecycle pattern as `playback_started` / `playback_finished`.
- **New Tauri commands** — `start_recording`, `stop_recording`, `create_macro`, `update_macro_full`. Plus possibly retiring `update_macro_metadata` in favor of `update_macro_full` (steps optional).

No new crates. `rm-recorder` may need a small extension (a "stop key" hook) but the design favors keeping the stop-key logic at the app-level wrapper rather than baking it into the generic recorder crate.

---

## Feature 1 — In-app recording

### User flow

```
1. User clicks the "+ Record" button in MacroTable's header
   (the button exists today, disabled with "(3b)" label — enable it).
2. Recording-start modal opens:
   • Title: "Record a new macro"
   • Body: "Press F10 to stop. The window will minimize while you record."
   • Buttons: [Cancel]  [Start]
3. User clicks Start:
   • Frontend calls `start_recording` Tauri command.
   • Backend opens the Interception driver (lazy, same as play_macro), 
     spawns a capture task with passthrough=true, populates ActiveRecording slot,
     emits `recording_started` event.
   • Frontend listens for `recording_started`, calls window.minimize() via
     @tauri-apps/api/window.
4. User works in the target application. The capture task records every key
   and mouse event via Interception. Events pass through to the OS so the
   user sees their own input in the target app.
5. User presses F10:
   • The capture task observes the F10 keydown, discards it from the
     captured step list, and fires its stop signal.
   • Recording finalizes; the captured steps are collected.
   • Backend clears ActiveRecording slot, emits 
     `recording_finished { steps: Vec<Step> }`.
6. Frontend receives `recording_finished`:
   • Restores the window via window.unminimize() / window.setFocus().
   • Opens the Preview modal.
7. Preview modal:
   • Header: "Recording finished — N steps captured"
   • Step list (read-only) — basic summary, scrollable.
   • Name input (required)
   • HotkeyPicker (default Ctrl+F1)
   • Playback mode picker (default Once)
   • Buttons: [Discard]  [Save]
8. Save: frontend calls `create_macro(name, trigger, playback, steps)` →
   new macro persisted, modal closes, list refreshes, new row appears.
   Discard: modal closes without saving. Steps are dropped.
```

### Stop key (F10)

F10 is hardcoded in 3b. The choice is justified by:
- It is rarely used in target apps (compared to Esc, F1, F12 which conflict often).
- It does not collide with common dev tools shortcuts.
- It is unambiguous to communicate ("press F10").

Configurable stop key is a Settings concern (3c). Until then, the F10 constant lives in `rm-app` (not exposed via config).

The capture task detects F10 at the event-reception level:
- When a `RawEvent::KeyDown { key: KeyCode::F10 }` arrives, the task:
  - Does NOT append it to the captured step list.
  - Fires its internal stop signal.
  - Exits its loop cleanly.
- A trailing `KeyUp { key: F10 }` from the user's release should also be discarded.

The decision to discard the F10 keydown lives in the app-level wrapper around `rm-recorder`, not in `rm-recorder` itself. The recorder remains a generic event-capture primitive.

### Concurrency guards

`start_recording` rejects in two cases:

| Condition | Error | Why |
|-----------|-------|-----|
| `active_playback.is_some()` | `AppError::PlaybackActive.to_wire()` | Recorder would capture the playback's synthetic keys → corrupt recording |
| `active_recording.is_some()` | `AppError::RecordingActive.to_wire()` | Single-recording-at-a-time |

`play_macro` is **not** modified to reject during recording — this is unidirectional. Rationale: a user trying to start recording while a macro plays gets a clear "stop the playback first" error; a user trying to play while recording is unusual and unsupported, but the Interception fresh-context model means it would just fail to open a second context (the recording owns its own).

Actually — both guards are bidirectional for safety. `play_macro` also rejects with `RecordingActive` if recording is in progress. The lock-acquisition order in both commands is: `active_playback` first, then `active_recording`. This prevents lock-ordering deadlocks.

### Window-close cancels recording

When the user closes the rust-macro window while recording is active, the recording is **cancelled** (captured steps discarded, no Preview modal). Implementation: in `main.rs`, Tauri's window event handler intercepts `WindowEvent::CloseRequested` and:

1. If `state.active.recording.is_some()`: fire stop on its `stop_tx`, await the supervisor (with timeout), then allow close.
2. Otherwise: allow close immediately.

Rationale: a half-recording is more likely junk than something the user wants saved. The Preview modal is the explicit save gesture; if the user closes the window before reaching it, the intent was to abandon.

### Backend lifecycle

```
ActiveRecording {
    stop_tx: Option<oneshot::Sender<()>>,
    // (no macro_id — we don't persist anything until Save)
}

start_recording():
    Acquire BOTH locks (active_playback, active_recording) in a consistent
    order. (Use the order: active_playback first, then active_recording —
    matches what play_macro already does.)
    If active_playback.is_some() → return Err(AppError::PlaybackActive)
        (cannot record while playing — recorder would capture synthesized
        keystrokes from the playback).
    If active_recording.is_some() → return Err(AppError::RecordingActive).

    Open a FRESH Interception context for this recording session (do NOT
    reuse the lazy hub from playback). Drop on stop. Rationale: after F10
    fires and the recorder exits, the recorder is gone but Interception is
    still capturing keys (no passthrough), so any user typing during the
    teardown window is silently dropped. Owning + dropping per-session
    bounds that gap to ~100-300ms (Interception context close) and isolates
    the recording lifecycle from playback's reused hub.

    Build channels: (stop_tx, stop_rx).
    Call rm_recorder::start_recording_with_stop_key(hub, true, KeyCode::F10).
    Insert ActiveRecording { stop_tx: Some(stop_tx), session_hub: Arc<DriverHub> }.
    Release the active_recording lock.
    Spawn supervisor task:
        Race recording_handle.run completion against an external stop_rx
            (so the frontend's stop_recording command also works).
        On stop: take steps from the handle, drop the session_hub
            (releases Interception), clear ActiveRecording slot via
            try_state, emit `recording_finished` with RecordingOutcome::Ok
            (or Failed if the recorder errored).
    Emit `recording_started`.
    Ok(())

stop_recording():
    Send on the stop_tx in the slot. Supervisor handles the rest.
    (This command is unused by F10 path — only by explicit user action,
     e.g. a future "Stop recording" button if we add one.)
```

### Create-macro persistence

The `create_macro` Tauri command:

```rust
#[tauri::command]
pub async fn create_macro(
    state: State<'_, AppState>,
    name: String,
    trigger: TriggerDto,
    playback: PlaybackModeDto,
    steps: Vec<StepDto>,  // new DTO mirror of rm_macro_model::Step
) -> Result<MacroDto, WireError> {
    let mut m = Macro::new(name, trigger.into(), playback.into());
    m.steps = steps.into_iter().map(Into::into).collect();
    m.validate().map_err(|e| AppError::Other(e).to_wire())?;
    save_macro(&state.storage_root, &m).map_err(|e| e.to_wire())?;
    Ok(MacroDto::from(&m))
}
```

This requires a new `StepDto` (and discriminated-union mirrors of `KeyCode`/`MouseButton`/`MoveMode`/`Point`). The DTOs follow the same pattern as `TriggerDto` and `PlaybackModeDto` from 3a.

---

## Feature 2 — Step editor (full-screen view)

### Routing

`App.svelte` switches between two views via a state machine:

```ts
type View =
  | { tag: "list" }
  | { tag: "editor"; macroId: string };

let view = $state<View>({ tag: "list" });
```

- `MacroRow`'s ✎ button no longer opens `EditMetadataModal`. Instead it sets `view = { tag: "editor", macroId: id }`.
- The full-screen editor mounts when `view.tag === "editor"`, fetches the macro by id, and renders the layout below.
- Save / Discard / Back returns `view = { tag: "list" }`.

`EditMetadataModal` is retired (its inputs are absorbed into the full-screen editor). The metadata fields live at the top of the editor view; steps live below.

### Layout

```
┌──────────────────────────────────────────────────────────────┐
│ ← Back to list                       [Discard]  [Save]       │
├──────────────────────────────────────────────────────────────┤
│ Metadata                                                     │
│   Name:    [_____________________]                           │
│   Hotkey:  [Ctrl] [Shift]☐ [Alt]☐ [Win]☐  [F1 ▾]  [🎯 Capture]│
│   Mode:    [Once ▾]   (if Repeat: [N=3])                     │
├──────────────────────────────────────────────────────────────┤
│ Steps (12)                                                   │
│  #1  ↑↓  KeyPress     key:[A ▾]     hold_ms:[80    ]     ✕   │
│  #2  ↑↓  Wait         min_ms:[50]   max_ms:[150     ]    ✕   │
│  #3  ↑↓  MouseClick   button:[L▾]   hold_ms:[50     ]    ✕   │
│  #4  ↑↓  MouseMove    x:[100] y:[200] mode:[Absolute ▾]  ✕   │
│  #5  ↑↓  MouseScroll  delta:[120  ]                      ✕   │
│  #6  ↑↓  KeyDown      key:[LShift ▾]                     ✕   │
│  #7  ↑↓  KeyUp        key:[LShift ▾]                     ✕   │
│  ...                                                         │
│  [+ Add step  ▾]   (dropdown: KeyPress | Wait | ... )       │
└──────────────────────────────────────────────────────────────┘
```

### Per-step controls

- **↑ ↓**: move step one position. ↑ disabled on first step, ↓ disabled on last.
- **✕**: remove step. No confirmation for now (Save / Discard at the top makes mistakes recoverable until Save).
- **Parameter inputs**: inline editors specific to the step type:
  - `KeyPress` / `KeyDown` / `KeyUp` → key dropdown (reuse the full `KEY_OPTIONS` list from 3a)
  - `KeyPress` → also `hold_ms` integer input
  - `MouseClick` → button dropdown (Left/Right/Middle/X1/X2) + `hold_ms`. Drop `at: Option<Point>` from the editor for simplicity (it's always recorded as `None` by passthrough recording).
  - `MouseMove` → `x`, `y` integer inputs + mode dropdown (Absolute/Relative)
  - `MouseScroll` → `delta` integer
  - `Wait` → `min_ms`, `max_ms` integer inputs. Validation: `min_ms ≤ max_ms` (use the existing `Step::validate()` rule on Save).

### Add step

`[+ Add step ▾]` is a dropdown / split-button at the bottom. Clicking it opens a small menu:

```
+ Add step
├─ KeyPress
├─ KeyDown
├─ KeyUp
├─ MouseClick
├─ MouseMove
├─ MouseScroll
└─ Wait
```

Picking a type appends a new step at the end with these defaults:

| Step | Default values |
|------|----------------|
| KeyPress | `key: A, hold_ms: 50` |
| KeyDown | `key: A` |
| KeyUp | `key: A` |
| MouseClick | `button: Left, hold_ms: 50, at: None` |
| MouseMove | `to: { x: 0, y: 0 }, mode: Relative` |
| MouseScroll | `delta: 0` |
| Wait | `min_ms: 100, max_ms: 100` |

The user then edits the parameters inline as usual.

**Step variants come pre-correct from the recorder.** `rm-recorder::compile_events` already collapses adjacent `KeyDown(k) → KeyUp(k)` into `KeyPress` and emits raw `KeyDown`/`KeyUp` for overlapping inputs (see test `overlapping_keys_emit_raw_down_up` in `crates/recorder/src/compile.rs`). The editor's row template just needs to render whatever variants happen to be in the macro; no additional collapse / expand logic.

**Concurrent edit policy:** editor uses last-write-wins. No optimistic-concurrency check (single-window app; no contention possible from this codebase).

### Save behavior

- Save calls `update_macro_full(id, name, trigger, playback, steps)`. This replaces today's `update_macro_metadata` (which only handles the metadata fields). The new command accepts the whole macro shape and writes it via the existing storage layer.
- On Save error (validation failure, IO), surface a toast and stay on the editor.
- On Save success, return to the list view; the row reflects the new step count.

### Discard behavior

- If the user has not made changes (computed by comparing current state to the initial loaded macro), Discard is a no-op return to list.
- If changes exist, show a browser `confirm()`: "Discard unsaved changes?". Confirm → return to list. Cancel → stay.

### Routing & navigation gotchas

- **Direct list refresh while editing:** the list store re-fetches on `loadAll()`. If a `playback_finished` event fires while the editor is open and triggers a list reload, the editor's local state must remain stable. The editor holds its own copy of the macro (not the store's), so a list reload doesn't disturb it.
- **Deleting from elsewhere:** out of scope. Only one window, one user.

---

## Feature 3 — Live hotkey capture

### UI changes to HotkeyPicker

The existing `HotkeyPicker` (modifiers checkboxes + key dropdown) stays. Add a single new button `🎯 Capture` to the right of the dropdown.

States of the picker:

1. **Idle** (today's behavior): modifiers + dropdown + Capture button.
2. **Listening:** picker shows a banner "Press your hotkey combo (Esc to cancel)". The dropdown and checkboxes hide; only the banner + `[Cancel]` button are visible.
3. **Captured** (transient, ~300ms): banner shows the captured combo in big text, then auto-commits and returns to Idle with the new value set.

Esc at any time during Listening cancels back to Idle without changing the value.

**Validation rules during capture:**

- Capture requires **at least one non-modifier key**. Modifier-only combos (e.g. Shift alone) cannot be committed — picker stays in Listening until a non-modifier is pressed or Esc cancels.
- 5-second timeout: if no non-modifier keydown arrives within 5 seconds of entering Listening, picker auto-cancels back to Idle. Prevents the picker from getting stuck if the user clicks Capture and walks away.
- Esc is reserved for Cancel — cannot be bound as a hotkey via Capture. (The dropdown still allows `escape` as a fallback if a user really wants it.)

### Capture implementation

Browser-level only:

```svelte
<script lang="ts">
  let listening = $state(false);
  let liveModifiers = $state<Modifier[]>([]);
  let liveKey = $state<KeyCode | null>(null);

  function startCapture() {
    listening = true;
    window.addEventListener("keydown", onKeyDown, { capture: true });
    window.addEventListener("keyup", onKeyUp, { capture: true });
  }

  function stopCapture(commit: boolean) {
    listening = false;
    window.removeEventListener("keydown", onKeyDown, { capture: true } as any);
    window.removeEventListener("keyup", onKeyUp, { capture: true } as any);
    if (commit && liveKey) onChange({ type: "hotkey", key: liveKey, modifiers: liveModifiers });
    liveModifiers = []; liveKey = null;
  }

  function onKeyDown(e: KeyboardEvent) {
    e.preventDefault();
    if (e.key === "Escape") { stopCapture(false); return; }
    // Map e.code / e.key to KeyCode + Modifier values...
    // Build liveModifiers from e.ctrlKey/shiftKey/altKey/metaKey
    // liveKey from non-modifier KeyCode
  }

  function onKeyUp(e: KeyboardEvent) {
    if (liveKey) stopCapture(true);
  }
</script>
```

Key mapping table (browser `KeyboardEvent.code` → `KeyCode` snake_case strings):

| Browser `code`         | KeyCode (Rust)  |
|-----------------------|-----------------|
| KeyA..KeyZ            | a..z            |
| Digit0..Digit9        | num0..num9      |
| F1..F12               | f1..f12         |
| Space                 | space           |
| Enter                 | enter           |
| Tab                   | tab             |
| Backspace             | backspace       |
| Escape                | escape — handled as cancel, not bound |
| ArrowUp/Down/Left/Right | up/down/left/right |
| Insert/Delete/Home/End/PageUp/PageDown | insert/delete/home/end/page_up/page_down |
| Minus/Equal/BracketLeft/etc | minus/equals/l_bracket/... |

Modifier mapping from event flags:

| Flag            | Modifier |
|-----------------|----------|
| e.ctrlKey       | ctrl     |
| e.shiftKey      | shift    |
| e.altKey        | alt      |
| e.metaKey       | win      |

Unsupported by browser (Print Screen, Win key sólo, function lock, etc.): the user falls back to the dropdown, which remains available. The Capture button does NOT replace the dropdown — it sits next to it as an alternate input method.

### EditMetadataModal retirement

Since Feature 2 moves all editing to the full-screen editor, `EditMetadataModal.svelte` is deleted. The Capture button lives inside the editor's metadata section.

---

## DTOs added in 3b

`crates/app/src/dto.rs` gains:

```rust
#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StepDto {
    KeyPress { key: KeyCode, hold_ms: u32 },
    KeyDown { key: KeyCode },
    KeyUp { key: KeyCode },
    MouseClick { button: MouseButton, hold_ms: u32, at: Option<PointDto> },
    MouseMove { to: PointDto, mode: MoveModeDto },
    MouseScroll { delta: i32 },
    Wait { min_ms: u32, max_ms: u32 },
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
pub struct PointDto { pub x: i32, pub y: i32 }

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MoveModeDto { Absolute, Relative }

// From/Into impls mirror Trigger/PlaybackMode pattern from 3a.
```

`MacroDto` gains an optional `steps: Option<Vec<StepDto>>` field for when the editor needs them (toggle via a new command `load_macro_full(id) -> MacroDto`), or alternatively we add a separate command `load_macro_steps(id) -> Vec<StepDto>` and keep `MacroDto.step_count` as it is. **Decision: separate command** — keeps `load_macros` cheap for the list view.

---

## Tauri commands — additions and changes

| Command | Status | Purpose |
|---------|--------|---------|
| `load_macros` | unchanged | List view (no steps) |
| `load_macro_steps(id)` | NEW | Editor needs the steps for the chosen macro |
| `create_macro(name, trigger, playback, steps)` | NEW | After successful recording, persist as new |
| `update_macro_full(id, name, trigger, playback, steps)` | NEW | Step editor's Save (full payload incl. steps) |
| `update_macro_metadata` | UNCHANGED | Kept for metadata-only edits — smaller payload, used by future hotkey-only flows |
| `delete_macro` | unchanged | |
| `play_macro` | MODIFIED | Also rejects with `RecordingActive` if recording is in progress |
| `stop_playback` | unchanged | |
| `start_recording` | NEW | Open Interception, begin capture |
| `stop_recording` | NEW | Explicit stop (F10 path doesn't need this) |

---

## Tauri events — additions

| Event | Payload | When |
|-------|---------|------|
| `recording_started` | `(none)` | After `start_recording` succeeds |
| `recording_finished` | `{ outcome: RecordingOutcome }` (see below) | When the capture task ends |
| `playback_started` | (existing) | |
| `playback_finished` | (existing) | |

`RecordingOutcome` shape (mirrors `PlaybackOutcome` from 3a):

```rust
#[derive(Serialize, Clone)]
#[serde(tag = "status", rename_all = "snake_case")]
enum RecordingOutcome {
    /// Recording captured cleanly via F10 (or explicit stop_recording).
    Ok { steps: Vec<StepDto> },
    /// Capture task hit an error mid-recording.
    Failed { error: WireError },
}
```

No `Stopped` variant — both F10 and `stop_recording` are "ok" paths (they deliver captured steps). `Failed` is only for unexpected errors (driver lost, etc.).

---

## State changes

`crates/app/src/state.rs`:

```rust
pub struct AppState {
    pub storage_root: PathBuf,
    pub driver_hub: Mutex<Option<Arc<DriverHub>>>,
    pub active: Mutex<Option<ActivePlayback>>,
    pub recording: Mutex<Option<ActiveRecording>>,   // NEW
}

pub struct ActiveRecording {
    pub stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// Per-session DriverHub. Owned by the slot so it lives as long as the
    /// recording does; dropped when the slot is cleared on stop. NOT shared
    /// with the lazy playback hub (see start_recording for rationale).
    pub session_hub: Arc<DriverHub>,
}
```

---

## Frontend file structure (changes from 3a)

```
crates/app/ui/src/
├── App.svelte                     ← view router (list | editor)
├── lib/
│   ├── api.ts                     ← add create_macro, update_macro_full, 
│   │                                load_macro_steps, start_recording, 
│   │                                stop_recording wrappers
│   ├── types.ts                   ← add StepDto union, PointDto, MoveModeDto
│   ├── stores/
│   │   ├── macros.ts              ← add createMacro/updateFull/loadSteps actions
│   │   ├── recording.ts           ← NEW: active recording state, event listeners
│   │   └── playback.ts            ← unchanged
│   └── components/
│       ├── MacroTable.svelte      ← enable "+ Record" button → opens recording modal
│       ├── MacroRow.svelte        ← ✎ button → setView("editor", id)
│       ├── HotkeyPicker.svelte    ← add Capture button + listening state
│       ├── RecordingModal.svelte  ← NEW: pre-recording confirm + post-recording preview
│       ├── StepEditor.svelte      ← NEW: full-screen editor
│       ├── StepRow.svelte         ← NEW: one row per step with inline inputs
│       ├── PlaybackBanner.svelte  ← unchanged
│       ├── Toast.svelte / ToastHost.svelte   ← unchanged
│       └── EditMetadataModal.svelte  ← DELETED (subsumed by StepEditor)
```

---

## Out of scope (Plan 3c+)

- Global hotkey listener (`rm-hotkey` integration) — triggering macros while focus is in another app
- Driver status indicator + install button
- Settings page (configurable stop key, default storage root, theme)
- System tray icon
- Window state persistence (size, position memory across launches)
- Toast persistence across reloads
- Multi-macro concurrent playback
- Drag-and-drop step reordering (the ↑↓ buttons satisfy MVP)
- Live hotkey capture via Interception (for keys browser can't see) — fallback to dropdown is acceptable for 3b
- Configurable stop key — F10 is hardcoded; configurable in Settings (3c)
- Hotkey conflict detection (binding the same combo to two macros) — no checks in 3a or 3b; deferred to 3c+ when global hotkey integration makes it observable
- Mouse coordinate (`MouseClick.at`) editor — recordings always capture `None`; manual editing of click coordinates is deferred

---

## Acceptance criteria

1. **Recording:**
   - Clicking "+ Record" in the macro list opens the start modal.
   - Starting recording minimizes the window and opens the Interception driver.
   - User can record any keyboard/mouse activity in another app.
   - Pressing F10 ends recording; the F10 keypress does not appear in the step list.
   - Recording's preview modal lets the user name, hotkey-bind, mode-set, and save the macro.
   - Discard throws the recording away without saving.

2. **Step editor:**
   - Clicking ✎ on a list row opens the full-screen editor pre-loaded with that macro's metadata and steps.
   - User can edit any parameter inline, remove any step, reorder via ↑↓, and add new steps via the bottom menu.
   - Save persists via `update_macro_full`. List refreshes; new step count visible.
   - Discard returns to list. Unsaved changes prompt a confirmation.

3. **Hotkey capture:**
   - HotkeyPicker has a Capture button that puts it in listening mode.
   - Pressing a hotkey combo + releasing commits the combo to the trigger.
   - Esc cancels without committing.
   - Dropdown remains available as fallback.

4. **Tests:**
   - `cargo test --workspace` is green.
   - New `rm-app` commands have unit tests for happy path + RecordingActive/MacroNotFound failure paths.
   - Frontend builds clean (`npm run build`).

5. **Manual smoke (added to `crates/app/README.md`):**
   - Record a macro from scratch via the GUI; play it back.
   - After F10 stop, the rust-macro window restores AND re-takes focus; Preview modal is interactive immediately (no need to click the window first).
   - Edit a macro's steps (delete one, change a wait timing, add a new step); save; play it back; new behavior is reflected.
   - Bind a hotkey by clicking Capture and pressing Ctrl+Shift+F5; verify the combo is set.
   - Try to start recording while a macro is playing → see `PlaybackActive` toast.
   - Try to start a playback while a recording is in progress → see `RecordingActive` toast.
   - Close the rust-macro window mid-recording → Interception releases, app exits cleanly, captured steps are discarded.

---

## Open implementation notes

- **`rm-recorder` extension:** the existing crate exposes `start_recording(hub, passthrough: bool)` returning a handle. The handle has `wait_for_close()` and `finish()` methods. For F10-aware stopping, the recommended approach is to **extend `rm-recorder` with a stop-key parameter** rather than building a parallel listener in `rm-app`. Specifically: add a `start_recording_with_stop_key(hub, passthrough, stop_key: KeyCode)` (or change the existing signature with `Option<KeyCode>`). The recorder, which already consumes the event stream, filters out the stop key and ends its own loop when it sees the keydown.

  **Rationale (corrected):** `DriverHub` supports multiple subscribers cleanly (`broadcast::Receiver` — see `crates/driver/src/hub.rs::two_subscribers_each_receive_every_event`). The reason to put F10 filtering inside the recorder is **passthrough atomicity**: the recorder is the only subscriber that calls `hub.send()` for passthrough re-emission. F10 must be dropped from the buffer AND from the OS-bound re-emit in the same iteration. A parallel listener in `rm-app` would observe F10 too late — the recorder might have already passed it through to the OS.

  **Implementation order for the stop-key filter (REQUIRED):**
  1. `rx.recv()` returns an event.
  2. **Before** appending to buffer AND **before** passthrough `hub.send()`: if `event == RawEvent::KeyDown { key: stop_key }`, fire internal stop signal and `continue` (skip this iteration entirely).
  3. Otherwise proceed normally: passthrough, then append to buffer.

  Without this explicit order, a careless implementation will emit a trailing `KeyPress { F10 }` in the compiled steps.

  The implementation plan will start by reading `rm-recorder/src/lib.rs` to choose the exact API shape and write the test for stop-key behavior.

- **Window minimize/restore on Windows:** Tauri 2 exposes `Window::minimize()` and `Window::unminimize()`. After F10 stop, calling `unminimize()` may not bring focus back; we may also need `set_focus()`. Test during implementation.

- **Browser `KeyboardEvent.code` quirks:** Some users have non-US keyboard layouts where `Digit1` maps differently. Plan 3b uses `code` (physical key) rather than `key` (logical) so that the captured hotkey matches what the Interception driver later sees at the scancode level. This is consistent with how the existing dropdown lists work.

- **Editor performance:** very long macros (1000+ steps) may render slowly without virtualization. Plan 3b does not virtualize the step list — macros that long are out of typical scope. If we see real issues, add virtualization in 3c.

- **`MacroDto.step_count` vs `load_macro_steps`:** keeping the list view cheap is important. `load_macros` continues to return `step_count` without the actual steps. The editor's first action is `load_macro_steps(id)` which returns the full step array. The Save path doesn't need to read steps — only writes them.

- **Validation on Save:** `Macro::validate()` already covers `Wait { min > max }` and empty names. Both should surface as toasts on Save failure.

- **DTO bloat:** introducing `StepDto`, `PointDto`, `MoveModeDto` adds boilerplate. Worth it for the same reason `TriggerDto` was introduced in 3a — wire format independent from the on-disk format.

- **`update_macro_metadata` retained alongside `update_macro_full`:** the existing 3a metadata-only command stays — used today by the editor's existing flow and possibly future surfaces that need to change just hotkey/name without re-sending a large step array. The editor's Save uses `update_macro_full` because it touches steps. Both commands write through `rm-storage::save_macro` and have identical persistence semantics.
