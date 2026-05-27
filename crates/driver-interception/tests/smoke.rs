//! Smoke tests for `InterceptionDriver`. These require the Interception driver
//! to be installed on the host and `interception.dll` to be on PATH. Run with:
//!
//!     cargo test -p rm-driver-interception --features smoke
//!
//! Never runs in CI — the `smoke` feature is off by default.

#![cfg(feature = "smoke")]

use std::time::{Duration, Instant};

use rm_driver_interception::{detect_status, DriverStatus, InterceptionDriver};

#[test]
fn detect_status_returns_known_variant() {
    let s = detect_status();
    // We don't assert which variant — just that the call doesn't panic and
    // returns one of the three.
    match s {
        DriverStatus::NotInstalled
        | DriverStatus::InstalledNotRunning
        | DriverStatus::Running => {}
    }
    eprintln!("detected status: {:?}", s);
}

#[test]
fn open_then_drop_within_200ms() {
    if detect_status() != DriverStatus::Running {
        eprintln!("Interception not running; skipping open_then_drop");
        return;
    }
    let driver = InterceptionDriver::new().expect("Interception::new failed despite Running");
    let started = Instant::now();
    drop(driver);
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_millis(300),
        "drop took {:?}, expected < 300ms (100ms WAIT_SLICE + slack)",
        elapsed
    );
}
