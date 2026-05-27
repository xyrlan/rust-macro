# rust-macro — Plan 3c: global hotkey listener + mouse triggers + settings + step compaction — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make hotkeys actually trigger macros from anywhere (not just the ▶ button), allow mouse buttons (X1/X2/middle/right/left + modifiers) as triggers, add a Settings page (configurable stop-key + storage root), and further compact recorded step lists (filter sub-threshold Waits).

**Architecture:** One persistent Interception context for the listener (filtered, passthrough forwarder, hotkey dispatcher). Recording reuses the persistent hub by toggling the listener's passthrough off (recorder owns passthrough during a session). Playback continues to use its own send-only hub (no listener conflict; Plan 3b architecture preserved). Domain model gains a `Trigger::MouseButton { button, modifiers }` variant alongside `Trigger::Hotkey`. Settings live in `{storage_root}/settings.json` and load on app boot.

**Tech Stack:** Tauri 2 (Rust stable MSVC), Svelte 5 (runes), TypeScript, Vite 5. Target Windows 10/11 x64. Builds on Plans 3a + 3b.

**Spec:** This plan is self-contained — design notes inline. Cross-references: 3a (`docs/superpowers/specs/2026-05-26-rust-macro-plan-3a-tauri-gui-design.md`) and 3b (`docs/superpowers/specs/2026-05-27-rust-macro-plan-3b-recording-editor-design.md`).

---

## Open Architectural Risks (read before starting)

1. **Interception self-event loopback.** The persistent listener's filtered context has a passthrough subscriber that calls `hub.send(event)` for every received event. When `play_macro` later sends a synthesized event through its send-only context (or through the persistent context — both options exist), it's unknown whether the kernel routes that injected stroke back into the listener's filter and creates an infinite re-emit loop or self-triggered hotkey. Task 13's smoke test checks this with a single Play → observe whether the listener double-triggers.

2. **Listener observes F10 during recording.** When recording is active, the listener must not dispatch macros (F10 should only stop the recording). Solution: the listener checks `state.recording` and `state.active` on every match and skips when either is set. This is in Task 11.

3. **Migration of saved macros.** Old `Trigger::Hotkey { key, modifiers }` JSON must still parse. The new `Trigger::MouseButton { button, modifiers }` variant is an addition, not a replacement — serde's external tag (`type`) makes this backward-compatible. Task 1's test asserts that 3a/3b-vintage JSON loads correctly.

4. **Wait filter is lossy.** Dropping Waits <20ms changes replay timing. Test users may notice. Mitigation: keep the threshold conservative (20ms) and document. Task 5.

---

## File Structure

**Files to create (backend):**
- `crates/app/src/settings.rs` — Settings struct, load/save, defaults
- `crates/app/src/listener.rs` — Persistent listener supervisor (subscribes for passthrough + hotkey dispatch)

**Files to create (frontend):**
- `crates/app/ui/src/lib/stores/settings.ts`
- `crates/app/ui/src/lib/components/SettingsView.svelte`

**Files to modify (backend):**
- `crates/macro_model/src/macro_def.rs` — add `Trigger::MouseButton`
- `crates/hotkey/src/lib.rs` — extend registry + listener for mouse triggers
- `crates/recorder/src/compile.rs` — Wait filter (drop <20ms)
- `crates/app/src/state.rs` — add `settings`, `listener` fields
- `crates/app/src/dto.rs` — add `MouseTriggerDto` variant + `SettingsDto`
- `crates/app/src/commands.rs` — `load_settings`/`save_settings` commands; refresh registry on CRUD; use settings.stop_key
- `crates/app/src/recording.rs` — read stop key from settings (instead of const)
- `crates/app/src/main.rs` — boot listener; register settings commands
- `crates/app/README.md` — smoke test updates

**Files to modify (frontend):**
- `crates/app/ui/src/lib/types.ts` — `MouseTriggerDto`, `SettingsDto`, helpers
- `crates/app/ui/src/lib/api.ts` — settings + new trigger wrappers
- `crates/app/ui/src/lib/components/HotkeyPicker.svelte` — mode toggle (key vs mouse)
- `crates/app/ui/src/lib/components/MacroTable.svelte` — gear icon → settings view
- `crates/app/ui/src/App.svelte` — view router gains `settings` tag

Tasks decomposed backend-first so the frontend can compile against stable Tauri commands.

---

## Task 1: Extend `Trigger` with `MouseButton` variant

**Files:**
- Modify: `crates/macro_model/src/macro_def.rs`

- [ ] **Step 1: Write the failing roundtrip + backward-compat tests**

Append to `mod tests` in `crates/macro_model/src/macro_def.rs`:

```rust
    #[test]
    fn trigger_mouse_button_serde_roundtrip() {
        let t = Trigger::MouseButton {
            button: MouseButton::X1,
            modifiers: vec![Modifier::Ctrl],
        };
        let j = serde_json::to_string(&t).unwrap();
        assert!(j.contains("\"type\":\"mouse_button\""));
        assert!(j.contains("\"button\":\"x1\""));
        let back: Trigger = serde_json::from_str(&j).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn legacy_hotkey_trigger_still_parses() {
        // Vintage 3a/3b on-disk format. Must continue to load after the
        // MouseButton variant is added.
        let j = r#"{"type":"hotkey","key":"f1","modifiers":["ctrl"]}"#;
        let back: Trigger = serde_json::from_str(j).unwrap();
        assert_eq!(
            back,
            Trigger::Hotkey {
                key: KeyCode::F1,
                modifiers: vec![Modifier::Ctrl],
            }
        );
    }
```

- [ ] **Step 2: Run — confirm FAIL**

Run: `cargo test -p rm-macro-model trigger_mouse_button_serde_roundtrip`
Expected: FAIL — `Trigger::MouseButton` does not exist.

- [ ] **Step 3: Add the variant**

In `crates/macro_model/src/macro_def.rs`, replace the `Trigger` enum with:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    Hotkey {
        key: KeyCode,
        modifiers: Vec<Modifier>,
    },
    MouseButton {
        button: MouseButton,
        modifiers: Vec<Modifier>,
    },
}
```

- [ ] **Step 4: Run — confirm PASS**

Run: `cargo test -p rm-macro-model`
Expected: PASS — all prior tests + 2 new.

- [ ] **Step 5: Commit**

```powershell
git add crates/macro_model/src/macro_def.rs
git commit -m "feat(macro-model): Trigger::MouseButton variant (backward-compatible)"
```

---

## Task 2: Extend `HotkeyRegistry` + listener to match mouse triggers

**Files:**
- Modify: `crates/hotkey/src/lib.rs`

Current `HotkeyRegistry::bind` destructures `Trigger::Hotkey` exclusively; after Task 1's domain change that's a compile error. Fix the match and add mouse-event handling to the listener loop.

- [ ] **Step 1: Add failing test for mouse dispatch**

Append to `mod tests`:

```rust
    #[tokio::test]
    async fn listener_dispatches_on_mouse_button_match() {
        use rm_macro_model::MouseButton;
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let reg = HotkeyRegistry::new();
        let id = Uuid::new_v4();
        reg.bind(
            id,
            Trigger::MouseButton {
                button: MouseButton::X1,
                modifiers: vec![Modifier::Ctrl],
            },
        )
        .await;

        let (tx, mut rx) = mpsc::unbounded_channel();
        let handle = start_listener(hub, reg.clone(), tx);

        drv.inject(RawEvent::KeyDown { key: KeyCode::LCtrl });
        drv.inject(RawEvent::MouseDown { button: MouseButton::X1 });

        let hit = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(hit, HotkeyHit(id));

        handle.shutdown().await;
    }
```

- [ ] **Step 2: Run — confirm FAIL**

Run: `cargo test -p rm-hotkey listener_dispatches_on_mouse_button_match`
Expected: FAIL — `Trigger::MouseButton` not handled.

Also expect compile errors in the existing `bind` (irrefutable-pattern issue). That's fine; Step 3 fixes both.

- [ ] **Step 3: Update `bind`, `match_pressed`, and the listener loop**

Replace the body of `HotkeyRegistry::bind` (currently destructures `Trigger::Hotkey` only):

```rust
    pub async fn bind(&self, id: Uuid, mut trigger: Trigger) {
        let mods = match &mut trigger {
            Trigger::Hotkey { modifiers, .. } => modifiers,
            Trigger::MouseButton { modifiers, .. } => modifiers,
        };
        mods.sort();
        mods.dedup();
        self.inner.lock().await.by_id.insert(id, trigger);
    }
```

Replace `match_pressed`:

```rust
    /// Match by a pressed keyboard key. Used by KeyDown handling.
    pub async fn match_key(&self, key: KeyCode, modifiers: &HashSet<Modifier>) -> Vec<Uuid> {
        let g = self.inner.lock().await;
        g.by_id
            .iter()
            .filter_map(|(id, t)| match t {
                Trigger::Hotkey { key: tk, modifiers: tm } => {
                    let tm_set: HashSet<_> = tm.iter().copied().collect();
                    if *tk == key && tm_set == *modifiers {
                        Some(*id)
                    } else {
                        None
                    }
                }
                Trigger::MouseButton { .. } => None,
            })
            .collect()
    }

    /// Match by a pressed mouse button. Used by MouseDown handling.
    pub async fn match_mouse(&self, button: rm_macro_model::MouseButton, modifiers: &HashSet<Modifier>) -> Vec<Uuid> {
        let g = self.inner.lock().await;
        g.by_id
            .iter()
            .filter_map(|(id, t)| match t {
                Trigger::MouseButton { button: tb, modifiers: tm } => {
                    let tm_set: HashSet<_> = tm.iter().copied().collect();
                    if *tb == button && tm_set == *modifiers {
                        Some(*id)
                    } else {
                        None
                    }
                }
                Trigger::Hotkey { .. } => None,
            })
            .collect()
    }
```

(Keep the old `match_pressed` only if other crates use it. CLI's `e2e.rs` references `match_pressed`; rename calls to `match_key` if so.)

In the listener loop (`start_listener`), update event handling:

```rust
                got = rx.recv() => match got {
                    Ok(RawEvent::KeyDown { key }) => {
                        if let Some(m) = key_as_modifier(key) {
                            mods.insert(m);
                        } else {
                            for id in registry.match_key(key, &mods).await {
                                let _ = out_tx.send(HotkeyHit(id));
                            }
                        }
                    }
                    Ok(RawEvent::KeyUp { key }) => {
                        if let Some(m) = key_as_modifier(key) {
                            mods.remove(&m);
                        }
                    }
                    Ok(RawEvent::MouseDown { button }) => {
                        for id in registry.match_mouse(button, &mods).await {
                            let _ = out_tx.send(HotkeyHit(id));
                        }
                    }
                    Ok(_) => { /* mouse up / move / wheel — not used for trigger matching */ }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!(lagged = n, "hotkey: dropped events");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("hotkey: hub closed");
                        break;
                    }
                }
```

- [ ] **Step 4: Fix CLI test compile errors**

`crates/cli/tests/e2e.rs:6` imports `start_listener, HotkeyHit, HotkeyRegistry` and may call `match_pressed`. Update any call sites to `match_key`. Run `cargo check --workspace`; if the CLI test doesn't compile, fix the rename. (If `match_pressed` is unused, you can keep both names — `match_pressed` as a deprecated alias for `match_key`. Simpler: just rename and update the one CLI call.)

- [ ] **Step 5: Run all hotkey tests + workspace check**

Run: `cargo test -p rm-hotkey`
Expected: PASS — prior tests + the new mouse dispatch test (4 → 5 typically; verify your local count).

Run: `cargo check --workspace`
Expected: PASS, no errors.

- [ ] **Step 6: Commit**

```powershell
git add crates/hotkey/src/lib.rs crates/cli/tests/e2e.rs
git commit -m "feat(hotkey): registry + listener dispatch for Trigger::MouseButton"
```

---

## Task 3: Wait filter in `compile_events`

**Files:**
- Modify: `crates/recorder/src/compile.rs`

Drop emitted `Wait` steps below a threshold (default 20ms). The user generally doesn't perceive sub-20ms gaps, and they bloat the step list during dense input.

- [ ] **Step 1: Add failing test**

Append to `mod tests`:

```rust
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
```

- [ ] **Step 2: Run — confirm FAIL**

Run: `cargo test -p rm-recorder waits_below_threshold_are_dropped`
Expected: FAIL — current code emits all Waits ≥1ms.

- [ ] **Step 3: Add threshold + filter**

In `crates/recorder/src/compile.rs`, near the top:

```rust
/// Minimum Wait duration that survives compilation. Sub-threshold gaps are
/// dropped — humans don't perceive them, and they bloat step lists during
/// dense input. If you need precise timing, edit the macro JSON directly.
pub const MIN_WAIT_MS: u32 = 20;
```

Update the gap-emit block (currently `if gap >= Duration::from_millis(1)`):

```rust
        let gap = cur.at.duration_since(last_at);
        let ms = gap.as_millis().min(u32::MAX as u128) as u32;
        if ms >= MIN_WAIT_MS {
            out.push(Step::Wait {
                min_ms: ms,
                max_ms: ms,
            });
        }
```

- [ ] **Step 4: Update `gap_between_keys_emits_wait` to use a gap ≥ threshold**

The existing test uses a 150ms gap, which is fine — no change needed. But re-run the full test set to confirm no regression.

Run: `cargo test -p rm-recorder`
Expected: PASS — all 16 prior tests + 2 new (18 total in the recorder crate).

- [ ] **Step 5: Commit**

```powershell
git add crates/recorder/src/compile.rs
git commit -m "feat(recorder): drop Wait steps below MIN_WAIT_MS (20ms) threshold"
```

---

## Task 4: Settings module — struct + load/save + defaults

**Files:**
- Create: `crates/app/src/settings.rs`
- Modify: `crates/app/src/main.rs` (to add `mod settings;`)

- [ ] **Step 1: Create `crates/app/src/settings.rs`**

```rust
//! Persistent app settings. Stored in `{storage_root}/settings.json`.
//! Loaded once at app startup; written on every `save_settings` Tauri call.

use std::path::{Path, PathBuf};

use rm_macro_model::KeyCode;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    /// Key that stops an in-app recording. Defaults to F10.
    #[serde(default = "default_stop_key")]
    pub stop_key: KeyCode,

    /// Override for the storage root. When `None`, the app uses
    /// `dirs::data_dir().join("rust-macro")`.
    #[serde(default)]
    pub storage_root_override: Option<PathBuf>,
}

fn default_stop_key() -> KeyCode {
    KeyCode::F10
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            stop_key: default_stop_key(),
            storage_root_override: None,
        }
    }
}

pub fn settings_path(storage_root: &Path) -> PathBuf {
    storage_root.join("settings.json")
}

/// Load settings from `{storage_root}/settings.json`. Returns `Settings::default()`
/// if the file doesn't exist. Any parse error is returned to the caller (don't
/// silently overwrite a corrupt user file).
pub fn load(storage_root: &Path) -> Result<Settings, std::io::Error> {
    let path = settings_path(storage_root);
    if !path.exists() {
        return Ok(Settings::default());
    }
    let bytes = std::fs::read(&path)?;
    let s = serde_json::from_slice(&bytes)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(s)
}

/// Atomically save settings via write-then-rename.
pub fn save(storage_root: &Path, s: &Settings) -> Result<(), std::io::Error> {
    std::fs::create_dir_all(storage_root)?;
    let path = settings_path(storage_root);
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(s)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_has_f10_stop_key() {
        let s = Settings::default();
        assert_eq!(s.stop_key, KeyCode::F10);
        assert!(s.storage_root_override.is_none());
    }

    #[test]
    fn load_missing_returns_default() {
        let tmp = TempDir::new().unwrap();
        let s = load(tmp.path()).unwrap();
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn save_then_load_roundtrips() {
        let tmp = TempDir::new().unwrap();
        let s = Settings {
            stop_key: KeyCode::Escape,
            storage_root_override: Some(PathBuf::from("/custom/path")),
        };
        save(tmp.path(), &s).unwrap();
        let back = load(tmp.path()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn load_corrupt_file_returns_err() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("settings.json"), b"{ bogus").unwrap();
        assert!(load(tmp.path()).is_err());
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/app/src/main.rs`, after `mod recording;`:

```rust
mod settings;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p rm-app settings::tests`
Expected: PASS — 4 tests.

- [ ] **Step 4: Commit**

```powershell
git add crates/app/src/settings.rs crates/app/src/main.rs
git commit -m "feat(app): Settings struct + load/save (stop_key, storage_root_override)"
```

---

## Task 5: `SettingsDto` + `load_settings`/`save_settings` Tauri commands

**Files:**
- Modify: `crates/app/src/dto.rs`
- Modify: `crates/app/src/commands.rs`
- Modify: `crates/app/src/main.rs`

- [ ] **Step 1: Add DTO in `crates/app/src/dto.rs`**

Append:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SettingsDto {
    pub stop_key: rm_macro_model::KeyCode,
    pub storage_root_override: Option<String>,
}

impl From<&crate::settings::Settings> for SettingsDto {
    fn from(s: &crate::settings::Settings) -> Self {
        Self {
            stop_key: s.stop_key,
            storage_root_override: s
                .storage_root_override
                .as_ref()
                .map(|p| p.display().to_string()),
        }
    }
}

impl From<SettingsDto> for crate::settings::Settings {
    fn from(s: SettingsDto) -> Self {
        crate::settings::Settings {
            stop_key: s.stop_key,
            storage_root_override: s.storage_root_override.map(std::path::PathBuf::from),
        }
    }
}
```

- [ ] **Step 2: Add commands in `crates/app/src/commands.rs`**

Append after `stop_recording`:

```rust
#[tauri::command]
pub async fn load_settings(state: State<'_, AppState>) -> Result<crate::dto::SettingsDto, WireError> {
    let s = state.settings.lock().await;
    Ok(crate::dto::SettingsDto::from(&*s))
}

#[tauri::command]
pub async fn save_settings(
    state: State<'_, AppState>,
    settings: crate::dto::SettingsDto,
) -> Result<(), WireError> {
    let new = crate::settings::Settings::from(settings);
    crate::settings::save(&state.storage_root, &new)
        .map_err(|e| AppError::Io(e.to_string()).to_wire())?;
    let mut g = state.settings.lock().await;
    *g = new;
    Ok(())
}
```

(Add `AppError::Io(String)` to `rm-error` if the variant doesn't already exist. Verify in `crates/error/src/lib.rs`; if missing, add `#[error("i/o error: {0}")] Io(String),` to `AppError` and the equivalent kind in `to_wire`.)

- [ ] **Step 3: Add `settings` field to `AppState`**

In `crates/app/src/state.rs`, modify `AppState`:

```rust
pub struct AppState {
    pub storage_root: PathBuf,
    pub driver_hub: Mutex<Option<Arc<DriverHub>>>,
    pub active: Mutex<Option<ActivePlayback>>,
    pub recording: Mutex<Option<ActiveRecording>>,
    pub settings: Mutex<crate::settings::Settings>,
}
```

Update `AppState::new`:

```rust
impl AppState {
    pub fn new(storage_root: PathBuf, settings: crate::settings::Settings) -> Self {
        Self {
            storage_root,
            driver_hub: Mutex::new(None),
            active: Mutex::new(None),
            recording: Mutex::new(None),
            settings: Mutex::new(settings),
        }
    }
}
```

- [ ] **Step 4: Update `main.rs` to load settings at boot + register commands**

```rust
fn main() {
    tracing_subscriber::fmt()...

    let storage_root = dirs::data_dir()
        .map(|d| d.join("rust-macro"))
        .unwrap_or_else(|| PathBuf::from("./.rust-macro"));

    // Load settings before constructing AppState. Failure to load is fatal
    // (corrupt settings.json means the user needs to delete it manually —
    // silent overwrite would lose their config).
    let settings = settings::load(&storage_root).unwrap_or_else(|e| {
        eprintln!("warning: settings load failed ({e}); using defaults");
        settings::Settings::default()
    });

    tauri::Builder::default()
        .manage(AppState::new(storage_root, settings))
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
            commands::load_settings,
            commands::save_settings,
        ])
        .on_window_event(...)  // unchanged from 3b
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 5: Compile + run tests**

Run: `cargo check -p rm-app`
Expected: PASS.

Run: `cargo test -p rm-app`
Expected: PASS — all prior tests still green (the new `settings` field on AppState doesn't break the existing test fixtures because they construct via `fixture_state` which needs a matching signature update).

Update `fixture_state` in `crates/app/src/commands.rs` `mod tests`:

```rust
    fn fixture_state() -> (TempDir, AppState) {
        let tmp = TempDir::new().unwrap();
        let state = AppState::new(tmp.path().to_path_buf(), crate::settings::Settings::default());
        (tmp, state)
    }
```

Re-run: `cargo test -p rm-app`
Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add crates/app/src/dto.rs crates/app/src/commands.rs crates/app/src/state.rs crates/app/src/main.rs crates/error/src/lib.rs
git commit -m "feat(app): SettingsDto + load_settings/save_settings commands"
```

---

## Task 6: Use `settings.stop_key` in `start_recording`

**Files:**
- Modify: `crates/app/src/recording.rs`
- Modify: `crates/app/src/commands.rs`

Currently the recorder always uses the hardcoded `STOP_KEY = KeyCode::F10`. Plan 3c reads it from settings.

- [ ] **Step 1: Remove the const, take stop_key as a parameter**

In `crates/app/src/recording.rs`, delete `pub const STOP_KEY: KeyCode = KeyCode::F10;`. The const is no longer used.

- [ ] **Step 2: Update `start_recording` to read from settings**

In `crates/app/src/commands.rs`, replace the `STOP_KEY` reference:

```rust
    // Read stop key from settings (default F10; user-configurable via the
    // Settings page).
    let stop_key = state.settings.lock().await.stop_key;

    let handle = rm_recorder::start_recording_with_stop_key(
        hub.clone(),
        true,
        Some(stop_key),
    );
```

Also update the `use` statement: `use crate::recording::{spawn_supervisor, RecordingStartedEvent};` (drop `STOP_KEY`).

- [ ] **Step 3: Run all tests**

Run: `cargo test -p rm-app`
Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add crates/app/src/recording.rs crates/app/src/commands.rs
git commit -m "feat(app): recording reads stop_key from settings (was hardcoded F10)"
```

---

## Task 7: Persistent listener — `crates/app/src/listener.rs`

**Files:**
- Create: `crates/app/src/listener.rs`
- Modify: `crates/app/src/main.rs`

The listener owns a **filtered** Interception context, a passthrough subscriber that forwards every received event back to the OS, and an rm-hotkey listener that dispatches macros. It runs for the app's lifetime.

**Critical:** the passthrough subscriber MUST be paused while recording is active (the recorder owns passthrough during a session — otherwise events double-forward). It also pauses the hotkey dispatcher (otherwise F10 would trigger a macro instead of stopping the recording).

- [ ] **Step 1: Create `crates/app/src/listener.rs`**

```rust
//! Persistent listener — runs from app boot to shutdown. Owns a single
//! filtered `Arc<DriverHub>` and subscribes for two purposes:
//!   1. **Passthrough forwarding**: every received event is re-sent via
//!      `hub.send(event)` so the OS keeps seeing user input. Paused while
//!      a recording session is active (the recorder owns passthrough).
//!   2. **Hotkey dispatch**: rm-hotkey's `start_listener` watches for trigger
//!      matches and emits `HotkeyHit(id)`. The dispatcher task receives those
//!      and calls `play_macro` internally. Paused while a recording or
//!      playback is active.

use std::sync::Arc;

use rm_driver::DriverHub;
use rm_hotkey::{start_listener as start_hotkey_listener, HotkeyHit, HotkeyRegistry, ListenerHandle};
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use crate::state::AppState;

pub struct ActiveListener {
    pub hub: Arc<DriverHub>,
    pub registry: HotkeyRegistry,
    pub hotkey_handle: Option<ListenerHandle>,
    pub passthrough_stop_tx: Option<oneshot::Sender<()>>,
    pub dispatcher_stop_tx: Option<oneshot::Sender<()>>,
}

/// Open Interception (with filters), spawn passthrough + dispatcher tasks.
/// Returns the assembled `ActiveListener` for storage in AppState.
pub fn start(app: AppHandle, registry: HotkeyRegistry) -> Result<ActiveListener, rm_error::AppError> {
    let drv: Arc<dyn rm_driver::Driver> = Arc::new(
        rm_driver_interception::open_with_status()?,
    );
    let hub = DriverHub::start(drv);

    // Passthrough subscriber — synchronous subscribe per DriverHub invariant.
    let pt_rx = hub.subscribe().ok_or_else(|| {
        rm_error::AppError::Other("listener: hub already shut down".into())
    })?;
    let (pt_stop_tx, mut pt_stop_rx) = oneshot::channel();
    let pt_hub = hub.clone();
    tokio::spawn(async move {
        let mut rx = pt_rx;
        loop {
            tokio::select! {
                _ = &mut pt_stop_rx => { debug!("listener passthrough: stop"); break; }
                got = rx.recv() => match got {
                    Ok(event) => {
                        if let Err(e) = pt_hub.send(event).await {
                            debug!(error = ?e, "listener passthrough: send failed");
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(lagged = n, "listener passthrough: dropped events");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        debug!("listener passthrough: hub closed");
                        break;
                    }
                }
            }
        }
    });

    // Hotkey listener — uses rm-hotkey, emits HotkeyHit on the channel.
    let (hit_tx, hit_rx) = mpsc::unbounded_channel();
    let hotkey_handle = start_hotkey_listener(hub.clone(), registry.clone(), hit_tx);

    // Dispatcher — pumps HotkeyHit and calls play_macro via the AppHandle.
    let (disp_stop_tx, mut disp_stop_rx) = oneshot::channel();
    let app_for_disp = app.clone();
    tokio::spawn(async move {
        let mut rx = hit_rx;
        loop {
            tokio::select! {
                _ = &mut disp_stop_rx => { debug!("listener dispatcher: stop"); break; }
                hit = rx.recv() => match hit {
                    Some(HotkeyHit(id)) => {
                        // Skip if recording or playback is currently active.
                        if let Some(s) = app_for_disp.try_state::<AppState>() {
                            let busy = s.recording.lock().await.is_some()
                                    || s.active.lock().await.is_some();
                            if busy {
                                debug!(macro_id = %id, "dispatcher: skipping (busy)");
                                continue;
                            }
                        }
                        // Dispatch by directly invoking the play_macro logic.
                        // The simplest path is to call the command function;
                        // it accepts State + AppHandle which we synthesize
                        // here. See dispatcher_invoke_play below.
                        if let Err(e) = dispatch_play(&app_for_disp, id).await {
                            warn!(error = ?e, macro_id = %id, "dispatcher: play failed");
                        }
                    }
                    None => break,
                }
            }
        }
    });

    Ok(ActiveListener {
        hub,
        registry,
        hotkey_handle: Some(hotkey_handle),
        passthrough_stop_tx: Some(pt_stop_tx),
        dispatcher_stop_tx: Some(disp_stop_tx),
    })
}

/// Direct invocation of `play_macro`'s body bypassing the `#[tauri::command]`
/// wrapper. Same lookup → guard → spawn-supervisor sequence.
async fn dispatch_play(app: &AppHandle, id: uuid::Uuid) -> Result<(), rm_error::AppError> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| rm_error::AppError::Other("dispatcher: AppState missing".into()))?;
    crate::commands::play_macro_internal(app.clone(), &state, id).await
}
```

(Yes — this needs a new `play_macro_internal` helper in `commands.rs`. Add it in Step 2.)

- [ ] **Step 2: Extract `play_macro_internal` in `commands.rs`**

Refactor `play_macro` to delegate to a helper that takes `&AppState` instead of `State<'_, AppState>`. The Tauri-command wrapper stays for the frontend; the listener calls the helper directly.

Concrete refactor of `play_macro`:

**Before** (current `play_macro` at `crates/app/src/commands.rs`):
```rust
#[tauri::command]
pub async fn play_macro(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<(), WireError> {
    // recording guard → load → ensure_hub → reserve active → spawn supervisor → emit started → Ok
    [~70 lines, see commands.rs lines ~187-282]
}
```

**After:** rename current body to `play_macro_internal` and add a thin Tauri wrapper. Replace the function with these two:

```rust
/// Reusable body of `play_macro`. Used by the Tauri command wrapper AND by the
/// listener's dispatcher task. Takes a `&AppState` borrow (not Tauri's `State`)
/// so it can be invoked from a plain tokio task that only has an `AppHandle`.
pub(crate) async fn play_macro_internal(
    app: AppHandle,
    state: &AppState,
    id: Uuid,
) -> Result<(), AppError> {
    // [Take the existing play_macro body verbatim. Two mechanical changes:]
    //   1. Drop the `state: State<'_, AppState>` parameter signature change
    //      (the new signature already takes &AppState).
    //   2. Where the original code wrote `state.foo.lock().await`, leave it
    //      as-is — auto-deref on the borrow works identically.
    //   3. Remove every `.to_wire()` call inside the body — return AppError
    //      directly. The Tauri wrapper below maps to WireError at the edge.
    //   4. The final return becomes `Ok(())` instead of `Ok(())` wrapped in
    //      WireError (it already is `Ok(())` — just drop the WireError type
    //      from the function signature).

    // [paste the existing body here with the mechanical changes above]
}

#[tauri::command]
pub async fn play_macro(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<(), WireError> {
    play_macro_internal(app, &state, id).await.map_err(|e| e.to_wire())
}
```

The supervisor task spawned inside `play_macro_internal` still emits `playback_finished` via `app.emit(...)` — no change. The only callers of `play_macro` (frontend invoke + listener dispatcher) both reach the same logic.

- [ ] **Step 3: Register `mod listener;`**

In `crates/app/src/main.rs`, after `mod recording;`:

```rust
mod listener;
```

- [ ] **Step 4: Compile-check**

Run: `cargo check -p rm-app`
Expected: PASS.

Run: `cargo check -p rm-app --no-default-features`
Expected: PASS — the listener depends on `rm-driver-interception`. The `interception` feature is default-on; the `listener::start` function will be unreachable in the no-default-features build. Confirm `listener.rs` is gated behind `#[cfg(feature = "interception")]` if needed, OR keep it unconditional (it'll just fail at runtime when `open_with_status` returns DriverNotInstalled). Recommendation: gate the entire `listener` module with `#[cfg(feature = "interception")]` in `main.rs`:

```rust
#[cfg(feature = "interception")]
mod listener;
```

- [ ] **Step 5: Commit**

```powershell
git add crates/app/src/listener.rs crates/app/src/commands.rs crates/app/src/main.rs
git commit -m "feat(app): persistent listener module (filtered hub + passthrough + dispatcher)"
```

---

## Task 8: Boot the listener at startup + add `listener` field to AppState

**Files:**
- Modify: `crates/app/src/state.rs`
- Modify: `crates/app/src/main.rs`

The listener handle needs to live for the app lifetime so its tasks aren't dropped. Store it in AppState.

- [ ] **Step 1: Add the field to AppState**

In `crates/app/src/state.rs`:

```rust
pub struct AppState {
    pub storage_root: PathBuf,
    pub driver_hub: Mutex<Option<Arc<DriverHub>>>,
    pub active: Mutex<Option<ActivePlayback>>,
    pub recording: Mutex<Option<ActiveRecording>>,
    pub settings: Mutex<crate::settings::Settings>,
    #[cfg(feature = "interception")]
    pub listener: Mutex<Option<crate::listener::ActiveListener>>,
}

impl AppState {
    pub fn new(storage_root: PathBuf, settings: crate::settings::Settings) -> Self {
        Self {
            storage_root,
            driver_hub: Mutex::new(None),
            active: Mutex::new(None),
            recording: Mutex::new(None),
            settings: Mutex::new(settings),
            #[cfg(feature = "interception")]
            listener: Mutex::new(None),
        }
    }
}
```

- [ ] **Step 2: Boot the listener in `main.rs`**

Replace the existing builder with:

```rust
    let builder = tauri::Builder::default()
        .manage(AppState::new(storage_root, settings))
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
            commands::load_settings,
            commands::save_settings,
        ])
        .on_window_event(|window, event| {
            // ... existing 3b on_window_event body ...
        });

    #[cfg(feature = "interception")]
    let builder = builder.setup(|app| {
        let app_handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            // Build the registry from all macros currently on disk.
            let registry = rm_hotkey::HotkeyRegistry::new();
            if let Some(state) = app_handle.try_state::<AppState>() {
                if let Ok(macros) = rm_storage::load_all(&state.storage_root) {
                    for m in macros {
                        registry.bind(m.id, m.trigger).await;
                    }
                }
            }

            // Start the listener.
            match listener::start(app_handle.clone(), registry) {
                Ok(active) => {
                    if let Some(state) = app_handle.try_state::<AppState>() {
                        *state.listener.lock().await = Some(active);
                    }
                }
                Err(e) => {
                    tracing::warn!(error = ?e, "listener failed to start (driver not installed?); hotkeys disabled");
                }
            }
        });
        Ok(())
    });

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
```

- [ ] **Step 3: Compile-check + tests**

Run: `cargo check -p rm-app && cargo test -p rm-app`
Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add crates/app/src/state.rs crates/app/src/main.rs
git commit -m "feat(app): boot persistent listener at startup + store handle in AppState"
```

---

## Task 9: Refresh registry on macro CRUD

**Files:**
- Modify: `crates/app/src/commands.rs`

After every `create_macro`, `update_macro_metadata`, `update_macro_full`, or `delete_macro`, the registry must be updated so the new/changed/removed trigger takes effect immediately. Otherwise the user has to restart the app for hotkey changes to apply.

- [ ] **Step 1: Add a helper to refresh registry from the current macro list**

In `crates/app/src/commands.rs`:

```rust
#[cfg(feature = "interception")]
async fn refresh_registry(state: &AppState) {
    let listener_guard = state.listener.lock().await;
    let Some(listener) = listener_guard.as_ref() else { return };
    let registry = listener.registry.clone();
    drop(listener_guard);

    // Clear and rebuild from disk.
    if let Ok(macros) = rm_storage::load_all(&state.storage_root) {
        // Naive rebuild: unbind all known ids, then rebind. The registry
        // doesn't expose a "clear all" so we unbind by id from the on-disk
        // set; any id no longer on disk is naturally absent.
        for m in &macros {
            registry.unbind(m.id).await;
        }
        for m in macros {
            registry.bind(m.id, m.trigger).await;
        }
    }
}

#[cfg(not(feature = "interception"))]
async fn refresh_registry(_state: &AppState) {}
```

(Better: extend `rm-hotkey` with a `clear()` method or `rebind_all(vec)` for atomic rebuild. If you do, the helper above simplifies to a single call. Implementer's choice — both approaches work.)

- [ ] **Step 2: Call `refresh_registry` from each CRUD command**

Add at the end of `create_macro`, `update_macro_metadata`, `update_macro_full`, and `delete_macro` (before the final `Ok(...)`):

```rust
    refresh_registry(&state).await;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p rm-app`
Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add crates/app/src/commands.rs
git commit -m "feat(app): refresh hotkey registry on macro create/update/delete"
```

---

## Task 10: Pause listener passthrough + dispatcher during recording

**Files:**
- Modify: `crates/app/src/commands.rs` (start_recording / stop_recording)
- Modify: `crates/app/src/listener.rs`

The recorder owns passthrough during a session (it forwards every event via the recorder loop's `hub.send`). The listener's passthrough must pause to avoid double forwarding. The dispatcher must also pause so F10 isn't interpreted as a macro trigger.

- [ ] **Step 1: Add pause/resume methods on `ActiveListener`**

In `crates/app/src/listener.rs`, replace the `ActiveListener` struct's stop fields with re-creatable subscribers. The simplest design: keep the listener struct as-is but add boolean flags + AtomicBool gates inside each task. When the flag is set, the task continues reading from `rx.recv()` but drops the event without forwarding/dispatching.

Concretely, add an `Arc<AtomicBool> pause` shared by both passthrough and dispatcher tasks. In `start()`, capture it in both task closures:

```rust
use std::sync::atomic::{AtomicBool, Ordering};

pub struct ActiveListener {
    pub hub: Arc<DriverHub>,
    pub registry: HotkeyRegistry,
    pub paused: Arc<AtomicBool>,
    pub hotkey_handle: Option<ListenerHandle>,
    pub passthrough_stop_tx: Option<oneshot::Sender<()>>,
    pub dispatcher_stop_tx: Option<oneshot::Sender<()>>,
}
```

Update both task bodies (passthrough + dispatcher) to check `paused.load(Ordering::SeqCst)` at the top of each iteration and skip forwarding/dispatching when true.

In `start()`:

```rust
    let paused = Arc::new(AtomicBool::new(false));
    let pt_paused = paused.clone();
    let disp_paused = paused.clone();
    // [pass these into the spawned tasks]
```

Inside the passthrough task body:

```rust
                got = rx.recv() => match got {
                    Ok(event) => {
                        if pt_paused.load(Ordering::SeqCst) { continue; }
                        if let Err(e) = pt_hub.send(event).await { ... }
                    }
                    ...
                }
```

Inside the dispatcher task body:

```rust
                hit = rx.recv() => match hit {
                    Some(HotkeyHit(id)) => {
                        if disp_paused.load(Ordering::SeqCst) { continue; }
                        // [rest of existing dispatch logic]
                    }
                    ...
                }
```

Return the `paused` Arc on the `ActiveListener` struct.

- [ ] **Step 2: Pause on `start_recording`; resume on supervisor cleanup**

In `crates/app/src/commands.rs`'s `start_recording`, after reserving the slot:

```rust
    // Pause the listener (passthrough + dispatcher) so it doesn't double-
    // forward events the recorder is already forwarding, and so F10 doesn't
    // trigger an incidental macro.
    #[cfg(feature = "interception")]
    if let Some(l) = state.listener.lock().await.as_ref() {
        l.paused.store(true, std::sync::atomic::Ordering::SeqCst);
    }
```

In `crates/app/src/recording.rs`'s `spawn_supervisor`, in the cleanup block (after clearing the slot):

```rust
        #[cfg(feature = "interception")]
        if let Some(s) = app.try_state::<AppState>() {
            if let Some(l) = s.listener.lock().await.as_ref() {
                l.paused.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
```

- [ ] **Step 3: Compile + test**

Run: `cargo check -p rm-app && cargo test -p rm-app`
Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add crates/app/src/listener.rs crates/app/src/commands.rs crates/app/src/recording.rs
git commit -m "feat(app): pause listener passthrough+dispatcher during recording"
```

---

## Task 11: Frontend types — Trigger mouse variant + SettingsDto

**Files:**
- Modify: `crates/app/ui/src/lib/types.ts`

- [ ] **Step 1: Append to `types.ts`**

After the existing `Trigger` declaration, replace it with:

```ts
export type Trigger =
  | { type: "hotkey"; key: KeyCode; modifiers: Modifier[] }
  | { type: "mouse_button"; button: MouseButton; modifiers: Modifier[] };

export type SettingsDto = {
  stop_key: KeyCode;
  storage_root_override: string | null;
};

/** Human-readable label for a Trigger. */
export function triggerLabel(t: Trigger): string {
  const mods = t.modifiers.map(inputLabel).join("+");
  const tail = t.type === "hotkey" ? inputLabel(t.key) : `Mouse:${inputLabel(t.button)}`;
  return mods ? `${mods}+${tail}` : tail;
}
```

(The existing `Trigger` type only had the `hotkey` variant. Replacing it is a breaking change for any consumer using `value.key` directly without narrowing. The compiler will catch them — fix each by adding `if (t.type === "hotkey")` guards.)

- [ ] **Step 2: Update all consumers**

Run `npm run build` and fix every TypeScript error. Likely culprits:
- `HotkeyPicker.svelte` — currently assumes `value.modifiers` and `value.key`. Add type narrowing.
- `MacroRow.svelte` — if it formats the trigger.
- `RecordingModal.svelte` — has `trigger: Trigger` form state with `{ type: "hotkey", ... }`. Continues to work.
- `StepEditor.svelte` — same.

For now, every place that creates a default trigger should default to `{ type: "hotkey", key: "f1", modifiers: ["ctrl"] }`.

- [ ] **Step 3: `npm run build` — must pass**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add crates/app/ui/src/lib/types.ts crates/app/ui/src/lib/components/HotkeyPicker.svelte crates/app/ui/src/lib/components/MacroRow.svelte
git commit -m "feat(app/ui): Trigger mouse_button variant + SettingsDto + triggerLabel"
```

---

## Task 12: HotkeyPicker — mode toggle (key vs mouse)

**Files:**
- Modify: `crates/app/ui/src/lib/components/HotkeyPicker.svelte`

The picker needs a mode switch: "Keyboard" (current behavior) or "Mouse" (button select). The Capture button works in both modes; for mouse mode it captures a mouse button + modifiers.

- [ ] **Step 1: Add a mode toggle + mouse button select**

Replace the picker's UI. The high-level structure:

```svelte
<script lang="ts">
  // existing imports + type, plus MouseButton
  import type { Trigger, KeyCode, Modifier, MouseButton } from "../types";
  import { inputLabel } from "../types";

  let { value, onChange }: { value: Trigger; onChange: (t: Trigger) => void } = $props();

  const MOUSE_BUTTON_OPTIONS: MouseButton[] = ["left", "right", "middle", "x1", "x2"];

  function setMode(mode: "hotkey" | "mouse_button") {
    if (value.type === mode) return;
    if (mode === "hotkey") {
      onChange({ type: "hotkey", key: "f1", modifiers: value.modifiers });
    } else {
      onChange({ type: "mouse_button", button: "x1", modifiers: value.modifiers });
    }
  }

  function changeMouseButton(e: Event) {
    if (value.type !== "mouse_button") return;
    const button = (e.target as HTMLSelectElement).value as MouseButton;
    onChange({ ...value, button });
  }

  // The existing `toggle(mod)` (modifier checkboxes) works for both modes —
  // it operates on `value.modifiers`, which is present on both Trigger
  // variants. Keep it unchanged.
  //
  // `changeKey` only fires from the keyboard <select>; it asserts
  // `value.type === "hotkey"`. Keep it unchanged.
  //
  // `startCapture` / `stopCapture` from 3b stays for keyboard mode. Add a
  // SEPARATE pair (`startMouseCapture` / `stopMouseCapture`) for mouse mode
  // — listens to `mousedown` on window instead of `keydown`. The capture
  // button below switches based on `value.type`.
</script>

<div class="mode-row">
  <label><input type="radio" name="trigger-mode" checked={value.type === "hotkey"} onchange={() => setMode("hotkey")} /> Keyboard</label>
  <label><input type="radio" name="trigger-mode" checked={value.type === "mouse_button"} onchange={() => setMode("mouse_button")} /> Mouse</label>
</div>

<!-- modifier checkboxes here (same as before) -->

{#if value.type === "hotkey"}
  <!-- existing key select + capture button -->
{:else}
  <select onchange={changeMouseButton} value={value.button}>
    {#each MOUSE_BUTTON_OPTIONS as b}<option value={b}>{inputLabel(b)}</option>{/each}
  </select>
  <!-- Capture button for mouse: similar to keyboard, but listens for mousedown events on window. -->
{/if}
```

- [ ] **Step 2: Add mouse capture logic**

For mouse capture, listen for `mousedown` on window with `{ capture: true }`. Detect button via `e.button` (0=left, 1=middle, 2=right, 3=x1, 4=x2). Map and commit. Esc cancels (same as keyboard capture).

```ts
function onMouseDown(e: MouseEvent) {
  e.preventDefault();
  e.stopPropagation();
  const map: Record<number, MouseButton> = { 0: "left", 1: "middle", 2: "right", 3: "x1", 4: "x2" };
  const btn = map[e.button];
  if (btn) {
    const modifiers = modifiersFromMouseEvent(e);
    onChange({ type: "mouse_button", button: btn, modifiers });
    stopMouseCapture();
  }
}
```

Add a mode-aware `startCapture` / `stopCapture` that toggles between keyboard and mouse listeners.

- [ ] **Step 3: Build + commit**

```powershell
cd crates/app/ui && npm run build && cd ..\..\..
git add crates/app/ui/src/lib/components/HotkeyPicker.svelte
git commit -m "feat(app/ui): HotkeyPicker keyboard/mouse mode toggle + mouse capture"
```

---

## Task 13: Frontend api.ts — settings wrappers

**Files:**
- Modify: `crates/app/ui/src/lib/api.ts`

- [ ] **Step 1: Append wrappers**

```ts
import type { SettingsDto } from "./types";  // consolidate with existing imports

export async function loadSettings(): Promise<SettingsDto> {
  return invoke<SettingsDto>("load_settings");
}

export async function saveSettings(settings: SettingsDto): Promise<void> {
  await invoke("save_settings", { settings });
}
```

- [ ] **Step 2: Build + commit**

```powershell
cd crates/app/ui && npm run build && cd ..\..\..
git add crates/app/ui/src/lib/api.ts
git commit -m "feat(app/ui): api wrappers for load_settings + save_settings"
```

---

## Task 14: Settings store + SettingsView

**Files:**
- Create: `crates/app/ui/src/lib/stores/settings.ts`
- Create: `crates/app/ui/src/lib/components/SettingsView.svelte`

- [ ] **Step 1: Create `stores/settings.ts`**

```ts
import { writable } from "svelte/store";
import type { SettingsDto, KeyCode } from "../types";
import * as api from "../api";
import { reportError, pushToast } from "./toast";

const DEFAULT_SETTINGS: SettingsDto = {
  stop_key: "f10",
  storage_root_override: null,
};

export const settings = writable<SettingsDto>(DEFAULT_SETTINGS);

export async function load(): Promise<void> {
  try {
    const s = await api.loadSettings();
    settings.set(s);
  } catch (e) {
    reportError(e);
  }
}

export async function save(s: SettingsDto): Promise<void> {
  try {
    await api.saveSettings(s);
    settings.set(s);
    pushToast("info", "Settings saved.");
  } catch (e) {
    reportError(e);
  }
}
```

- [ ] **Step 2: Create `SettingsView.svelte`**

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { settings, load as loadSettings, save as saveSettings } from "../stores/settings";
  import type { KeyCode } from "../types";

  let { onBack }: { onBack: () => void } = $props();

  let stopKey = $state<KeyCode>("f10");
  let storageOverride = $state<string>("");
  let saving = $state(false);

  onMount(async () => {
    await loadSettings();
    const s = $settings;
    stopKey = s.stop_key;
    storageOverride = s.storage_root_override ?? "";
  });

  const STOP_KEY_OPTIONS: KeyCode[] = [
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
    "escape", "pause",
  ];

  async function save() {
    saving = true;
    await saveSettings({
      stop_key: stopKey,
      storage_root_override: storageOverride.trim() === "" ? null : storageOverride.trim(),
    });
    saving = false;
  }
</script>

<main>
  <header>
    <button class="back" onclick={onBack}>← Back</button>
    <div class="spacer"></div>
    <button class="primary" disabled={saving} onclick={save}>{saving ? "Saving…" : "Save"}</button>
  </header>

  <h2>Settings</h2>

  <div class="field">
    <label for="stop-key">Recording stop key</label>
    <select id="stop-key" bind:value={stopKey}>
      {#each STOP_KEY_OPTIONS as k}<option value={k}>{k.toUpperCase()}</option>{/each}
    </select>
    <p class="hint">Pressed during a recording to stop it. Default: F10.</p>
  </div>

  <div class="field">
    <label for="storage-root">Storage root override</label>
    <input id="storage-root" bind:value={storageOverride} placeholder="(default: %AppData%\rust-macro)" />
    <p class="hint">
      Leave empty for the default. Changing this does NOT move existing macros —
      restart the app after changing.
    </p>
  </div>
</main>

<style>
  main { max-width: 720px; margin: 0 auto; padding: 1.5rem; }
  header { display: flex; gap: 0.5rem; align-items: center; margin-bottom: 1.5rem; }
  .back { background: transparent; }
  .spacer { flex: 1; }
  .field { margin-bottom: 1rem; }
  .field > label { display: block; font-size: 0.85rem; color: var(--text-muted); margin-bottom: 0.35rem; text-transform: uppercase; letter-spacing: 0.05em; }
  .field input, .field select { width: 100%; max-width: 360px; }
  .hint { color: var(--text-muted); font-size: 0.8rem; margin: 0.25rem 0 0 0; }
</style>
```

- [ ] **Step 3: Build + commit**

```powershell
cd crates/app/ui && npm run build && cd ..\..\..
git add crates/app/ui/src/lib/stores/settings.ts crates/app/ui/src/lib/components/SettingsView.svelte
git commit -m "feat(app/ui): settings store + SettingsView (stop_key + storage_root)"
```

---

## Task 15: App router + gear icon

**Files:**
- Modify: `crates/app/ui/src/App.svelte`
- Modify: `crates/app/ui/src/lib/components/MacroTable.svelte`

- [ ] **Step 1: Add `settings` tag to view router**

In `App.svelte`:

```svelte
type View =
  | { tag: "list" }
  | { tag: "editor"; macroId: string }
  | { tag: "settings" };

function handleSettings() { view = { tag: "settings" }; }
function backToList() { view = { tag: "list" }; }
```

Add in the router:

```svelte
{:else if view.tag === "settings"}
  <SettingsView onBack={backToList} />
  <ToastHost />
```

Add the import: `import SettingsView from "./lib/components/SettingsView.svelte";`

Pass `onSettings={handleSettings}` to `MacroTable`.

- [ ] **Step 2: Add gear icon to MacroTable header**

In `MacroTable.svelte`:

```svelte
let {
  onPlay,
  onEdit,
  onRecord,
  onSettings,
}: {
  onPlay: (id: string) => void;
  onEdit: (id: string) => void;
  onRecord: () => void;
  onSettings: () => void;
} = $props();
```

Header row:

```svelte
<div class="header">
  <h2>Macros</h2>
  <div class="header-actions">
    <button class="icon" onclick={onSettings} title="Settings">⚙</button>
    <button class="primary" onclick={onRecord}>+ Record new</button>
  </div>
</div>
```

CSS additions:

```css
.header-actions { display: flex; gap: 0.5rem; align-items: center; }
.icon { background: transparent; padding: 0.4rem 0.5rem; font-size: 1.1rem; }
```

- [ ] **Step 3: Build + commit**

```powershell
cd crates/app/ui && npm run build && cd ..\..\..
git add crates/app/ui/src/App.svelte crates/app/ui/src/lib/components/MacroTable.svelte
git commit -m "feat(app/ui): view router + gear icon for Settings"
```

---

## Task 16: README updates

**Files:**
- Modify: `crates/app/README.md`

Add smoke-test items for Plan 3c features:

```markdown
14. **Global hotkey.** Save a macro with hotkey Ctrl+F1. Switch to another app
    (Notepad). Press Ctrl+F1. The macro plays without you clicking ▶.
15. **Mouse-button trigger.** Edit a macro; switch HotkeyPicker to "Mouse"
    mode; bind X1 (back button). Save. From any app, press X1 — macro plays.
16. **Settings — stop key.** Open ⚙ Settings. Change stop key to Escape.
    Save. Start a new recording; press Escape to stop instead of F10.
17. **Step compaction.** Record a macro while waving the mouse across the
    screen. Open the editor; step count should be a handful (one MouseMove
    per pause), not hundreds.
18. **Listener resilience.** Restart the app. Without playing anything, type
    normally and use the mouse — input flows through, no freeze.
```

Update the "Known limitations" section to remove items now implemented (hotkey listener, settings page) and add residual Plan 3d+ items (system tray, conflict detection, etc.).

- [ ] **Step 1: Edit + commit**

```powershell
git add crates/app/README.md
git commit -m "docs(app): Plan 3c README smoke test additions"
```

---

## Task 17: Final verification

- [ ] **Step 1: All workspace tests pass**

```powershell
cargo test --workspace --no-fail-fast
```

Expected: PASS. Test count growth:
- `rm-macro-model`: +2 (trigger variant)
- `rm-hotkey`: +1 (mouse dispatch)
- `rm-recorder`: +2 (Wait filter)
- `rm-app`: +4 (settings tests)

- [ ] **Step 2: Frontend builds clean**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS.

- [ ] **Step 3: Tauri dev opens, listener boots, hotkeys trigger**

```powershell
cd crates/app
cargo tauri dev
```

Expected: window opens. Tracing log shows `listener: dispatcher` ready. Configured hotkeys fire macros from outside the app.

- [ ] **Step 4: Walk README smoke items 14–18**

If item 14 (global hotkey) fails: check whether the listener's filtered context loops back self-sent events. Add logging in `dispatch_play` for the dispatched id; observe whether the hotkey re-fires on the synthesized events. If so, the architecture needs a "synthetic flag" or per-event marker — file as a Plan 3d follow-up and document the limitation.

- [ ] **Step 5: No commit if Steps 1–3 pass**

---

## Acceptance Checklist

- [ ] `cargo test --workspace` is green.
- [ ] `cargo build -p rm-app` succeeds (default + `--no-default-features`).
- [ ] `cargo tauri dev` opens, listener boots when Interception is installed.
- [ ] Saved hotkey actually triggers playback from any focused app.
- [ ] Mouse buttons (X1/X2/middle/right/left + modifiers) can be set as trigger and trigger playback.
- [ ] Settings page lets the user change the recording stop key; the next recording uses the new key.
- [ ] Settings page can override the storage root (cosmetic for now; effective on next restart).
- [ ] Recorded macros with lots of mouse motion produce single coalesced MouseMove steps (verified in 3b commit `f94c009`; this plan inherits that behavior).
- [ ] Sub-20ms Waits are filtered out of compiled step lists.
- [ ] Listener pauses during recording — F10 (or configured key) stops the recording instead of triggering a macro.
- [ ] After playback ends, keyboard and mouse remain fully responsive (verified by Plan 3b commit `91690f9`; this plan inherits that behavior).

---

## Open Implementation Notes

- **`rm_storage::load_all` import in `listener.rs`/`main.rs`:** verify the path. If `load_all` is not directly accessible, mirror the existing `commands.rs` `use rm_storage::load_all;`.

- **The `paused` flag is racy by design.** A keystroke that lands between `paused.store(true)` and the recorder's first event won't double-forward (the recorder isn't subscribed yet) but might briefly miss a forward. Acceptable trade-off — the alternative is full task tear-down + respawn, which has its own race window.

- **Tauri 2 `setup` hook signature:** the snippet uses `.setup(|app| { ... Ok(()) })`. If the Tauri 2 version in use has a different signature (e.g., `Box::new(setup_fn)` wrapper), adapt minimally.

- **Listener startup latency:** opening Interception takes ~50-200ms. The window appears first; hotkeys may not work for the first few hundred ms after launch. Document in the smoke test ("wait until the title bar finishes loading before testing hotkeys").

- **Mouse-as-modifier-only is not allowed.** The HotkeyPicker's mouse mode requires a primary button — pure modifier combos (e.g., Ctrl+Shift only) can't be triggers, matching the keyboard rule.

- **Wait filter is non-configurable.** `MIN_WAIT_MS = 20` is a const. Future Plan 3d could expose it in Settings.

- **Storage root override:** the field is saved but the app doesn't dynamically switch storage root mid-session. Effective only after restart. Document in the SettingsView hint and the README.
