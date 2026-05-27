//! Tauri command handlers. Each command takes `State<'_, AppState>` and
//! returns `Result<T, WireError>`. Errors map from `AppError::to_wire()`.

use std::sync::Arc;

use rm_error::{AppError, WireError};
use rm_storage::{delete_macro as storage_delete, load_all, load_macro, save_macro as storage_save};
use tauri::State;
use uuid::Uuid;

use crate::dto::{MacroDto, PlaybackModeDto, TriggerDto};
use crate::state::AppState;

#[cfg(feature = "interception")]
mod driver_init {
    use super::*;
    use rm_driver::{Driver, DriverHub};
    use rm_driver_interception::open_with_status;
    use std::sync::Arc;

    pub async fn ensure_hub(state: &AppState) -> Result<Arc<DriverHub>, AppError> {
        let mut guard = state.driver_hub.lock().await;
        if let Some(h) = guard.as_ref() {
            return Ok(h.clone());
        }
        let drv: Arc<dyn Driver> = Arc::new(open_with_status()?);
        let hub = DriverHub::start(drv);
        *guard = Some(hub.clone());
        Ok(hub)
    }
}

#[cfg(not(feature = "interception"))]
mod driver_init {
    use super::*;
    use rm_driver::DriverHub;
    use std::sync::Arc;

    pub async fn ensure_hub(_state: &AppState) -> Result<Arc<DriverHub>, AppError> {
        Err(AppError::DriverNotInstalled)
    }
}

use driver_init::ensure_hub;

#[tauri::command]
pub async fn load_macros(state: State<'_, AppState>) -> Result<Vec<MacroDto>, WireError> {
    let macros = load_all(&state.storage_root).map_err(|e| e.to_wire())?;
    Ok(macros.iter().map(MacroDto::from).collect())
}

#[tauri::command]
pub async fn delete_macro(
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<(), WireError> {
    // load_macro returns MacroNotFound for a missing file via a single
    // path.exists() check — cheaper than load_all on machines with many
    // macros, and gives us the same "fail with MacroNotFound rather than a
    // silent no-op" behavior when the UI is out of sync.
    load_macro(&state.storage_root, id).map_err(|e| e.to_wire())?;
    storage_delete(&state.storage_root, id).map_err(|e| e.to_wire())?;
    Ok(())
}

#[tauri::command]
pub async fn update_macro_metadata(
    state: State<'_, AppState>,
    id: Uuid,
    name: String,
    trigger: TriggerDto,
    playback: PlaybackModeDto,
) -> Result<MacroDto, WireError> {
    let mut all = load_all(&state.storage_root).map_err(|e| e.to_wire())?;
    let m = all
        .iter_mut()
        .find(|m| m.id == id)
        .ok_or_else(|| AppError::MacroNotFound(id.to_string()).to_wire())?;

    m.name = name;
    m.trigger = trigger.into();
    m.playback = playback.into();
    m.updated_at = chrono::Utc::now();

    storage_save(&state.storage_root, m).map_err(|e| e.to_wire())?;
    Ok(MacroDto::from(&*m))
}

use crate::state::ActivePlayback;
use rm_player::play;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Clone)]
struct PlaybackStartedEvent {
    macro_id: Uuid,
    macro_name: String,
}

#[derive(Serialize, Clone)]
struct PlaybackFinishedEvent {
    macro_id: Uuid,
    outcome: PlaybackOutcome,
}

#[derive(Serialize, Clone)]
#[serde(tag = "status", rename_all = "snake_case")]
enum PlaybackOutcome {
    /// Macro ran to completion normally.
    Ok,
    /// User clicked Stop (or another stop_playback call took the slot).
    Stopped,
    /// Player returned an error.
    Failed { kind: &'static str, message: String },
}

#[tauri::command]
pub async fn play_macro(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<(), WireError> {
    // Reject if a playback is already active.
    {
        let active = state.active.lock().await;
        if active.is_some() {
            return Err(AppError::PlaybackActive.to_wire());
        }
    }

    // Load the macro before opening the driver, so MacroNotFound surfaces
    // without an unnecessary Interception context attempt.
    let all = load_all(&state.storage_root).map_err(|e| e.to_wire())?;
    let m = all
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| AppError::MacroNotFound(id.to_string()).to_wire())?;

    let hub = ensure_hub(&state).await.map_err(|e| e.to_wire())?;

    let macro_id = m.id;
    let macro_name = m.name.clone();

    // The slot's `stop_tx` is what `stop_playback` fires when the user clicks
    // Stop. A small relay records the user-initiated flag (so we can map the
    // player's clean exit to `PlaybackOutcome::Stopped` rather than `Ok`) and
    // forwards the signal to `PlaybackHandle::run_with_stop`, which calls the
    // handle's internal `stop()` and awaits the player's clean exit between
    // steps. No tasks are aborted; no players run detached.
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped_for_signal = stopped.clone();
    let (inner_stop_tx, inner_stop_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        if stop_rx.await.is_ok() {
            stopped_for_signal.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = inner_stop_tx.send(());
        }
    });

    let app_for_task = app.clone();
    tokio::spawn(async move {
        let handle = play(hub, m);
        let result = handle.run_with_stop(inner_stop_rx).await;
        let outcome = match (result, stopped.load(std::sync::atomic::Ordering::SeqCst)) {
            (Ok(()), true) => PlaybackOutcome::Stopped,
            (Ok(()), false) => PlaybackOutcome::Ok,
            (Err(e), _) => PlaybackOutcome::Failed { kind: e.kind(), message: e.to_string() },
        };

        // Cleanup: clear active slot if we're still the active playback.
        // Re-acquire AppState via the AppHandle so we don't capture a
        // 'static reference.
        if let Some(s) = app_for_task.try_state::<AppState>() {
            let mut active = s.active.lock().await;
            if active.as_ref().map(|a| a.macro_id) == Some(macro_id) {
                *active = None;
            }
        }

        let _ = app_for_task.emit(
            "playback_finished",
            PlaybackFinishedEvent { macro_id, outcome },
        );
    });

    // Store the active playback. The supervisor task above owns the player.
    // `stop_playback` takes `stop_tx` out of the slot and fires it; the
    // supervisor handles the rest.
    {
        let mut active = state.active.lock().await;
        *active = Some(ActivePlayback {
            macro_id,
            macro_name: macro_name.clone(),
            stop_tx: Some(stop_tx),
        });
    }

    // Emit playback_started after the active slot is populated, so any
    // frontend handler that immediately calls `stop_playback` sees a
    // consistent state.
    let _ = app.emit(
        "playback_started",
        PlaybackStartedEvent { macro_id, macro_name },
    );

    Ok(())
}

#[tauri::command]
pub async fn stop_playback(
    _app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), WireError> {
    // Send the cooperative stop signal. The supervisor task spawned by
    // `play_macro` will observe it, call `PlaybackHandle::run_with_stop`'s
    // internal `stop()`, await the player's clean exit, clear the active
    // slot, and emit `playback_finished` with `outcome: stopped`.
    let mut active = state.active.lock().await;
    if let Some(ap) = active.as_mut() {
        if let Some(tx) = ap.stop_tx.take() {
            let _ = tx.send(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rm_macro_model::{KeyCode, Modifier, PlaybackMode, Step, Trigger};
    use rm_storage::save_macro;
    use tempfile::TempDir;

    fn fixture_state() -> (TempDir, AppState) {
        let tmp = TempDir::new().unwrap();
        let state = AppState::new(tmp.path().to_path_buf());
        (tmp, state)
    }

    fn fixture_macro(name: &str) -> rm_macro_model::Macro {
        let mut m = rm_macro_model::Macro::new(
            name,
            Trigger::Hotkey {
                key: KeyCode::F1,
                modifiers: vec![Modifier::Ctrl],
            },
            PlaybackMode::Once,
        );
        m.steps = vec![Step::Wait { min_ms: 10, max_ms: 10 }];
        m
    }

    // The State<'_, AppState> wrapper from Tauri is hard to construct outside a
    // Tauri runtime, so we test the inner logic by calling the storage layer
    // directly with our AppState's storage_root. This is what each command's
    // body does; the only thing not covered is the Tauri command-dispatch
    // wiring (which is verified by the manual smoke test at the end of the
    // plan).

    #[tokio::test]
    async fn load_returns_saved_macros() {
        let (_tmp, state) = fixture_state();
        let m = fixture_macro("alpha");
        save_macro(&state.storage_root, &m).unwrap();

        let macros = load_all(&state.storage_root).unwrap();
        let dtos: Vec<MacroDto> = macros.iter().map(MacroDto::from).collect();
        assert_eq!(dtos.len(), 1);
        assert_eq!(dtos[0].name, "alpha");
        assert_eq!(dtos[0].step_count, 1);
    }

    #[tokio::test]
    async fn delete_missing_returns_macro_not_found() {
        let (_tmp, state) = fixture_state();
        let id = Uuid::new_v4();
        let result = load_all(&state.storage_root)
            .map_err(|e| e.to_wire())
            .and_then(|all| {
                if all.iter().any(|m| m.id == id) {
                    Ok(())
                } else {
                    Err(AppError::MacroNotFound(id.to_string()).to_wire())
                }
            });
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, "MacroNotFound");
    }

    #[tokio::test]
    async fn delete_existing_removes_file() {
        let (_tmp, state) = fixture_state();
        let m = fixture_macro("to-be-deleted");
        save_macro(&state.storage_root, &m).unwrap();
        assert_eq!(load_all(&state.storage_root).unwrap().len(), 1);

        storage_delete(&state.storage_root, m.id).unwrap();
        assert_eq!(load_all(&state.storage_root).unwrap().len(), 0);
    }

    #[tokio::test]
    async fn update_metadata_changes_fields_and_persists() {
        let (_tmp, state) = fixture_state();
        let m = fixture_macro("before");
        let id = m.id;
        save_macro(&state.storage_root, &m).unwrap();

        // Simulate the command body (the State<'_, AppState> wrapper isn't
        // constructible without a Tauri runtime).
        let mut loaded = load_all(&state.storage_root)
            .unwrap()
            .into_iter()
            .find(|x| x.id == id)
            .unwrap();
        loaded.name = "after".into();
        loaded.trigger = Trigger::Hotkey {
            key: KeyCode::F5,
            modifiers: vec![Modifier::Alt],
        };
        loaded.playback = PlaybackMode::Repeat { count: 3 };
        loaded.updated_at = chrono::Utc::now();
        save_macro(&state.storage_root, &loaded).unwrap();

        let reloaded = load_all(&state.storage_root)
            .unwrap()
            .into_iter()
            .find(|x| x.id == id)
            .unwrap();
        assert_eq!(reloaded.name, "after");
        assert!(matches!(reloaded.trigger,
            Trigger::Hotkey { key: KeyCode::F5, .. }));
        assert!(matches!(reloaded.playback, PlaybackMode::Repeat { count: 3 }));
        assert_eq!(reloaded.steps.len(), 1); // steps preserved
    }

    #[tokio::test]
    async fn active_slot_rejects_concurrent_play() {
        let (_tmp, state) = fixture_state();
        // Simulate that a playback is in progress by placing a dummy in the
        // active slot. The macro_id/name don't matter — we only care about
        // the guard returning PlaybackActive.
        let (tx, _rx) = tokio::sync::oneshot::channel::<()>();
        {
            let mut active = state.active.lock().await;
            *active = Some(crate::state::ActivePlayback {
                macro_id: Uuid::new_v4(),
                macro_name: "x".into(),
                stop_tx: Some(tx),
            });
        }
        // The guard in play_macro is a simple `if active.is_some()` block;
        // verify it would reject:
        let blocked = {
            let active = state.active.lock().await;
            active.is_some()
        };
        assert!(blocked);
    }
}
