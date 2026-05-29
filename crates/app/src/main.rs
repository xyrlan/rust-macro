//! Entry point for the rust-macro Tauri GUI. Commands and state are wired in
//! later tasks of Plan 3a; this initial revision only verifies that the Tauri
//! runtime starts and shows a window.

// Hide the Windows console when launching the release binary; keep it for
// debug so println!/tracing output is visible during development.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod commands;
mod dto;
#[cfg(feature = "interception")]
mod listener;
mod driver_install;
mod recording;
mod settings;
mod state;

use std::path::PathBuf;

use state::AppState;

fn read_pending_reboot(storage_root: &std::path::Path) -> bool {
    storage_root.join(".driver-install-pending").exists()
}

fn main() {
    // Raise this process's timer resolution to 1ms so tokio::time::sleep
    // honors sub-15ms durations. Windows default is ~15.6ms (system tick) —
    // without this, the mouse-stream playback (1ms-cadence chunks) silently
    // runs ~15x slower than intended, distorting recorded motion in-game.
    // Per-process timer resolution is automatically released on process exit
    // (Windows 10+), so no timeEndPeriod call is needed.
    #[cfg(windows)]
    unsafe {
        windows_sys::Win32::Media::timeBeginPeriod(1);
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

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

    let pending_reboot = read_pending_reboot(&storage_root);

    let builder = tauri::Builder::default()
        .manage(AppState::new(storage_root, settings, pending_reboot))
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
            commands::driver_status,
            commands::install_driver,
            commands::uninstall_driver,
            commands::clear_pending_reboot,
            commands::reboot_windows,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // If a recording is active, fire its stop signal so the
                // supervisor finalizes cleanly (drops the Interception
                // context). We don't block close on completion — the
                // OS will reap any orphaned task on exit, and Interception
                // releases on context drop.
                use tauri::Manager;
                let app_handle = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(s) = app_handle.try_state::<AppState>() {
                        let mut recording = s.recording.lock().await;
                        if let Some(ar) = recording.as_mut() {
                            if let Some(tx) = ar.stop_tx.take() {
                                let _ = tx.send(());
                            }
                        }
                    }
                });
            }
        });

    #[cfg(feature = "interception")]
    let builder = builder.setup(|app| {
        use tauri::Manager;
        let app_handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            // Build the registry from all macros currently on disk. Skip
            // unsafe triggers (bare Left/Right/Middle click) so a pre-existing
            // disaster macro can't auto-fire on boot — the user has to edit
            // the trigger or delete the macro before it'll bind again.
            let registry = rm_hotkey::HotkeyRegistry::new();
            if let Some(state) = app_handle.try_state::<AppState>() {
                if let Ok(macros) = rm_storage::load_all(&state.storage_root) {
                    for m in macros {
                        if !m.trigger.is_safe() {
                            tracing::warn!(
                                macro_id = %m.id,
                                name = %m.name,
                                "skipping unsafe trigger (bare Left/Right/Middle click) — not bound"
                            );
                            continue;
                        }
                        registry.bind(m.id, m.trigger).await;
                    }
                }
            }

            // Snapshot the current stop_key so the listener can use it for
            // emergency-stop on playback (the listener can refresh this via
            // save_settings if the user changes it).
            let initial_stop_key = if let Some(state) = app_handle.try_state::<AppState>() {
                state.settings.lock().await.stop_key
            } else {
                rm_macro_model::KeyCode::F10
            };

            // Start the listener.
            match listener::start(app_handle.clone(), registry, initial_stop_key) {
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
}
