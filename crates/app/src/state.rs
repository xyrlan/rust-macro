//! Runtime state for the Tauri app. `listener` owns the single shared
//! Interception context (with capture filters). Both recording and playback
//! borrow this hub — see `commands::ensure_hub` for the reasoning. `active`
//! enforces one-playback-at-a-time; `recording` holds the per-session
//! supervisor stop signal.

use std::path::PathBuf;
use std::sync::Arc;

use rm_driver::DriverHub;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Initialised once at startup in `main`. All Tauri commands receive a
/// `State<'_, AppState>` parameter.
pub struct AppState {
    pub storage_root: PathBuf,
    pub active: Mutex<Option<ActivePlayback>>,
    pub recording: Mutex<Option<ActiveRecording>>,
    pub settings: Mutex<crate::settings::Settings>,
    pub pending_reboot: Mutex<bool>,
    #[cfg(feature = "interception")]
    pub listener: Mutex<Option<crate::listener::ActiveListener>>,
}

pub struct ActivePlayback {
    pub macro_id: Uuid,
    /// User-initiated stop signal. `Some` while the playback is running;
    /// `stop_playback` takes the sender out and fires it.
    pub stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

/// Per-session recording state. `session_hub` holds a clone of the listener's
/// hub for the duration of the recording (kept around for symmetry with
/// `ActivePlayback`; the underlying Interception context is owned by the
/// listener and outlives the recording).
pub struct ActiveRecording {
    pub stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    pub session_hub: Arc<DriverHub>,
}

impl AppState {
    pub fn new(storage_root: PathBuf, settings: crate::settings::Settings, pending_reboot: bool) -> Self {
        Self {
            storage_root,
            active: Mutex::new(None),
            recording: Mutex::new(None),
            settings: Mutex::new(settings),
            pending_reboot: Mutex::new(pending_reboot),
            #[cfg(feature = "interception")]
            listener: Mutex::new(None),
        }
    }
}
