use std::path::Path;
use std::sync::Arc;

use rm_driver::Driver;
use rm_error::{AppError, Result};
use rm_macro_model::{KeyCode, Macro, Modifier, PlaybackMode, Trigger};
use rm_player::play;
use rm_recorder::start_recording;
use rm_storage::{delete_macro, load_all, save_macro};

use crate::stdio_driver::StdioDriver;

/// Record from stdin (JSONL of RawEvent). The recorder exits naturally when
/// stdin EOFs (the StdioDriver returns `DriverError::Closed`), so this just
/// awaits the task without ever sending a stop signal.
pub async fn cmd_record(root: &Path, name: &str) -> Result<()> {
    let drv: Arc<dyn Driver> = Arc::new(StdioDriver::new());
    let handle = start_recording(drv, false);
    let steps = handle.wait_for_close().await?;
    if steps.is_empty() {
        return Err(AppError::Other("no events recorded".into()));
    }
    let mut m = Macro::new(
        name,
        Trigger::Hotkey {
            key: KeyCode::F1,
            modifiers: vec![Modifier::Ctrl],
        },
        PlaybackMode::Once,
    );
    m.steps = steps;
    save_macro(root, &m)?;
    println!("saved {} ({})", m.name, m.id);
    Ok(())
}

pub async fn cmd_play(root: &Path, name: &str) -> Result<()> {
    let macros = load_all(root)?;
    let mut m = macros
        .into_iter()
        .find(|m| m.name == name)
        .ok_or_else(|| AppError::MacroNotFound(name.into()))?;
    // The CLI demo has no stop-hotkey or signal handler, so unbounded modes
    // would block the terminal forever. Override to Once with a note.
    if matches!(m.playback, PlaybackMode::Loop | PlaybackMode::Toggle) {
        eprintln!(
            "note: macro playback is {:?}; CLI overrides to Once \
                   (no stop signal available)",
            m.playback
        );
        m.playback = PlaybackMode::Once;
    }
    let drv: Arc<dyn Driver> = Arc::new(StdioDriver::new());
    play(drv, m).wait().await
}

pub fn cmd_list(root: &Path) -> Result<()> {
    for m in load_all(root)? {
        println!("{}  {}  steps={}", m.id, m.name, m.steps.len());
    }
    Ok(())
}

pub fn cmd_delete(root: &Path, name: &str) -> Result<()> {
    let macros = load_all(root)?;
    let id = macros
        .into_iter()
        .find(|m| m.name == name)
        .map(|m| m.id)
        .ok_or_else(|| AppError::MacroNotFound(name.into()))?;
    delete_macro(root, id)?;
    println!("deleted {name}");
    Ok(())
}
