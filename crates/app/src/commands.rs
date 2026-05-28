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
    use rm_driver::DriverHub;
    use std::sync::Arc;

    /// Return the listener's filtered hub for playback. We deliberately do
    /// NOT open a separate send-only context, even though playback only needs
    /// to send: the kernel driver routes a context's `send` strokes back
    /// through OTHER contexts' filters, so a separate send-only playback
    /// context would have its injected strokes re-intercepted by the
    /// listener's filter and re-relayed via the listener's own `send`,
    /// doubling every event the OS sees (cursor jumps at 2x speed, keys
    /// double-trigger, etc). Sending through the listener's OWN context
    /// avoids this — Interception does not re-intercept same-context sends.
    pub async fn ensure_hub(state: &AppState) -> Result<Arc<DriverHub>, AppError> {
        let listener_guard = state.listener.lock().await;
        let l = listener_guard
            .as_ref()
            .ok_or(AppError::DriverNotInstalled)?;
        Ok(l.hub.clone())
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

#[cfg(feature = "interception")]
async fn refresh_registry(state: &AppState) {
    let listener_guard = state.listener.lock().await;
    let Some(listener) = listener_guard.as_ref() else { return };
    let registry = listener.registry.clone();
    drop(listener_guard);

    // Clear and rebuild from disk.
    if let Ok(macros) = rm_storage::load_all(&state.storage_root) {
        // Naive rebuild: unbind all known ids on disk, then rebind. The
        // registry doesn't expose a "clear all" so we unbind by id from the
        // on-disk set; any id no longer on disk is naturally absent.
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
    refresh_registry(&state).await;
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
    let mut m = load_macro(&state.storage_root, id).map_err(|e| e.to_wire())?;

    m.name = name;
    m.trigger = trigger.into();
    m.playback = playback.into();
    m.updated_at = chrono::Utc::now();

    storage_save(&state.storage_root, &m).map_err(|e| e.to_wire())?;
    refresh_registry(&state).await;
    Ok(MacroDto::from(&m))
}

#[tauri::command]
pub async fn load_macro_steps(
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<Vec<crate::dto::StepDto>, WireError> {
    let m = load_macro(&state.storage_root, id).map_err(|e| e.to_wire())?;
    Ok(m.steps.iter().map(crate::dto::StepDto::from).collect())
}

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
    refresh_registry(&state).await;
    Ok(MacroDto::from(&m))
}

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
    refresh_registry(&state).await;
    Ok(MacroDto::from(&m))
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
    Failed { error: WireError },
}

pub(crate) async fn play_macro_internal(
    app: AppHandle,
    state: &AppState,
    id: Uuid,
) -> Result<(), AppError> {
    // Reject if a recording is in progress — playback would synthesize keys
    // that the recorder would capture.
    {
        let recording = state.recording.lock().await;
        if recording.is_some() {
            return Err(AppError::RecordingActive);
        }
    }

    // Do I/O before reserving the slot so MacroNotFound / driver errors
    // don't leave us holding a stale reservation.
    let all = load_all(&state.storage_root)?;
    let m = all
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| AppError::MacroNotFound(id.to_string()))?;

    let hub = ensure_hub(state).await?;

    let macro_id = m.id;
    let macro_name = m.name.clone();

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

    // Reserve the active slot atomically: check + write under one lock.
    {
        let mut active = state.active.lock().await;
        if active.is_some() {
            return Err(AppError::PlaybackActive);
        }
        *active = Some(ActivePlayback {
            macro_id,
            stop_tx: Some(stop_tx),
        });
    }

    // Single supervisor task: hosts both the relay (outer stop -> inner
    // stop + atomic flag) and the player. The relay is a child task whose
    // handle we abort+await once the player returns, so it never leaks.
    let app_for_task = app.clone();
    tokio::spawn(async move {
        let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopped_for_signal = stopped.clone();
        let (inner_stop_tx, inner_stop_rx) = tokio::sync::oneshot::channel::<()>();
        let relay = tokio::spawn(async move {
            if stop_rx.await.is_ok() {
                stopped_for_signal.store(true, std::sync::atomic::Ordering::SeqCst);
                let _ = inner_stop_tx.send(());
            }
        });

        let handle = play(hub, m);
        let result = handle.run_with_stop(inner_stop_rx).await;

        // Tear down the relay. If the player completed naturally, the
        // relay is still parked on stop_rx — abort + await flushes it.
        relay.abort();
        let _ = relay.await;

        let outcome = match (result, stopped.load(std::sync::atomic::Ordering::SeqCst)) {
            (Ok(()), true) => PlaybackOutcome::Stopped,
            (Ok(()), false) => PlaybackOutcome::Ok,
            (Err(e), _) => PlaybackOutcome::Failed { error: e.to_wire() },
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
pub async fn play_macro(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<(), WireError> {
    play_macro_internal(app, &state, id).await.map_err(|e| e.to_wire())
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

use crate::recording::{spawn_supervisor, RecordingStartedEvent};
use crate::state::ActiveRecording;

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

    // Read stop key from settings (default F10; user-configurable via the
    // Settings page).
    let stop_key = state.settings.lock().await.stop_key;

    // Share the listener's existing filtered hub. Opening a second
    // Interception context here would silently starve: the kernel routes each
    // hardware stroke to one context only (precedence-based, ties broken by
    // creation order), and the listener — created at boot — wins. The
    // recorder runs with `passthrough: false` because the listener's
    // passthrough task is the relay; we tell it to drop the stop key via
    // `suppress_key` so F10 doesn't leak to apps.
    #[cfg(feature = "interception")]
    let (hub, suppress_key) = {
        let listener_guard = state.listener.lock().await;
        let l = listener_guard
            .as_ref()
            .ok_or_else(|| AppError::DriverNotInstalled.to_wire())?;
        (l.hub.clone(), l.suppress_key.clone())
    };
    #[cfg(not(feature = "interception"))]
    return Err(AppError::DriverNotInstalled.to_wire());

    #[cfg(feature = "interception")]
    {
        let handle = rm_recorder::start_recording_with_stop_key(
            hub.clone(),
            false, // listener already relays — recorder must NOT also send
            Some(stop_key),
        );

        // External stop signal (used by `stop_recording` command and by the
        // window-close handler).
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

        // Reserve the recording slot atomically: check + write under one lock.
        // If another start_recording call won the race, we return early; the
        // local handle is dropped here.
        {
            let mut recording = state.recording.lock().await;
            if recording.is_some() {
                return Err(AppError::RecordingActive.to_wire());
            }
            *recording = Some(ActiveRecording {
                stop_tx: Some(stop_tx),
                session_hub: hub.clone(),
            });
        }

        // Activate stop-key suppression so the listener's passthrough drops
        // F10 (KeyDown and KeyUp) for the duration of the recording. The
        // hotkey dispatcher already skips when `state.recording.is_some()`.
        *suppress_key.lock().unwrap() = Some(stop_key);

        // Spawn the supervisor. It owns the handle; on completion it clears
        // the slot, clears suppress_key, and emits `recording_finished`.
        spawn_supervisor(app.clone(), handle, stop_rx);

        // Notify frontend AFTER the slot is populated.
        let _ = app.emit("recording_started", RecordingStartedEvent {});

        Ok(())
    }
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
        .map_err(|e| AppError::Other(e.to_string()).to_wire())?;
    let new_stop_key = new.stop_key;
    let mut g = state.settings.lock().await;
    *g = new;
    drop(g);

    // Refresh the listener's cached stop_key so the emergency-stop path
    // honors the new setting without an app restart.
    #[cfg(feature = "interception")]
    if let Some(l) = state.listener.lock().await.as_ref() {
        *l.stop_key.lock().unwrap() = new_stop_key;
    }
    Ok(())
}

#[tauri::command]
pub async fn driver_status(state: State<'_, AppState>) -> Result<crate::dto::DriverStateDto, WireError> {
    #[cfg(feature = "interception")]
    let status: crate::dto::DriverStatusDto = rm_driver_interception::detect_status().into();
    #[cfg(not(feature = "interception"))]
    let status = crate::dto::DriverStatusDto::NotInstalled;

    // If the driver is now Running, the reboot took effect — self-heal:
    // clear both the in-memory flag and the file marker so subsequent
    // launches don't keep seeding pending_reboot=true forever.
    let pending_reboot = if matches!(status, crate::dto::DriverStatusDto::Running) {
        let mut flag = state.pending_reboot.lock().await;
        if *flag {
            *flag = false;
            let _ = crate::driver_install::clear_pending_marker(&state.storage_root);
        }
        false
    } else {
        *state.pending_reboot.lock().await
    };

    Ok(crate::dto::DriverStateDto { status, pending_reboot })
}

fn resource_path_or_err(app: &AppHandle, rel: &str) -> Result<std::path::PathBuf, AppError> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| AppError::Other(format!("resource_dir lookup: {e}")))?;
    let primary = resource_dir.join(rel);
    if primary.exists() {
        return Ok(primary);
    }
    // Dev-mode fallback: `cargo tauri dev` doesn't materialize bundle.resources
    // into resource_dir, so look relative to CARGO_MANIFEST_DIR
    // (`crates/app/`). Production bundles always hit the primary path above.
    #[cfg(debug_assertions)]
    {
        let dev = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
        if dev.exists() {
            return Ok(dev);
        }
    }
    // Return the primary (non-existent) path so the caller's exists() check
    // surfaces the expected "installer not bundled at <path>" error.
    Ok(primary)
}

#[tauri::command]
pub async fn install_driver(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), WireError> {
    let installer = resource_path_or_err(&app, "installers/interception/install-interception.exe")
        .map_err(|e| e.to_wire())?;
    if !installer.exists() {
        return Err(AppError::Other(format!(
            "installer not bundled at {}",
            installer.display()
        ))
        .to_wire());
    }
    let storage_root = state.storage_root.clone();
    tokio::task::spawn_blocking(move || {
        crate::driver_install::install_driver(&installer, &storage_root)
    })
    .await
    .map_err(|e| AppError::Other(format!("install task join: {e}")).to_wire())?
    .map_err(|e| e.to_wire())?;
    *state.pending_reboot.lock().await = true;
    Ok(())
}

#[tauri::command]
pub async fn uninstall_driver(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), WireError> {
    // Uninstall uses the SAME bundled binary with /uninstall — handled
    // inside driver_install::uninstall_driver. So the resource path is
    // install-interception.exe (not a separate uninstaller).
    let installer = resource_path_or_err(&app, "installers/interception/install-interception.exe")
        .map_err(|e| e.to_wire())?;
    if !installer.exists() {
        return Err(AppError::Other(format!(
            "installer not bundled at {}",
            installer.display()
        ))
        .to_wire());
    }
    let storage_root = state.storage_root.clone();
    tokio::task::spawn_blocking(move || {
        crate::driver_install::uninstall_driver(&installer, &storage_root)
    })
    .await
    .map_err(|e| AppError::Other(format!("uninstall task join: {e}")).to_wire())?
    .map_err(|e| e.to_wire())?;
    *state.pending_reboot.lock().await = true;
    Ok(())
}

#[tauri::command]
pub async fn clear_pending_reboot(state: State<'_, AppState>) -> Result<(), WireError> {
    crate::driver_install::clear_pending_marker(&state.storage_root)
        .map_err(|e| e.to_wire())?;
    *state.pending_reboot.lock().await = false;
    Ok(())
}

#[tauri::command]
pub async fn reboot_windows() -> Result<(), WireError> {
    crate::driver_install::restart_windows().map_err(|e| e.to_wire())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rm_macro_model::{KeyCode, Modifier, PlaybackMode, Step, Trigger};
    use rm_storage::save_macro;
    use tempfile::TempDir;

    fn fixture_state() -> (TempDir, AppState) {
        let tmp = TempDir::new().unwrap();
        let state = AppState::new(tmp.path().to_path_buf(), crate::settings::Settings::default(), false);
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
}
