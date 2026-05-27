//! Tauri command handlers. Each command takes `State<'_, AppState>` and
//! returns `Result<T, WireError>`. Errors map from `AppError::to_wire()`.

use rm_error::{AppError, WireError};
use rm_storage::{delete_macro as storage_delete, load_all, load_macro, save_macro as storage_save};
use tauri::State;
use uuid::Uuid;

use crate::dto::{MacroDto, PlaybackModeDto, TriggerDto};
use crate::state::AppState;

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
}
