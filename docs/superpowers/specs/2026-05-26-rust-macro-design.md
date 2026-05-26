# rust-macro — Design Document

**Date:** 2026-05-26
**Status:** Approved (brainstorming phase)
**Target version:** v1 (MVP+)

## Summary

`rust-macro` is a Windows desktop application for creating, editing, and executing keyboard and mouse macros, inspired by Keyran. It is GUI-first (no scripting required from end users) and built in Rust with a Tauri (HTML/CSS/JS) frontend. Input is captured and emitted via the Interception kernel driver, providing reliable I/O in games that block or restrict standard Win32 hooks (`SetWindowsHookEx`, `SendInput`).

The target use case is single-player / offline games and productivity automation. The project explicitly does **not** target detection evasion in anti-cheat-protected online games.

## Goals (v1)

- Record a sequence of keyboard and mouse events from real hardware input.
- Present the recording in a step-by-step editor where the user can add, remove, reorder, and tweak steps (including converting fixed delays into randomized delays).
- Save macros locally with names and metadata.
- Bind a hotkey to each macro; trigger playback by hotkey or by clicking in the UI.
- Play back macros with configurable mode: once, N repetitions, infinite loop, or toggle.
- Detect, prompt installation of, and recover from missing Interception driver state.
- Single distributable `.exe` (Tauri bundle); user-mode runtime, elevation only required for driver install.

## Non-goals (v1, deferred to later versions)

- Pixel/color-based conditionals.
- Multiple input profiles or profile switching by foreground window.
- Per-device disambiguation in the UI (more than one keyboard / mouse). The driver layer supports it; the v1 UI does not expose it.
- Scripting / DSL for advanced users.
- Pre-made macro library or import/export marketplace.
- Network features (cloud sync, sharing).
- macOS or Linux support. Windows only.
- Headless / service mode (running macros without GUI open). Refactor target for v2 if needed.

## Architecture

### Process model

Single Tauri process. The Rust backend hosts all logic; the WebView hosts the UI. Heavy or long-running work (driver I/O, recording loop, playback loop, hotkey listener) runs in dedicated Tokio tasks so the UI thread is never blocked.

```
┌────────────────────────────────────────────────────────────┐
│ Tauri WebView (Frontend — Svelte + TypeScript + Vite)      │
│  - Macro list                                              │
│  - Step-by-step macro editor                               │
│  - Recording overlay                                       │
│  - Hotkey configuration                                    │
│  - Settings / driver status                                │
└──────────────────────┬─────────────────────────────────────┘
                       │  Tauri invoke / emit (async, JSON)
┌──────────────────────▼─────────────────────────────────────┐
│ Rust backend (Tauri main process)                          │
│                                                            │
│  commands::                                                │
│    start_recording / stop_recording                        │
│    play_macro / stop_macro / stop_all                      │
│    save_macro / load_macros / delete_macro                 │
│    set_hotkey / get_driver_status / install_driver         │
│                                                            │
│  ┌──────────┐ ┌─────────┐ ┌──────────────────┐            │
│  │ recorder │ │ player  │ │ hotkey listener  │  Tokio     │
│  │  task    │ │ task(s) │ │     task         │  tasks     │
│  └────┬─────┘ └────┬────┘ └─────────┬────────┘            │
│       └────────────┼─────────────────┘                     │
│                    ▼                                       │
│         ┌──────────────────────┐                          │
│         │ driver (Interception)│                          │
│         │ wrapper over         │                          │
│         │ interception-rs      │                          │
│         └──────────┬───────────┘                          │
└────────────────────┼──────────────────────────────────────┘
                     ▼
              kernel: keyboard.sys / mouse.sys (Interception)
                     ▼
                 Hardware

Persistence:
  %APPDATA%/rust-macro/macros/<uuid>.json
  %APPDATA%/rust-macro/settings.json
  %APPDATA%/rust-macro/logs/*.log
```

### Workspace layout

Cargo workspace with one binary crate (`app`) and library crates for the rest. The boundaries are real: `recorder` cannot import `app`; `player` cannot import `recorder`. This enforces unidirectional dependencies and keeps each crate independently testable.

| Crate         | Responsibility                                                       | Depends on                |
|---------------|----------------------------------------------------------------------|---------------------------|
| `app`         | Tauri entry point; registers command handlers; wires runtime state.  | all others                |
| `driver`      | Thin wrapper over `interception-rs`: open context, send, recv, filter. | `interception-rs`, `error` |
| `macro_model` | `Macro`, `Step`, `Trigger`, `PlaybackMode` + serde.                 | `serde`, `uuid`, `chrono` |
| `recorder`    | Tokio task: reads raw events from driver, compiles to `Vec<Step>`.  | `driver`, `macro_model`, `error` |
| `player`      | Tokio task: executes a `Macro` by emitting events through `driver`. | `driver`, `macro_model`, `error` |
| `hotkey`      | Global hotkey listener; dispatches into `player` per binding.       | `driver`, `storage`, `error` |
| `storage`     | CRUD for macros and settings on disk (JSON).                        | `macro_model`, `serde_json`, `error` |
| `error`       | Central `AppError` enum and conversions.                            | `thiserror`               |

### Frontend stack

- **Svelte + TypeScript + Vite** inside the Tauri WebView.
- Rationale: small bundle, simple reactivity for list/form-heavy UI, no JSX overhead. Trivially swappable for React if preferred later.
- State persisted to the Rust backend through Tauri commands; no client-side database.

## Data Model

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct Macro {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub trigger: Trigger,
    pub playback: PlaybackMode,
    pub steps: Vec<Step>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Step {
    KeyPress    { key: KeyCode, hold_ms: u32 },
    KeyDown     { key: KeyCode },
    KeyUp       { key: KeyCode },
    MouseClick  { button: MouseButton, hold_ms: u32, at: Option<Point> },
    MouseMove   { to: Point, mode: MoveMode },
    MouseScroll { delta: i32 },
    Wait        { min_ms: u32, max_ms: u32 },
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Trigger {
    Hotkey { key: KeyCode, modifiers: Vec<Modifier> },
}

#[derive(Serialize, Deserialize, Clone)]
pub enum PlaybackMode {
    Once,
    Repeat(u32),
    Loop,
    Toggle,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct Point { pub x: i32, pub y: i32 }

#[derive(Serialize, Deserialize, Clone, Copy)]
pub enum MoveMode { Absolute, Relative }
```

`Step` is intentionally **high-level**, not raw events. The recorder compiles raw `KeyDown(W) → 250ms → KeyUp(W)` into `KeyPress { key: W, hold_ms: 250 }`. This matches the editor UI one-to-one and makes `Wait { min, max }` natural to express.

Wait randomization: at playback, `Wait { min_ms, max_ms }` resolves to a uniform random millisecond value in `[min_ms, max_ms]`. When `min == max`, behavior is a fixed delay (this is what fresh recordings produce).

`KeyCode` and `MouseButton` are enums over physical input codes (scancodes for keyboard), not virtual keys. This is what Interception works with natively and avoids layout ambiguity.

## Core Flows

### Recording

1. UI: user clicks **Record**. Hotkey to stop recording is configurable (default `F12`).
2. `start_recording` command spawns a `recorder` Tokio task.
3. `recorder` opens an Interception context filtered for keyboard + mouse devices and enters its read loop.
4. For each event received from the driver:
   - Timestamp it with `Instant::now()`.
   - **Pass it through to the OS** by re-emitting via `driver.send(event)`. The user must see the effect of their input in the target app/game during recording.
   - Push `(event, timestamp)` to an internal `Vec<RawEvent>`.
5. The stop hotkey, when pressed, triggers the `stop_recording` command. The task is signaled to terminate and returns its `Vec<RawEvent>`.
6. The backend compiles raw events into `Vec<Step>`:
   - `KeyDown(k) → KeyUp(k)` pairs collapse into `KeyPress { key: k, hold_ms: delta }`.
   - Gaps between events become `Wait { min_ms: d, max_ms: d }` (fixed; user can later edit either end in the UI to randomize).
   - Consecutive mouse movements within a short window may consolidate into a single `MouseMove` to the final point (configurable; default on).
7. The backend emits an event `recording_finished` to the frontend, carrying the provisional `Macro` (no `id` persisted yet). The UI opens the editor pre-filled with the result.

### Playback

1. Trigger arrives via hotkey listener or UI button → `play_macro(id)`.
2. Backend loads the `Macro` from storage and spawns a `player` Tokio task.
3. The task iterates according to `PlaybackMode`:

   ```rust
   for _ in playback_iter(macro.playback) {
       for step in &macro.steps {
           match step {
               Step::Wait { min_ms, max_ms } => {
                   let d = rand::thread_rng().gen_range(*min_ms..=*max_ms);
                   tokio::time::sleep(Duration::from_millis(d.into())).await;
               }
               Step::KeyPress { key, hold_ms } => {
                   driver.send(KeyDown(*key))?;
                   tokio::time::sleep(Duration::from_millis((*hold_ms).into())).await;
                   driver.send(KeyUp(*key))?;
               }
               // ... other step variants
           }
           if stop_signal.try_recv().is_ok() { return Ok(()); }
       }
   }
   ```

4. `playback_iter`:
   - `Once` → 1 iteration
   - `Repeat(n)` → n iterations
   - `Loop` → unbounded; exits on `stop_signal`
   - `Toggle` → unbounded; same hotkey sends the `stop_signal`

5. Concurrency: backend keeps `HashMap<MacroId, (JoinHandle, StopSender)>` of active executions. Re-trigger of an already-running macro: default behavior is **ignore** (configurable per macro to `restart` instead). A global stop hotkey (default `ESC`) cancels all active macros.

### Driver lifecycle

1. On app startup, backend calls `driver::detect_status()`:
   - Returns one of `{ NotInstalled, InstalledNotRunning, Running }`.
   - Implementation: query the Windows service manager for `keyboard` and `mouse` Interception services, plus attempt to open an Interception context.
2. If `NotInstalled` or `InstalledNotRunning`, frontend shows a modal with status and an **Install / Repair driver** button.
3. Clicking install calls `install_driver` command, which launches the bundled `install-interception.exe` with UAC elevation (`runas`). The Interception installer requires admin rights and a reboot.
4. After install + reboot, the next launch detects `Running` and proceeds normally.
5. Driver status is displayed in **Settings**, with a manual **Recheck** button.

## Persistence

- Root directory: `%APPDATA%/rust-macro/` (resolved via `dirs::config_dir()` for portability of the resolution code, but the path is platform-specific).
- Layout:

  ```
  %APPDATA%/rust-macro/
    settings.json             — global settings (stop hotkey, defaults, theme)
    macros/
      <uuid>.json             — one file per macro
    logs/
      rust-macro.log          — current log
      rust-macro.YYYY-MM-DD.log — rotated logs (last 7 days)
  ```

- File format: pretty-printed JSON.
- Loading: at startup, `storage::load_all_macros()` reads `macros/*.json` and returns `Vec<Macro>`. Malformed files are logged with their path and skipped, never aborted on.
- Writing: atomic write-then-rename (`<uuid>.json.tmp` → `<uuid>.json`). Prevents corruption if the app is killed mid-save.

## Error Handling

Central error type:

```rust
#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("Interception driver is not installed")]
    DriverNotInstalled,

    #[error("Interception driver is installed but not running")]
    DriverNotRunning,

    #[error("Driver I/O failed: {0}")]
    DriverIo(String),

    #[error("Macro not found: {0}")]
    MacroNotFound(Uuid),

    #[error("Cannot start: a recording is already in progress")]
    RecordingActive,

    #[error("Storage error: {0}")]
    Storage(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
```

Tauri commands return `Result<T, AppError>`. `AppError` implements `Serialize` via a `serde` `impl` that emits `{ kind: "DriverNotInstalled", message: "..." }` so the frontend can switch on `kind` to show appropriate UI (modal for driver issues, toast for transient failures, banner for storage issues).

Logging uses `tracing` with file output to `%APPDATA%/rust-macro/logs/`. Log level is configurable in settings (default `INFO`; `DEBUG` toggleable from a hidden settings panel).

## Testing Strategy

- **Unit tests** in each crate:
  - `macro_model`: serde roundtrip for every variant of `Step`, `Trigger`, `PlaybackMode`.
  - `recorder`: event-compilation logic (raw events → steps) fed with hand-crafted `Vec<RawEvent>` fixtures.
  - `player`: step execution with a **mock driver** (records sent events instead of calling Interception). Verifies timing tolerance, repeat counts, stop-signal honoring.
  - `storage`: roundtrip macros through a temp directory; corrupted-file recovery.
- **Integration tests** in `app/tests/`:
  - Full recorder → storage → player loop using the mock driver. Verifies a recorded macro plays back to the same sequence of events.
- **Manual test plan** for driver-dependent behavior, since Interception driver is hard to test in CI:
  - Fresh install on clean Windows VM: detect missing driver, install flow, post-reboot detection.
  - Record/play in a target app (e.g., Notepad for text, an offline game for confirmation).
  - Hotkey trigger from background while another app is focused.
  - Stop-all global hotkey from a runaway loop.
- CI: GitHub Actions on `windows-latest`, runs `cargo test` (mock driver) and `cargo clippy -D warnings`. Driver-dependent integration tests are gated behind a feature flag and not run in CI.

## Distribution

- Single `.msi` installer produced by `tauri build` (Tauri bundler → WiX).
- Bundles `install-interception.exe` as a side resource (not auto-installed; user opts in from the app).
- App runs without elevation; only the driver install button triggers UAC.

## Open Questions / Future Work

- **v2 candidates** (deferred): pixel/color conditionals, profiles per foreground window, per-device disambiguation in UI, macro library / import-export, headless service mode for running macros without GUI.
- **Performance budget:** playback step-to-step latency budget is < 1ms on top of the user-specified `Wait` / `hold_ms`. To validate, the test suite should record wall-clock deltas for a fixture macro and assert the overhead.
- **Hotkey conflicts:** v1 detects when two macros bind the same hotkey at save time and refuses; later versions may allow priority/ordering.
- **Crash safety during recording:** if the app crashes mid-recording, raw events are lost. Acceptable for v1. v2 may stream raw events to a temp file.
