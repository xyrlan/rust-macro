//! Entry point for the rust-macro Tauri GUI. Commands and state are wired in
//! later tasks of Plan 3a; this initial revision only verifies that the Tauri
//! runtime starts and shows a window.

// Hide the Windows console when launching the release binary; keep it for
// debug so println!/tracing output is visible during development.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod commands;
mod dto;
mod recording;
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

    tauri::Builder::default()
        .manage(AppState::new(storage_root))
        .invoke_handler(tauri::generate_handler![
            commands::load_macros,
            commands::delete_macro,
            commands::update_macro_metadata,
            commands::load_macro_steps,
            commands::play_macro,
            commands::stop_playback,
            commands::start_recording,
            commands::stop_recording,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
