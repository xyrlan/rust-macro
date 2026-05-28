//! Recording supervisor — wraps `rm-recorder` with the app-level lifecycle:
//! per-session DriverHub, ActiveRecording slot cleanup, `recording_finished`
//! event emission.

use rm_recorder::RecordingHandle;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

use crate::dto::StepDto;
use crate::state::AppState;

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

/// Spawn the supervisor task. It owns the `RecordingHandle`. When
/// `external_stop_rx` fires OR the recorder ends naturally (e.g. F10), the
/// supervisor:
///   1. Collects steps via `handle.run_with_stop(external_stop_rx)`.
///   2. Clears the ActiveRecording slot.
///   3. Clears the listener's `suppress_key` so the stop key reaches apps again.
///   4. Emits `recording_finished` with outcome.
///
/// Note: the hub itself is shared with the persistent listener and is NOT
/// released here — Interception stays open for the listener's lifetime.
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

        if let Some(s) = app.try_state::<AppState>() {
            let mut recording = s.recording.lock().await;
            *recording = None;
            drop(recording);

            #[cfg(feature = "interception")]
            if let Some(l) = s.listener.lock().await.as_ref() {
                *l.suppress_key.lock().unwrap() = None;
            }
        } else {
            tracing::error!(
                "recording supervisor: AppState not registered — recording slot may be stuck"
            );
        }

        let _ = app.emit(
            "recording_finished",
            RecordingFinishedEvent { outcome },
        );
    });
}
