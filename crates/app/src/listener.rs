//! Persistent listener — runs from app boot to shutdown. Owns a single
//! filtered `Arc<DriverHub>` and subscribes for two purposes:
//!   1. **Passthrough forwarding**: every received event is re-sent via
//!      `hub.send(event)` so the OS keeps seeing user input. Runs unconditionally
//!      — even during recording. The recorder shares this hub (does NOT open a
//!      second Interception context, which would silently starve since the
//!      kernel routes each stroke to one context only). To prevent the recording
//!      stop key from leaking to apps, `suppress_key` tells the passthrough to
//!      drop matching KeyDown/KeyUp while a recording is active.
//!   2. **Hotkey dispatch**: rm-hotkey's `start_listener` watches for trigger
//!      matches and emits `HotkeyHit` on the channel. The dispatcher task
//!      receives those and calls `play_macro_internal` directly. Skipped when
//!      a recording or playback is active (checks `state.recording`/`state.active`).

use std::sync::Arc;

use rm_driver::{DriverHub, RawEvent};
use rm_hotkey::{start_listener as start_hotkey_listener, HotkeyHit, HotkeyRegistry, ListenerHandle};
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use crate::state::AppState;

pub struct ActiveListener {
    pub hub: Arc<DriverHub>,
    pub registry: HotkeyRegistry,
    /// When `Some(k)`, the passthrough drops KeyDown/KeyUp for `k` so it
    /// doesn't reach the OS. Set to the recording stop key while a recording
    /// is active; cleared by the supervisor on completion.
    pub suppress_key: Arc<std::sync::Mutex<Option<rm_macro_model::KeyCode>>>,
    /// Configured stop key (default F10) — when this key is pressed while a
    /// playback is active, the listener fires that playback's stop signal and
    /// suppresses the keystroke so it doesn't leak to apps. Mirrored from
    /// `settings.stop_key`; refreshed by `save_settings` when the user changes
    /// the setting.
    pub stop_key: Arc<std::sync::Mutex<rm_macro_model::KeyCode>>,
    pub hotkey_handle: Option<ListenerHandle>,
    pub passthrough_stop_tx: Option<oneshot::Sender<()>>,
    pub dispatcher_stop_tx: Option<oneshot::Sender<()>>,
}

/// Open Interception (with filters), spawn passthrough + dispatcher tasks.
/// Returns the assembled `ActiveListener` for storage in AppState.
pub fn start(
    app: AppHandle,
    registry: HotkeyRegistry,
    initial_stop_key: rm_macro_model::KeyCode,
) -> Result<ActiveListener, rm_error::AppError> {
    let drv: Arc<dyn rm_driver::Driver> = Arc::new(
        rm_driver_interception::open_with_status()?,
    );
    let hub = DriverHub::start(drv);

    let suppress_key: Arc<std::sync::Mutex<Option<rm_macro_model::KeyCode>>> =
        Arc::new(std::sync::Mutex::new(None));
    let pt_suppress = suppress_key.clone();
    let stop_key: Arc<std::sync::Mutex<rm_macro_model::KeyCode>> =
        Arc::new(std::sync::Mutex::new(initial_stop_key));
    let pt_stop_key = stop_key.clone();
    let pt_app = app.clone();

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
                        // Suppress the recording stop key so the user's F10
                        // (or whatever they configured) doesn't leak to apps
                        // while they're recording. Read+release the std Mutex
                        // before any await.
                        let suppress = *pt_suppress.lock().unwrap();
                        if let Some(sk) = suppress {
                            if let RawEvent::KeyDown { key } | RawEvent::KeyUp { key } = event {
                                if key == sk { continue; }
                            }
                        }
                        // Emergency stop: stop_key during active playback
                        // kills the playback and suppresses the keystroke
                        // (otherwise the user can't recover from a runaway
                        // Loop macro without killing the process).
                        if let RawEvent::KeyDown { key } = event {
                            let configured = *pt_stop_key.lock().unwrap();
                            if key == configured {
                                if let Some(s) = pt_app.try_state::<AppState>() {
                                    let mut active = s.active.lock().await;
                                    if let Some(ap) = active.as_mut() {
                                        if let Some(tx) = ap.stop_tx.take() {
                                            let _ = tx.send(());
                                            debug!("listener: emergency stop fired via stop_key");
                                        }
                                        continue;
                                    }
                                }
                            }
                        }
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

    // Dispatcher — pumps HotkeyHit and calls play_macro_internal via the AppHandle.
    let (disp_stop_tx, mut disp_stop_rx) = oneshot::channel();
    let app_for_disp = app;
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
        suppress_key,
        stop_key,
        hotkey_handle: Some(hotkey_handle),
        passthrough_stop_tx: Some(pt_stop_tx),
        dispatcher_stop_tx: Some(disp_stop_tx),
    })
}

/// Direct invocation of `play_macro_internal` bypassing the `#[tauri::command]`
/// wrapper. Same lookup → guard → spawn-supervisor sequence.
async fn dispatch_play(app: &AppHandle, id: uuid::Uuid) -> Result<(), rm_error::AppError> {
    let state = app
        .try_state::<AppState>()
        .ok_or_else(|| rm_error::AppError::Other("dispatcher: AppState missing".into()))?;
    crate::commands::play_macro_internal(app.clone(), &state, id).await
}
