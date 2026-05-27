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

#[cfg(test)]
mod sanity {
    use rm_driver::RawEvent;
    use rm_macro_model::KeyCode;

    #[test]
    fn raw_event_constructible() {
        let _e = RawEvent::KeyDown { key: KeyCode::A };
    }
}
