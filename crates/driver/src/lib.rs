use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod mock;

pub use rm_macro_model::{KeyCode, MouseButton, Point};

/// One low-level event from / to the input device layer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RawEvent {
    KeyDown {
        key: KeyCode,
    },
    KeyUp {
        key: KeyCode,
    },
    MouseDown {
        button: MouseButton,
    },
    MouseUp {
        button: MouseButton,
    },
    /// Mouse motion. Plan 1 / mock: position is informational only.
    /// Plan 2: real driver returns relative deltas; absolute is converted upstream.
    MouseMove {
        dx: i32,
        dy: i32,
    },
    MouseWheel {
        delta: i32,
    },
}

#[derive(thiserror::Error, Debug)]
pub enum DriverError {
    #[error("driver closed")]
    Closed,

    #[error("driver i/o failure: {0}")]
    Io(String),

    #[error("driver not available: {0}")]
    Unavailable(String),
}

/// Abstracts the underlying input device layer (real Interception driver in Plan 2,
/// `MockDriver` in tests, `StdioDriver` in the CLI).
///
/// Implementations must be safely shareable across tasks (`Send + Sync`).
#[async_trait]
pub trait Driver: Send + Sync {
    /// Emit an event toward the OS (synthesized input).
    async fn send(&self, event: RawEvent) -> Result<(), DriverError>;

    /// Wait for the next event from the underlying source. Returns
    /// `Err(DriverError::Closed)` when the source is shut down.
    async fn recv(&self) -> Result<RawEvent, DriverError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_event_keydown_roundtrip() {
        let e = RawEvent::KeyDown { key: KeyCode::W };
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains("\"kind\":\"key_down\""));
        let back: RawEvent = serde_json::from_str(&j).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn raw_event_mouse_move_roundtrip() {
        let e = RawEvent::MouseMove { dx: 5, dy: -3 };
        let j = serde_json::to_string(&e).unwrap();
        let back: RawEvent = serde_json::from_str(&j).unwrap();
        assert_eq!(e, back);
    }

    #[test]
    fn raw_event_wheel_roundtrip() {
        let e = RawEvent::MouseWheel { delta: 120 };
        let j = serde_json::to_string(&e).unwrap();
        let back: RawEvent = serde_json::from_str(&j).unwrap();
        assert_eq!(e, back);
    }
}
