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
