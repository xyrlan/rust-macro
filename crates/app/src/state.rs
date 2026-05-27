//! Runtime state for the Tauri app. `DriverHub` is created lazily on the
//! first `play_macro` call; `active` enforces one-playback-at-a-time.

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
}

pub struct ActivePlayback {
    pub macro_id: Uuid,
    /// User-initiated stop signal. `Some` while the playback is running;
    /// `stop_playback` takes the sender out and fires it. The supervisor
    /// task spawned by `play_macro` observes this via a relay and forwards
    /// it to `PlaybackHandle::run_with_stop`. Once fired, the supervisor
    /// clears the active slot and emits `playback_finished`.
    pub stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl AppState {
    pub fn new(storage_root: PathBuf) -> Self {
        Self {
            storage_root,
            driver_hub: Mutex::new(None),
            active: Mutex::new(None),
        }
    }
}
