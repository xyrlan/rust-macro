//! Entry point for the rust-macro Tauri GUI. Commands and state are wired in
//! later tasks of Plan 3a; this initial revision only verifies that the Tauri
//! runtime starts and shows a window.

// Hide the Windows console when launching the release binary; keep it for
// debug so println!/tracing output is visible during development.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod commands;
mod dto;
mod recording;
mod settings;
mod state;

use std::path::PathBuf;

use state::AppState;

fn main() {
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

    tauri::Builder::default()
        .manage(AppState::new(storage_root, settings))
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
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
