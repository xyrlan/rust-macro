use std::path::Path;
use std::sync::Arc;

use rm_driver::{Driver, DriverHub};
use rm_error::{AppError, Result};
use rm_macro_model::{KeyCode, Macro, Modifier, PlaybackMode, Trigger};
use rm_player::play;
use rm_recorder::start_recording;
use rm_storage::{delete_macro, load_all, save_macro};

use crate::stdio_driver::StdioDriver;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverKind {
    Stdio,
    #[cfg(feature = "interception")]
    Interception,
}

/// Record events from the selected driver source and save under `name`.
///
/// - `DriverKind::Stdio` reads JSONL events from stdin; exits when stdin
///   closes. Passthrough is off (the StdioDriver re-emits to stdout, so
///   passthrough would double-print).
/// - `DriverKind::Interception` opens an Interception context, captures real
///   keyboard/mouse events with passthrough ON (so the user sees their input
///   in the target app), and exits on Ctrl+C.
pub async fn cmd_record(root: &Path, name: &str, driver_kind: DriverKind) -> Result<()> {
    let (drv, passthrough): (Arc<dyn Driver>, bool) = match driver_kind {
        DriverKind::Stdio => (Arc::new(StdioDriver::new()), false),
        #[cfg(feature = "interception")]
        DriverKind::Interception => (Arc::new(open_interception()?), true),
    };
    let hub = DriverHub::start(drv);
    let handle = start_recording(hub, passthrough);

    let steps = match driver_kind {
        DriverKind::Stdio => handle.wait_for_close().await?,
        #[cfg(feature = "interception")]
        DriverKind::Interception => {
            eprintln!("recording... press Ctrl+C to stop");
            tokio::signal::ctrl_c()
                .await
                .map_err(|e| AppError::Other(format!("ctrl_c handler: {e}")))?;
            eprintln!("stopping...");
            handle.finish().await?
        }
    };

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

pub async fn cmd_play(root: &Path, name: &str, driver_kind: DriverKind) -> Result<()> {
    let macros = load_all(root)?;
    let mut m = macros
        .into_iter()
        .find(|m| m.name == name)
        .ok_or_else(|| AppError::MacroNotFound(name.into()))?;
    if matches!(m.playback, PlaybackMode::Loop | PlaybackMode::Toggle) {
        eprintln!(
            "note: macro playback is {:?}; CLI overrides to Once \
                   (no stop signal available)",
            m.playback
        );
        m.playback = PlaybackMode::Once;
    }
    let drv: Arc<dyn Driver> = match driver_kind {
        DriverKind::Stdio => Arc::new(StdioDriver::new()),
        #[cfg(feature = "interception")]
        DriverKind::Interception => Arc::new(open_interception()?),
    };
    let hub = DriverHub::start(drv);
    play(hub, m).wait().await
}

#[cfg(feature = "interception")]
fn open_interception() -> Result<rm_driver_interception::InterceptionDriver> {
    use rm_driver_interception::{detect_status, DriverStatus, InterceptionDriver};
    InterceptionDriver::new().map_err(|orig| match detect_status() {
        DriverStatus::NotInstalled => AppError::DriverNotInstalled,
        DriverStatus::InstalledNotRunning => AppError::DriverNotRunning,
        DriverStatus::Running => AppError::DriverIo(orig.to_string()),
    })
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

#[cfg(feature = "interception")]
pub fn cmd_driver_status() -> Result<()> {
    use rm_driver_interception::{detect_status, DriverStatus};
    match detect_status() {
        DriverStatus::Running => {
            println!("Interception driver: Running.");
        }
        DriverStatus::InstalledNotRunning => {
            println!("Interception driver: Installed but not running.");
            println!("A reboot may be required after installation.");
        }
        DriverStatus::NotInstalled => {
            println!("Interception driver: Not installed.");
            println!("Install from: https://github.com/oblitum/Interception/releases");
            println!("Run the installer as Administrator; a reboot is required.");
        }
    }
    Ok(())
}
