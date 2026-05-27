# rust-macro — Plan 3a: Tauri GUI (macro manager + Play) — Design Spec

**Date:** 2026-05-26
**Status:** Draft, awaiting user review.
**Predecessors:** Plan 1 (backend crates), Plan 2a (DriverHub), Plan 2b (Interception driver).
**Successor:** Plan 3b (recording from GUI, hotkey live capture, driver status UI, settings).

## Summary

Plan 3a delivers the first usable GUI for `rust-macro`. It is a Tauri 2.x desktop app whose only jobs are: list saved macros, edit their metadata (name, hotkey assignment via dropdown, playback mode), delete them, and play/stop them via the existing `rm-player` + `InterceptionDriver`. It does **not** include recording from the GUI, the step-by-step editor, the global hotkey listener, the driver install flow, or a settings page — all of those are deferred to Plan 3b.

This is a deliberately narrow slice. It proves the Tauri ↔ Rust backend wiring against the existing crates, ships the bare-minimum UI that adds value over the CLI ("I can see my macros and click Play"), and leaves the harder UX (live event capture, driver lifecycle) for a focused follow-up.

## Goals

- Single Tauri window listing all macros in `<data_dir>/rust-macro/macros/*.json`.
- Per-macro actions: Play, Edit metadata, Delete.
- Edit-metadata modal: rename, choose hotkey (modifier checkboxes + key dropdown — **not** live capture), choose playback mode.
- Global "Stop" button while a playback is active.
- Backend retains Plan 2b's all-mock-by-default discipline: the GUI compiles and runs without Interception; opening the driver is **lazy**, only on the first `play_macro` call.
- New crate `rm-app` with Tauri commands and DTOs; existing crates (`rm-storage`, `rm-player`, `rm-driver`, `rm-driver-interception`, `rm-macro-model`, `rm-error`) are reused unchanged.

## Non-goals (deferred to Plan 3b)

- Recording macros from the GUI ("+ Record new" button is shown disabled).
- Step-by-step editor (adding/removing/reordering/editing individual steps; converting Wait fixed→randomized).
- Live hotkey capture ("press a key combo to bind").
- `rm-hotkey` integration (global hotkey listener for triggering macros while focus is elsewhere).
- Driver status indicator + install button + repair flow.
- Settings page (theme, log level, default playback mode, etc).
- Multiple concurrent playbacks (Plan 3a enforces one-at-a-time; matches CLI behavior).
- Window state persistence (size/position memory, tray icon, minimize-to-tray).
- Tauri WebDriver E2E tests (manual smoke test only).
- CI pipeline for the frontend (Vitest runs locally only; `cargo test --workspace` remains the gate).

## Architecture

### Process model

Single Tauri process. Rust backend hosts all state and logic; the WebView hosts the UI. Tokio runtime is configured by Tauri's macro. Heavy work (driver I/O, player task) runs in Tokio tasks separately from the UI thread.

```
┌──────────────────────────────────────────────────────────────┐
│ Tauri WebView — Svelte 5 + TypeScript + Vite                 │
│                                                              │
│   App.svelte                                                 │
│     └── MacroTable                                           │
│           └── MacroRow ── Play/Stop, Edit, Delete            │
│     └── EditMetadataModal (HotkeyPicker inside)              │
│     └── PlaybackBanner (visible while playing)               │
└────────────────────┬─────────────────────────────────────────┘
                     │  Tauri invoke (cmd) / listen (event)
┌────────────────────▼─────────────────────────────────────────┐
│ Rust backend — crates/app                                    │
│                                                              │
│   main.rs        — Tauri entry, command registration         │
│   state.rs       — AppState (DriverHub lazy, ActivePlayback) │
│   commands.rs    — Tauri command handlers                    │
│   dto.rs         — wire DTOs (Serialize for Tauri JSON)      │
│                                                              │
│  Existing reused crates:                                     │
│    rm-storage   — load/save/delete on disk                   │
│    rm-player    — runs a Macro through a DriverHub           │
│    rm-driver    — DriverHub, Driver trait                    │
│    rm-driver-interception — real driver impl                 │
│    rm-macro-model — domain types                             │
│    rm-error     — AppError + to_wire()                       │
└──────────────────────────────────────────────────────────────┘
```

### Crate layout

```
crates/app/
  Cargo.toml
  build.rs                — tauri_build::build()
  tauri.conf.json         — Tauri 2.x config
  icons/                  — placeholder icons (1 PNG per required size)
  src/
    main.rs               — fn main, tauri::Builder
    state.rs              — AppState
    commands.rs           — #[tauri::command] handlers
    dto.rs                — MacroDto, TriggerDto, PlaybackModeDto, WireError
  ui/                     — Vite root (frontend)
    package.json
    package-lock.json
    vite.config.ts
    svelte.config.js
    tsconfig.json
    index.html
    public/
    src/
      app.css             — single dark-theme stylesheet
      main.ts             — Svelte mount
      App.svelte
      lib/
        api.ts            — typed wrappers around @tauri-apps/api invoke/listen
        types.ts          — TS types mirroring dto.rs
        stores/
          macros.ts       — Svelte store + load/refresh helpers
          playback.ts     — current ActivePlayback state
        components/
          MacroTable.svelte
          MacroRow.svelte
          EditMetadataModal.svelte
          HotkeyPicker.svelte
          PlaybackBanner.svelte
          Toast.svelte
          ToastHost.svelte
```

### Frontend stack

- **Tauri 2.x** (latest stable at time of implementation). Tauri 2 is GA and has Svelte templates first-class.
- **Svelte 5** + **TypeScript** + **Vite** — confirmed from the top-level design doc.
- **Package manager:** npm (Tauri scaffolding default). No pnpm/yarn unless the user already prefers one.
- **No UI library.** Single hand-rolled `app.css` with CSS variables for a dark theme. Components use scoped Svelte styles for layout. Rationale: Svelte 5 UI lib ecosystem is still maturing (shadcn-svelte, Skeleton, etc. have partial Svelte 5 support); rolling our own keeps the bundle tiny and the design coherent. Refactor target later if needed.
- **Vitest** for component/unit tests (local-only; not in CI). `@testing-library/svelte` for component tests.

## Data flow

### Initial load
1. App mounts, `App.svelte` triggers `macros.load()` in `stores/macros.ts`.
2. `macros.load()` calls `api.loadMacros()` which `invoke('load_macros')`.
3. Backend returns `Vec<MacroDto>`. Store replaces its array.
4. `MacroTable` reactively renders rows.

### Play
1. User clicks ▶ on a row. `MacroRow` calls `api.playMacro(id)`.
2. Backend's `play_macro` command:
   - Acquires `active` mutex; if `Some`, returns `AppError::PlaybackActive` (NEW variant — see below).
   - Acquires `driver_hub` mutex; if `None`, calls `open_interception()` (reused helper from Plan 2b — copied/moved into `rm-app`). On failure, returns the same mapped error (`DriverNotInstalled` / `DriverNotRunning` / `DriverIo`). The `driver_hub` slot stays `None` so the next attempt re-tries (e.g. user fixed the driver and clicked again).
   - On success, calls `rm_storage::load_all`, finds the macro by id (returns `MacroNotFound` if absent), spawns `rm_player::play(hub.clone(), m).wait()` inside a Tokio task wrapped in a stop-channel.
   - Stores the `ActivePlayback { macro_id, stop_tx, join }` in `active`.
   - Emits `playback_started { macro_id, macro_name }` to the frontend.
   - Returns `Ok(())`.
3. A supervisor task awaits `join`, emits `playback_finished { macro_id, result }`, and clears `active`.

### Stop
1. User clicks ■ Stop in the `PlaybackBanner`. Frontend calls `api.stopPlayback()`.
2. Backend's `stop_playback`:
   - Takes `active`; if `None`, returns `Ok(())` (no-op — idempotent).
   - Sends `()` on `stop_tx` (drops the sender — player's `select!` on stop branch fires).
   - Awaits `join` with a 2s timeout. If it doesn't finish, logs and aborts the task.
   - Clears `active`.
   - The supervisor task emits `playback_finished` (with `result: ok` if normal, `result: { kind: "Other", message: "stopped" }` if forced). Both are handled the same way on the frontend (banner disappears).

### Edit metadata
1. User clicks ✎ → `EditMetadataModal` opens with current values.
2. User edits; clicks Save. `api.updateMacroMetadata(id, name, trigger, playback)` is called.
3. Backend `update_macro_metadata`:
   - Loads the macro by id (storage returns the full Macro including steps).
   - Mutates name, trigger, playback. Sets `updated_at = Utc::now()`.
   - Saves via `rm_storage::save_macro`.
   - Returns the new `MacroDto`.
4. Store replaces the entry; modal closes.

### Delete
1. User clicks ✕ → confirm dialog.
2. `api.deleteMacro(id)` → backend `delete_macro` → `rm_storage::delete_macro` → returns `Ok(())`.
3. Store filters out the entry.

## DTOs and wire format

```rust
// crates/app/src/dto.rs

#[derive(Serialize, Deserialize, Clone)]
pub struct MacroDto {
    pub id: Uuid,
    pub name: String,
    pub trigger: TriggerDto,
    pub playback: PlaybackModeDto,
    pub step_count: usize,           // not the full steps
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerDto {
    Hotkey { key: KeyCode, modifiers: Vec<Modifier> },
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(tag = "type", rename_all = "snake_case", content = "value")]
pub enum PlaybackModeDto {
    Once,
    Repeat(u32),
    Loop,
    Toggle,
}

impl From<&Macro> for MacroDto {
    fn from(m: &Macro) -> Self { /* mechanical copy */ }
}
```

`TriggerDto` and `PlaybackModeDto` mirror the model exactly for v1 — but they're separate types so we can evolve the wire format independently if needed (e.g. add UI-only fields like `kind: "hotkey" | "manual"`).

`KeyCode` and `Modifier` already `#[derive(Serialize, Deserialize)]` in `rm-macro-model`. The TS side declares matching string-literal unions:

```ts
// ui/src/lib/types.ts
export type KeyCode = "A" | "B" | ... | "F12" | ...;
export type Modifier = "Ctrl" | "Shift" | "Alt" | "Win";
export type Trigger = { type: "hotkey"; key: KeyCode; modifiers: Modifier[] };
export type PlaybackMode =
  | { type: "once" }
  | { type: "repeat"; value: number }
  | { type: "loop" }
  | { type: "toggle" };
```

## Error handling

`AppError` gains one new variant:

```rust
#[error("A playback is already in progress")]
PlaybackActive,
```

The `kind()` match arm gains `PlaybackActive => "PlaybackActive"` and the corresponding unit test (`crates/error/src/lib.rs` already has a pattern for `driver_not_installed_kind_is_stable` — add a sibling for the new variant). `WireError` shape is unchanged; only the `kind` string set expands. Frontend dispatches on `kind`:

| kind                  | UI treatment                                                              |
|-----------------------|---------------------------------------------------------------------------|
| `DriverNotInstalled`  | Persistent toast: "Interception driver not installed." Link: 3b add install button. |
| `DriverNotRunning`    | Persistent toast: "Interception driver installed but not running."        |
| `DriverIo`            | Red toast with message.                                                   |
| `PlaybackActive`      | Short yellow toast: "Already playing — stop it first."                    |
| `MacroNotFound`       | Reload list (silent fallback).                                            |
| `RecordingActive`     | (Not raised in 3a; reserved for 3b.)                                      |
| `Io`                  | Red toast with path + message.                                            |
| `Serde`               | Red toast with message.                                                   |
| `Other`               | Red toast with message.                                                   |

All toasts go through a single `Toast.svelte` component fed by a `toast` Svelte store. `ToastHost.svelte` renders the queue in the top-right.

## State management

### Backend

```rust
// crates/app/src/state.rs

pub struct AppState {
    pub storage_root: PathBuf,
    pub driver_hub: tokio::sync::Mutex<Option<Arc<DriverHub>>>,
    pub active: tokio::sync::Mutex<Option<ActivePlayback>>,
}
// Commands that need to emit events receive `app: tauri::AppHandle` as a
// command parameter (Tauri 2's DI handles this). The supervisor task that
// emits `playback_finished` is spawned from within `play_macro` with a
// cloned AppHandle in scope.

pub struct ActivePlayback {
    pub macro_id: Uuid,
    pub macro_name: String,
    pub stop_tx: tokio::sync::oneshot::Sender<()>,
    pub join: tokio::task::JoinHandle<Result<(), AppError>>,
}
```

`storage_root` is computed at startup the same way the CLI does: `dirs::data_dir().map(|d| d.join("rust-macro"))`. No CLI-overridable `--root` flag in 3a (would conflict with Tauri's argument handling; defer to settings page in 3b if ever needed).

### Frontend

Two Svelte stores:

```ts
// ui/src/lib/stores/macros.ts
export const macros: Writable<MacroDto[]> = writable([]);
export async function loadMacros(): Promise<void>;
export async function refresh(): Promise<void>;
export async function deleteMacro(id: string): Promise<void>;
export async function updateMetadata(id, name, trigger, playback): Promise<void>;

// ui/src/lib/stores/playback.ts
export type Active = { macroId: string; macroName: string; startedAt: number };
export const active: Writable<Active | null> = writable(null);
// Hooks into Tauri events `playback_started` / `playback_finished` on app mount.
```

## Testing strategy

| Layer | Tests | How |
|-------|-------|-----|
| `dto.rs` unit | Serde roundtrip for each DTO variant; `From<&Macro>` correctness | Rust unit tests in the file |
| `commands.rs` unit | Each command happy path + error path | Inject a `MockDriver`-backed `DriverHub` and a tempdir storage; do not depend on Tauri runtime (commands take `State<AppState>` which we construct directly) |
| `state.rs` unit | ActivePlayback lifecycle (start → stop → cleared) | Same as above |
| Frontend `api.ts` | invoke shape correctness | Vitest mocking `@tauri-apps/api/core` |
| Frontend components | `MacroRow` click → callback; `HotkeyPicker` emits correct value; modal validates name non-empty | Vitest + `@testing-library/svelte` |
| Manual | Open app on dev machine, click through all flows | Spec acceptance step |

CI runs `cargo test --workspace` only (same as before — Vitest is local-only). The new tests in `rm-app` are skipped automatically if Tauri's build script can't find a webview at compile time on CI? No — they're pure Rust unit tests, no Tauri runtime needed. They run.

## Distribution

`tauri build` produces a Windows `.msi` (Tauri 2 default). Not actually shipped in Plan 3a — we ship `cargo tauri dev` works and `cargo tauri build` produces a binary. End-user-ready packaging (signing, installer customization) is Plan 3b or later.

## Implementation notes

- **Tauri 2 plugin needs:** `@tauri-apps/plugin-dialog` for the delete-confirm modal? Or hand-rolled in Svelte? Hand-rolled — fewer deps.
- **Icons:** Tauri 2 requires icons in several sizes. Use the Tauri-generated placeholders for 3a (a generic logo); custom branding in 3b.
- **Identifier:** `dev.xyrlan.rust-macro` (per repo author).
- **App name:** "rust-macro" (literal; renaming is a v1.1 polish task).
- **Window:** 1000×700, resizable, standard decorations.
- **Lazy driver init**: the driver is opened on the first `play_macro` call, not on app startup. This means the GUI opens cleanly even without Interception installed; the error only surfaces when the user attempts to play. That matches the user mental model ("the app works; that one feature needs the driver") and avoids a startup modal in 3a.

## Acceptance criteria

- `cargo test --workspace` is green (existing 76 tests + new tests from `rm-app`).
- `cargo build -p rm-app` succeeds on Windows.
- `cargo tauri dev` from `crates/app/` opens a window that:
  1. Lists every macro saved at `%APPDATA%/rust-macro/macros/*.json`.
  2. Empty state shows "No macros yet. Use the CLI to record one (3b adds in-app recording)."
  3. Each row shows name, hotkey display, mode, step count.
  4. ✎ opens the modal; edits persist; list refreshes.
  5. ✕ confirms then deletes; list refreshes.
  6. ▶ kicks off playback (errors gracefully if Interception not installed); banner appears.
  7. ■ stops the active playback; banner disappears.
- Tested manually with at least one macro saved via the CLI (`cargo run -p rm-cli -- record demo` against `StdioDriver`).

## Open items / risks

- **Tauri 2 + Svelte 5 template availability.** Tauri's `create-tauri-app` may default to Svelte 4 at the time of implementation. If so, the implementer manually bumps `svelte` to `^5` in `package.json` and adjusts `svelte.config.js`. Track upstream; if Svelte 5 is unsupported by the official Tauri template, fall back to Svelte 4 and revisit at 3b.
- **Webview dependency.** Tauri on Windows uses the WebView2 runtime, which is pre-installed on Windows 11 (the user's platform). No extra install step needed.
- **Single-window restriction.** Concurrent edit-metadata modals on the same macro by two app instances would race; we don't open the same root dir twice in 3a so this is moot.
- **MSVC linker compile time.** Plan 2b confirmed the toolchain is set up; adding Tauri may pull in more native deps (windows-sys etc) but shouldn't surface anything new.
