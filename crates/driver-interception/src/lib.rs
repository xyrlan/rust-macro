//! Real-driver implementation of `rm_driver::Driver` backed by the Interception
//! kernel driver via the `kanata-interception` crate. Windows-only.
//!
//! See `docs/superpowers/specs/2026-05-26-rust-macro-plan-2b-real-driver-design.md`.

pub mod driver;
pub mod mouse;
pub mod scancode;
pub mod status;
