//! Real-driver implementation of `rm_driver::Driver` backed by the Interception
//! kernel driver via the `kanata-interception` crate. Windows-only.
//!
//! See `docs/superpowers/specs/2026-05-26-rust-macro-plan-2b-real-driver-design.md`.

pub mod driver;
pub mod mouse;
pub mod scancode;
pub mod status;

pub use driver::InterceptionDriver;
pub use status::{detect_status, DriverStatus};

use rm_error::AppError;

/// Open an Interception context with full capture filters, mapping failure
/// to `AppError` via `detect_status()`. Use for **recording**.
pub fn open_with_status() -> Result<InterceptionDriver, AppError> {
    InterceptionDriver::new().map_err(|orig| match detect_status() {
        DriverStatus::NotInstalled => AppError::DriverNotInstalled,
        DriverStatus::InstalledNotRunning => AppError::DriverNotRunning,
        DriverStatus::Running => AppError::DriverIo(orig.to_string()),
    })
}

/// Like `open_with_status` but with persisted `ulExtraInformation` signatures
/// at the given path. The pump loads cached signatures on startup and writes
/// them back when new signatures are observed from hardware. Primes the
/// first-activation case in games that bypass Interception in-play — the
/// signature observed once (e.g., from desktop usage or a menu screen) is
/// then available across app restarts.
pub fn open_with_persisted_signatures(
    signature_path: std::path::PathBuf,
) -> Result<InterceptionDriver, AppError> {
    InterceptionDriver::new_with_persisted_signatures(signature_path).map_err(|orig| match detect_status() {
        DriverStatus::NotInstalled => AppError::DriverNotInstalled,
        DriverStatus::InstalledNotRunning => AppError::DriverNotRunning,
        DriverStatus::Running => AppError::DriverIo(orig.to_string()),
    })
}

/// Open an Interception context without capture filters (send-only), mapping
/// failure to `AppError` via `detect_status()`. Use for **playback** — the
/// context can inject events via `send()` but does not steal user input from
/// the OS, so keyboard and mouse remain usable during and after the macro.
pub fn open_send_only_with_status() -> Result<InterceptionDriver, AppError> {
    InterceptionDriver::new_send_only().map_err(|orig| match detect_status() {
        DriverStatus::NotInstalled => AppError::DriverNotInstalled,
        DriverStatus::InstalledNotRunning => AppError::DriverNotRunning,
        DriverStatus::Running => AppError::DriverIo(orig.to_string()),
    })
}

#[cfg(test)]
mod sanity {
    use rm_driver::RawEvent;
    use rm_macro_model::KeyCode;

    #[test]
    fn raw_event_constructible() {
        let _e = RawEvent::KeyDown { key: KeyCode::A };
    }
}
