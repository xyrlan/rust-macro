//! Entry point for the rust-macro Tauri GUI. Commands and state are wired in
//! later tasks of Plan 3a; this initial revision only verifies that the Tauri
//! runtime starts and shows a window.

// Hide the Windows console when launching the release binary; keep it for
// debug so println!/tracing output is visible during development.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
