//! Runtime state for the Tauri app. `DriverHub` is created lazily on the
//! first `play_macro` call; `active` enforces one-playback-at-a-time;
//! `recording` owns the per-session Interception hub for in-app recording.

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

/// Per-session recording state. Owns its own DriverHub (NOT the lazy playback
/// hub) so the Interception context can be released cleanly when the
/// recording ends — see Plan 3b's "Backend lifecycle" notes.
pub struct ActiveRecording {
    pub stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    pub session_hub: Arc<DriverHub>,
}

impl AppState {
    pub fn new(storage_root: PathBuf, settings: crate::settings::Settings, pending_reboot: bool) -> Self {
        Self {
            storage_root,
            driver_hub: Mutex::new(None),
            active: Mutex::new(None),
            recording: Mutex::new(None),
            settings: Mutex::new(settings),
            pending_reboot: Mutex::new(pending_reboot),
            #[cfg(feature = "interception")]
            listener: Mutex::new(None),
        }
    }
}
