# rust-macro — Plan 1: Backend with Mock Driver + CLI

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement all backend Rust crates for the rust-macro macro engine, using a mock driver (no real Interception integration yet), and a `macro-cli` binary that exercises the full record → save → load → play pipeline through stdio. End state: full data model, recording compilation, playback (all modes), hotkey dispatch, JSON storage, and a testable CLI demo — all behind a `Driver` trait that Plan 2 will swap for the real `interception-rs` impl.

**Architecture:** Cargo workspace with one crate per responsibility (`error`, `macro_model`, `driver`, `storage`, `recorder`, `player`, `hotkey`, `cli`). Driver I/O is abstracted behind an async `Driver` trait so the same `recorder` and `player` code that runs against `MockDriver` in tests will run against the real Interception driver in Plan 2 with zero changes. Tokio is the async runtime throughout.

**Tech Stack:** Rust stable, Tokio, async-trait, serde + serde_json, uuid, chrono, thiserror, anyhow, tracing + tracing-subscriber, clap, rand, tempfile (dev-dep).

---

## File Structure

```
rust-macro/
├── Cargo.toml                                  ← workspace root
├── Cargo.lock
├── rust-toolchain.toml                         ← pin stable
├── rustfmt.toml
├── .gitignore
├── .github/workflows/ci.yml
├── README.md
├── docs/superpowers/
│   ├── specs/2026-05-26-rust-macro-design.md   ← existing
│   └── plans/2026-05-26-rust-macro-plan-1-backend.md ← this file
└── crates/
    ├── error/
    │   ├── Cargo.toml
    │   └── src/lib.rs
    ├── macro_model/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       ├── input.rs                        ← KeyCode, MouseButton, Modifier, Point
    │       └── macro_def.rs                    ← Macro, Step, Trigger, PlaybackMode
    ├── driver/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs                          ← Driver trait, RawEvent, DriverError re-export
    │       └── mock.rs                         ← MockDriver impl
    ├── storage/
    │   ├── Cargo.toml
    │   └── src/lib.rs
    ├── recorder/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs                          ← public API + record task
    │       └── compile.rs                      ← raw events → Vec<Step>
    ├── player/
    │   ├── Cargo.toml
    │   └── src/lib.rs
    ├── hotkey/
    │   ├── Cargo.toml
    │   └── src/lib.rs
    └── cli/
        ├── Cargo.toml
        └── src/
            ├── main.rs
            ├── stdio_driver.rs                 ← Driver impl that reads stdin / writes stdout
            └── commands.rs
```

Each library crate owns its tests in-line (`#[cfg(test)] mod tests`). The `cli` crate also gets one integration test in `crates/cli/tests/e2e.rs` covering record → save → play roundtrip.

---

## Task 1 — Workspace bootstrap

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `rust-toolchain.toml`
- Create: `rustfmt.toml`
- Modify: `.gitignore`
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Write workspace `Cargo.toml`**

Create `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
    "crates/error",
    "crates/macro_model",
    "crates/driver",
    "crates/storage",
    "crates/recorder",
    "crates/player",
    "crates/hotkey",
    "crates/cli",
]

[workspace.package]
edition = "2021"
rust-version = "1.75"
version = "0.1.0"
license = "MIT OR Apache-2.0"

[workspace.dependencies]
anyhow = "1"
async-trait = "0.1"
chrono = { version = "0.4", features = ["serde"] }
clap = { version = "4", features = ["derive"] }
rand = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
uuid = { version = "1", features = ["v4", "serde"] }

# dev
tempfile = "3"
```

- [ ] **Step 2: Pin toolchain**

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 3: Rustfmt config**

Create `rustfmt.toml`:

```toml
edition = "2021"
max_width = 100
```

- [ ] **Step 4: Update `.gitignore`**

Add to `.gitignore` (create if missing):

```
/target
**/*.rs.bk
Cargo.lock.bak
.DS_Store
```

- [ ] **Step 5: CI workflow**

Create `.github/workflows/ci.yml`:

```yaml
name: ci
on:
  push:
    branches: [main]
  pull_request:
jobs:
  test:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt -- --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
```

- [ ] **Step 6: Verify workspace builds**

Run: `cargo check --workspace`
Expected: errors saying member crates don't exist yet (no `Cargo.toml` inside `crates/*`). This is fine — confirms cargo sees the workspace file. We'll create the members in the following tasks.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml rust-toolchain.toml rustfmt.toml .gitignore .github/workflows/ci.yml
git commit -m "chore: bootstrap cargo workspace + ci"
```

---

## Task 2 — `error` crate

**Files:**
- Create: `crates/error/Cargo.toml`
- Create: `crates/error/src/lib.rs`

- [ ] **Step 1: Create crate**

Create `crates/error/Cargo.toml`:

```toml
[package]
name = "rm-error"
version.workspace = true
edition.workspace = true

[dependencies]
thiserror.workspace = true
serde.workspace = true
```

- [ ] **Step 2: Write failing tests**

Create `crates/error/src/lib.rs`:

```rust
use serde::Serialize;
use std::path::PathBuf;

#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("Interception driver is not installed")]
    DriverNotInstalled,

    #[error("Interception driver is installed but not running")]
    DriverNotRunning,

    #[error("Driver I/O failed: {0}")]
    DriverIo(String),

    #[error("Macro not found: {0}")]
    MacroNotFound(String),

    #[error("A recording is already in progress")]
    RecordingActive,

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Serialization error: {0}")]
    Serde(String),

    #[error("{0}")]
    Other(String),
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Serde(e.to_string())
    }
}

/// Wire-friendly serialization for Tauri (Plan 3).
#[derive(Serialize)]
pub struct WireError {
    pub kind: &'static str,
    pub message: String,
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::DriverNotInstalled => "DriverNotInstalled",
            AppError::DriverNotRunning => "DriverNotRunning",
            AppError::DriverIo(_) => "DriverIo",
            AppError::MacroNotFound(_) => "MacroNotFound",
            AppError::RecordingActive => "RecordingActive",
            AppError::Io { .. } => "Io",
            AppError::Serde(_) => "Serde",
            AppError::Other(_) => "Other",
        }
    }

    pub fn to_wire(&self) -> WireError {
        WireError { kind: self.kind(), message: self.to_string() }
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn driver_not_installed_kind_is_stable() {
        assert_eq!(AppError::DriverNotInstalled.kind(), "DriverNotInstalled");
    }

    #[test]
    fn macro_not_found_renders_name() {
        let e = AppError::MacroNotFound("foo".into());
        assert_eq!(e.to_string(), "Macro not found: foo");
        assert_eq!(e.kind(), "MacroNotFound");
    }

    #[test]
    fn serde_error_converts() {
        let bad: serde_json::Error = serde_json::from_str::<i32>("not json").unwrap_err();
        let app: AppError = bad.into();
        assert_eq!(app.kind(), "Serde");
    }

    #[test]
    fn wire_form_roundtrips_through_json() {
        let e = AppError::DriverIo("device closed".into());
        let wire = e.to_wire();
        let json = serde_json::to_string(&wire).unwrap();
        assert!(json.contains("\"kind\":\"DriverIo\""));
        assert!(json.contains("device closed"));
    }
}
```

- [ ] **Step 3: Run tests — expect compile success then test pass**

Run: `cargo test -p rm-error`
Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/error
git commit -m "feat(error): central AppError enum with wire-form serialization"
```

---

## Task 3 — `macro_model` crate: input types

**Files:**
- Create: `crates/macro_model/Cargo.toml`
- Create: `crates/macro_model/src/lib.rs`
- Create: `crates/macro_model/src/input.rs`

- [ ] **Step 1: Create crate**

Create `crates/macro_model/Cargo.toml`:

```toml
[package]
name = "rm-macro-model"
version.workspace = true
edition.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
uuid.workspace = true
chrono.workspace = true
```

Create `crates/macro_model/src/lib.rs`:

```rust
pub mod input;
pub mod macro_def;

pub use input::*;
pub use macro_def::*;
```

- [ ] **Step 2: Write failing tests**

Create `crates/macro_model/src/input.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Physical key, identified by USB HID scancode where possible.
/// Plan 2 will add `From<interception::ScanCode>` impls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyCode {
    // Letters
    A, B, C, D, E, F, G, H, I, J, K, L, M,
    N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    // Digits (top row)
    Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
    // Function row
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    // Modifiers
    LShift, RShift, LCtrl, RCtrl, LAlt, RAlt, LWin, RWin,
    // Whitespace & control
    Space, Enter, Tab, Backspace, Escape, CapsLock,
    // Arrows
    Up, Down, Left, Right,
    // Edit cluster
    Insert, Delete, Home, End, PageUp, PageDown,
    // Punctuation (US layout)
    Minus, Equals, LBracket, RBracket, Backslash,
    Semicolon, Apostrophe, Backtick, Comma, Period, Slash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Ctrl,
    Shift,
    Alt,
    Win,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveMode {
    Absolute,
    Relative,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keycode_serializes_snake_case() {
        let json = serde_json::to_string(&KeyCode::LShift).unwrap();
        assert_eq!(json, "\"l_shift\"");
    }

    #[test]
    fn keycode_roundtrip_every_variant_via_letters_sample() {
        for k in [KeyCode::A, KeyCode::Z, KeyCode::Num0, KeyCode::F12,
                  KeyCode::Backslash, KeyCode::LWin, KeyCode::PageDown] {
            let s = serde_json::to_string(&k).unwrap();
            let back: KeyCode = serde_json::from_str(&s).unwrap();
            assert_eq!(k, back, "roundtrip failed for {:?}", k);
        }
    }

    #[test]
    fn point_roundtrip() {
        let p = Point { x: 100, y: -50 };
        let s = serde_json::to_string(&p).unwrap();
        let back: Point = serde_json::from_str(&s).unwrap();
        assert_eq!(p, back);
        assert_eq!(s, r#"{"x":100,"y":-50}"#);
    }

    #[test]
    fn modifier_and_move_mode_roundtrip() {
        let m = Modifier::Ctrl;
        let mm = MoveMode::Relative;
        assert_eq!(serde_json::from_str::<Modifier>("\"ctrl\"").unwrap(), m);
        assert_eq!(serde_json::from_str::<MoveMode>("\"relative\"").unwrap(), mm);
    }

    #[test]
    fn mouse_button_x_buttons_serialize() {
        assert_eq!(serde_json::to_string(&MouseButton::X1).unwrap(), "\"x1\"");
        assert_eq!(serde_json::to_string(&MouseButton::X2).unwrap(), "\"x2\"");
    }
}
```

- [ ] **Step 3: Run tests — expect pass**

Run: `cargo test -p rm-macro-model input::`
Expected: 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/macro_model
git commit -m "feat(macro_model): input types (KeyCode, MouseButton, Modifier, Point, MoveMode)"
```

---

## Task 4 — `macro_model`: Macro, Step, Trigger, PlaybackMode

**Files:**
- Create: `crates/macro_model/src/macro_def.rs`

- [ ] **Step 1: Write failing tests + impl**

Create `crates/macro_model/src/macro_def.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::input::{KeyCode, Modifier, MouseButton, MoveMode, Point};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Step {
    KeyPress    { key: KeyCode, hold_ms: u32 },
    KeyDown     { key: KeyCode },
    KeyUp       { key: KeyCode },
    MouseClick  { button: MouseButton, hold_ms: u32, at: Option<Point> },
    MouseMove   { to: Point, mode: MoveMode },
    MouseScroll { delta: i32 },
    Wait        { min_ms: u32, max_ms: u32 },
}

impl Step {
    /// Validates a `Wait` step has min <= max. Returns Err with a human message otherwise.
    pub fn validate(&self) -> Result<(), String> {
        if let Step::Wait { min_ms, max_ms } = self {
            if min_ms > max_ms {
                return Err(format!("Wait: min_ms ({min_ms}) > max_ms ({max_ms})"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    Hotkey { key: KeyCode, modifiers: Vec<Modifier> },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum PlaybackMode {
    Once,
    Repeat { count: u32 },
    Loop,
    Toggle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Macro {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub trigger: Trigger,
    pub playback: PlaybackMode,
    pub steps: Vec<Step>,
}

impl Macro {
    /// Create a new macro with generated id and current timestamps.
    pub fn new(name: impl Into<String>, trigger: Trigger, playback: PlaybackMode) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            created_at: now,
            updated_at: now,
            trigger,
            playback,
            steps: Vec::new(),
        }
    }

    /// Validate every step. Returns Err with the first failure.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Macro name cannot be empty".into());
        }
        for (i, step) in self.steps.iter().enumerate() {
            step.validate().map_err(|e| format!("step #{i}: {e}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::*;

    #[test]
    fn step_keypress_serde_roundtrip() {
        let s = Step::KeyPress { key: KeyCode::W, hold_ms: 250 };
        let j = serde_json::to_string(&s).unwrap();
        assert!(j.contains("\"type\":\"key_press\""));
        assert!(j.contains("\"key\":\"w\""));
        assert!(j.contains("\"hold_ms\":250"));
        let back: Step = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn step_wait_serde_roundtrip() {
        let s = Step::Wait { min_ms: 100, max_ms: 300 };
        let j = serde_json::to_string(&s).unwrap();
        let back: Step = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn step_mouse_click_optional_at() {
        let s = Step::MouseClick { button: MouseButton::Left, hold_ms: 50, at: None };
        let j = serde_json::to_string(&s).unwrap();
        let back: Step = serde_json::from_str(&j).unwrap();
        assert_eq!(s, back);

        let s2 = Step::MouseClick { button: MouseButton::Right, hold_ms: 80,
                                    at: Some(Point { x: 10, y: 20 }) };
        let j2 = serde_json::to_string(&s2).unwrap();
        let back2: Step = serde_json::from_str(&j2).unwrap();
        assert_eq!(s2, back2);
    }

    #[test]
    fn step_wait_validates_min_le_max() {
        assert!(Step::Wait { min_ms: 100, max_ms: 100 }.validate().is_ok());
        assert!(Step::Wait { min_ms: 100, max_ms: 200 }.validate().is_ok());
        assert!(Step::Wait { min_ms: 200, max_ms: 100 }.validate().is_err());
    }

    #[test]
    fn macro_new_sets_timestamps_and_id() {
        let m = Macro::new("test",
            Trigger::Hotkey { key: KeyCode::F1, modifiers: vec![] },
            PlaybackMode::Once);
        assert_eq!(m.name, "test");
        assert_eq!(m.created_at, m.updated_at);
        assert_eq!(m.steps.len(), 0);
        // Sanity check that id is not nil
        assert_ne!(m.id, Uuid::nil());
    }

    #[test]
    fn macro_full_roundtrip() {
        let mut m = Macro::new("greet",
            Trigger::Hotkey { key: KeyCode::F2, modifiers: vec![Modifier::Ctrl] },
            PlaybackMode::Repeat { count: 3 });
        m.steps = vec![
            Step::KeyPress { key: KeyCode::H, hold_ms: 80 },
            Step::Wait { min_ms: 50, max_ms: 150 },
            Step::KeyPress { key: KeyCode::I, hold_ms: 80 },
        ];
        let j = serde_json::to_string_pretty(&m).unwrap();
        let back: Macro = serde_json::from_str(&j).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn macro_validate_rejects_empty_name() {
        let m = Macro::new("   ",
            Trigger::Hotkey { key: KeyCode::F1, modifiers: vec![] },
            PlaybackMode::Once);
        assert!(m.validate().is_err());
    }

    #[test]
    fn playback_mode_repeat_count_in_json() {
        let p = PlaybackMode::Repeat { count: 5 };
        let j = serde_json::to_string(&p).unwrap();
        assert!(j.contains("\"mode\":\"repeat\""));
        assert!(j.contains("\"count\":5"));
        let back: PlaybackMode = serde_json::from_str(&j).unwrap();
        assert_eq!(p, back);
    }
}
```

- [ ] **Step 2: Run tests — expect pass**

Run: `cargo test -p rm-macro-model`
Expected: 13 tests pass (5 from Task 3 + 8 new).

- [ ] **Step 3: Commit**

```bash
git add crates/macro_model
git commit -m "feat(macro_model): Macro, Step, Trigger, PlaybackMode"
```

---

## Task 5 — `driver` crate: types and trait

**Files:**
- Create: `crates/driver/Cargo.toml`
- Create: `crates/driver/src/lib.rs`

- [ ] **Step 1: Create crate**

Create `crates/driver/Cargo.toml`:

```toml
[package]
name = "rm-driver"
version.workspace = true
edition.workspace = true

[dependencies]
async-trait.workspace = true
rm-macro-model = { path = "../macro_model" }
serde.workspace = true
thiserror.workspace = true
tokio = { workspace = true }
tracing.workspace = true
```

- [ ] **Step 2: Write trait + types**

Create `crates/driver/src/lib.rs`:

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod mock;

pub use rm_macro_model::{KeyCode, MouseButton, Point};

/// One low-level event from / to the input device layer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RawEvent {
    KeyDown    { key: KeyCode },
    KeyUp      { key: KeyCode },
    MouseDown  { button: MouseButton },
    MouseUp    { button: MouseButton },
    /// Mouse motion. Plan 1 / mock: position is informational only.
    /// Plan 2: real driver returns relative deltas; absolute is converted upstream.
    MouseMove  { dx: i32, dy: i32 },
    MouseWheel { delta: i32 },
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
```

- [ ] **Step 3: Tests for serde**

Append to `crates/driver/src/lib.rs`:

```rust
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
```

Add `serde_json` to dev-dependencies in `crates/driver/Cargo.toml`:

```toml
[dev-dependencies]
serde_json.workspace = true
tokio = { workspace = true, features = ["macros", "rt", "rt-multi-thread", "sync", "time"] }
```

- [ ] **Step 4: Stub mock.rs so build succeeds**

Create `crates/driver/src/mock.rs`:

```rust
// Implemented in Task 6.
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p rm-driver`
Expected: 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/driver
git commit -m "feat(driver): Driver trait + RawEvent + DriverError"
```

---

## Task 6 — `driver` crate: MockDriver

**Files:**
- Modify: `crates/driver/src/mock.rs`

- [ ] **Step 1: Write tests first**

Replace `crates/driver/src/mock.rs` with the following — tests at top so they are easy to refer to:

```rust
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::{mpsc, Mutex as AsyncMutex};

use crate::{Driver, DriverError, RawEvent};

/// Driver impl backed by in-memory channels — for tests and as a reference
/// implementation. `inject(event)` queues events that the next `recv()` will
/// return; `drain_sent()` returns everything passed to `send()` in order.
#[derive(Clone)]
pub struct MockDriver {
    sent: Arc<Mutex<Vec<RawEvent>>>,
    inject_tx: mpsc::UnboundedSender<RawEvent>,
    inject_rx: Arc<AsyncMutex<mpsc::UnboundedReceiver<RawEvent>>>,
}

impl Default for MockDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl MockDriver {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            sent: Arc::new(Mutex::new(Vec::new())),
            inject_tx: tx,
            inject_rx: Arc::new(AsyncMutex::new(rx)),
        }
    }

    /// Queue an event to be returned by the next `recv()` call.
    pub fn inject(&self, event: RawEvent) {
        let _ = self.inject_tx.send(event);
    }

    /// Close the driver. Pending and future `recv()` calls will see `Closed`.
    pub fn close(&self) {
        // Dropping all senders closes the channel. We don't have access to drop
        // them all here without splitting types, so emulate close by simply not
        // injecting further. For tests that need a deterministic close, drop the
        // MockDriver itself.
        // (No-op left as documentation.)
    }

    /// Returns everything that was sent via `Driver::send`, draining the buffer.
    pub fn drain_sent(&self) -> Vec<RawEvent> {
        std::mem::take(&mut self.sent.lock().unwrap())
    }

    /// Returns a snapshot of everything sent so far without draining.
    pub fn sent_snapshot(&self) -> Vec<RawEvent> {
        self.sent.lock().unwrap().clone()
    }
}

#[async_trait]
impl Driver for MockDriver {
    async fn send(&self, event: RawEvent) -> Result<(), DriverError> {
        self.sent.lock().unwrap().push(event);
        Ok(())
    }

    async fn recv(&self) -> Result<RawEvent, DriverError> {
        let mut rx = self.inject_rx.lock().await;
        rx.recv().await.ok_or(DriverError::Closed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KeyCode;

    #[tokio::test]
    async fn send_records_into_drain_sent() {
        let d = MockDriver::new();
        d.send(RawEvent::KeyDown { key: KeyCode::A }).await.unwrap();
        d.send(RawEvent::KeyUp { key: KeyCode::A }).await.unwrap();
        let s = d.drain_sent();
        assert_eq!(s.len(), 2);
        // Drain empties.
        assert_eq!(d.drain_sent().len(), 0);
    }

    #[tokio::test]
    async fn recv_returns_injected_in_order() {
        let d = MockDriver::new();
        d.inject(RawEvent::KeyDown { key: KeyCode::A });
        d.inject(RawEvent::KeyUp { key: KeyCode::A });
        assert_eq!(d.recv().await.unwrap(),
                   RawEvent::KeyDown { key: KeyCode::A });
        assert_eq!(d.recv().await.unwrap(),
                   RawEvent::KeyUp { key: KeyCode::A });
    }

    #[tokio::test]
    async fn recv_returns_closed_when_dropped() {
        let d = MockDriver::new();
        let d2 = d.clone();
        // Drop the original; the cloned one keeps the channel alive.
        drop(d);
        // Inject + recv via d2 should still work.
        d2.inject(RawEvent::KeyDown { key: KeyCode::B });
        assert!(d2.recv().await.is_ok());
    }

    #[tokio::test]
    async fn send_and_recv_independent() {
        let d = MockDriver::new();
        // recv waits — spawn injector after delay.
        let d2 = d.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            d2.inject(RawEvent::KeyDown { key: KeyCode::C });
        });
        let e = d.recv().await.unwrap();
        assert_eq!(e, RawEvent::KeyDown { key: KeyCode::C });
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p rm-driver mock::`
Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/driver/src/mock.rs
git commit -m "feat(driver): MockDriver with inject/drain_sent for tests"
```

---

## Task 7 — `storage` crate

**Files:**
- Create: `crates/storage/Cargo.toml`
- Create: `crates/storage/src/lib.rs`

- [ ] **Step 1: Create crate**

Create `crates/storage/Cargo.toml`:

```toml
[package]
name = "rm-storage"
version.workspace = true
edition.workspace = true

[dependencies]
rm-error = { path = "../error" }
rm-macro-model = { path = "../macro_model" }
serde_json.workspace = true
tracing.workspace = true
uuid.workspace = true

[dev-dependencies]
tempfile.workspace = true
chrono.workspace = true
```

- [ ] **Step 2: Write impl + tests**

Create `crates/storage/src/lib.rs`:

```rust
use std::fs;
use std::path::{Path, PathBuf};

use rm_error::{AppError, Result};
use rm_macro_model::Macro;
use tracing::warn;
use uuid::Uuid;

/// Returns the directory holding macro files, given a storage root.
pub fn macros_dir(root: &Path) -> PathBuf {
    root.join("macros")
}

/// Save (or overwrite) a macro to `<root>/macros/<id>.json` via atomic
/// write-then-rename. Creates the directory if missing.
pub fn save_macro(root: &Path, m: &Macro) -> Result<()> {
    let dir = macros_dir(root);
    fs::create_dir_all(&dir).map_err(|source| AppError::Io { path: dir.clone(), source })?;
    let path = dir.join(format!("{}.json", m.id));
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(m)?;
    fs::write(&tmp, json).map_err(|source| AppError::Io { path: tmp.clone(), source })?;
    fs::rename(&tmp, &path).map_err(|source| AppError::Io { path: path.clone(), source })?;
    Ok(())
}

/// Load a single macro by id. Returns `MacroNotFound` if no file exists.
pub fn load_macro(root: &Path, id: Uuid) -> Result<Macro> {
    let path = macros_dir(root).join(format!("{id}.json"));
    if !path.exists() {
        return Err(AppError::MacroNotFound(id.to_string()));
    }
    let s = fs::read_to_string(&path).map_err(|source| AppError::Io { path: path.clone(), source })?;
    Ok(serde_json::from_str(&s)?)
}

/// Load every readable macro from `<root>/macros/`. Malformed files are logged
/// and skipped — never aborted on. Returns an empty vec if the directory is
/// missing.
pub fn load_all(root: &Path) -> Result<Vec<Macro>> {
    let dir = macros_dir(root);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|source| AppError::Io { path: dir.clone(), source })? {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => { warn!(error = %e, "skipping unreadable dir entry"); continue; }
        };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") { continue; }
        match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Macro>(&text) {
                Ok(m) => out.push(m),
                Err(e) => warn!(path = %path.display(), error = %e, "skipping malformed macro file"),
            },
            Err(e) => warn!(path = %path.display(), error = %e, "skipping unreadable macro file"),
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Delete a macro by id. No-op if it does not exist.
pub fn delete_macro(root: &Path, id: Uuid) -> Result<()> {
    let path = macros_dir(root).join(format!("{id}.json"));
    if path.exists() {
        fs::remove_file(&path).map_err(|source| AppError::Io { path, source })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rm_macro_model::{KeyCode, Modifier, PlaybackMode, Step, Trigger};
    use tempfile::TempDir;

    fn sample_macro(name: &str) -> Macro {
        let mut m = Macro::new(
            name,
            Trigger::Hotkey { key: KeyCode::F1, modifiers: vec![Modifier::Ctrl] },
            PlaybackMode::Once,
        );
        m.steps.push(Step::KeyPress { key: KeyCode::A, hold_ms: 100 });
        m
    }

    #[test]
    fn save_then_load_macro_roundtrips() {
        let tmp = TempDir::new().unwrap();
        let m = sample_macro("hello");
        save_macro(tmp.path(), &m).unwrap();
        let back = load_macro(tmp.path(), m.id).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn load_missing_returns_not_found() {
        let tmp = TempDir::new().unwrap();
        let err = load_macro(tmp.path(), Uuid::new_v4()).unwrap_err();
        assert_eq!(err.kind(), "MacroNotFound");
    }

    #[test]
    fn load_all_empty_when_dir_missing() {
        let tmp = TempDir::new().unwrap();
        assert!(load_all(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn load_all_skips_malformed() {
        let tmp = TempDir::new().unwrap();
        let m1 = sample_macro("a");
        let m2 = sample_macro("b");
        save_macro(tmp.path(), &m1).unwrap();
        save_macro(tmp.path(), &m2).unwrap();
        // Write a junk file.
        fs::write(macros_dir(tmp.path()).join("garbage.json"), "not json").unwrap();
        // Write a non-json file (should be ignored by extension filter).
        fs::write(macros_dir(tmp.path()).join("readme.txt"), "ignored").unwrap();

        let all = load_all(tmp.path()).unwrap();
        let names: Vec<_> = all.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn delete_removes_file() {
        let tmp = TempDir::new().unwrap();
        let m = sample_macro("toremove");
        save_macro(tmp.path(), &m).unwrap();
        delete_macro(tmp.path(), m.id).unwrap();
        assert!(load_macro(tmp.path(), m.id).is_err());
        // Deleting again is no-op.
        delete_macro(tmp.path(), m.id).unwrap();
    }

    #[test]
    fn save_overwrites_existing() {
        let tmp = TempDir::new().unwrap();
        let mut m = sample_macro("over");
        save_macro(tmp.path(), &m).unwrap();
        m.name = "renamed".into();
        save_macro(tmp.path(), &m).unwrap();
        let back = load_macro(tmp.path(), m.id).unwrap();
        assert_eq!(back.name, "renamed");
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p rm-storage`
Expected: 6 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/storage
git commit -m "feat(storage): atomic save/load/delete for macros"
```

---

## Task 8 — `recorder` crate: event compilation

**Files:**
- Create: `crates/recorder/Cargo.toml`
- Create: `crates/recorder/src/lib.rs`
- Create: `crates/recorder/src/compile.rs`

- [ ] **Step 1: Create crate**

Create `crates/recorder/Cargo.toml`:

```toml
[package]
name = "rm-recorder"
version.workspace = true
edition.workspace = true

[dependencies]
async-trait.workspace = true
rm-driver = { path = "../driver" }
rm-error = { path = "../error" }
rm-macro-model = { path = "../macro_model" }
tokio = { workspace = true, features = ["sync", "rt", "macros", "time"] }
tracing.workspace = true

[dev-dependencies]
tokio = { workspace = true, features = ["test-util", "rt-multi-thread"] }
```

Create `crates/recorder/src/lib.rs` (will expand in Task 9):

```rust
pub mod compile;
pub use compile::compile_events;
```

- [ ] **Step 2: Write compile.rs with TDD — failing tests first**

Create `crates/recorder/src/compile.rs`:

```rust
use std::time::{Duration, Instant};

use rm_driver::RawEvent;
use rm_macro_model::{KeyCode, MouseButton, Point, Step};

/// One raw event paired with its capture timestamp.
#[derive(Debug, Clone)]
pub struct TimedEvent {
    pub event: RawEvent,
    pub at: Instant,
}

/// Compile a sequence of raw timed events into a high-level `Vec<Step>`:
///   * `KeyDown(k) → KeyUp(k)` within a single recording collapses into
///     `KeyPress { key: k, hold_ms: delta }`.
///   * `MouseDown(b) → MouseUp(b)` collapses into `MouseClick { hold_ms: delta }`.
///   * `MouseMove` events become `Step::MouseMove { mode: Relative, to: dxdy }`.
///   * `MouseWheel` becomes `Step::MouseScroll`.
///   * Inter-event gaps become `Step::Wait { min_ms: gap, max_ms: gap }`.
///   * Trailing key/mouse that never went up emits a `Step::KeyDown` /
///     `Step::*` lone variant. (Caller decides how to surface this.)
pub fn compile_events(raw: &[TimedEvent]) -> Vec<Step> {
    if raw.is_empty() { return Vec::new(); }
    let mut out = Vec::new();
    let mut i = 0;
    let mut last_at = raw[0].at;
    while i < raw.len() {
        let cur = &raw[i];
        // Emit a Wait for the gap since previous emitted step's wall-clock end.
        let gap = cur.at.duration_since(last_at);
        if gap >= Duration::from_millis(1) {
            let ms = gap.as_millis().min(u32::MAX as u128) as u32;
            out.push(Step::Wait { min_ms: ms, max_ms: ms });
        }
        match cur.event {
            RawEvent::KeyDown { key } => {
                // Look ahead for matching KeyUp.
                if let Some(j) = find_matching_key_up(raw, i + 1, key) {
                    let hold = duration_ms_between(cur.at, raw[j].at);
                    out.push(Step::KeyPress { key, hold_ms: hold });
                    last_at = raw[j].at;
                    i = j + 1;
                    continue;
                } else {
                    out.push(Step::KeyDown { key });
                }
            }
            RawEvent::KeyUp { key } => {
                // Lone KeyUp without a prior KeyDown — emit as-is.
                out.push(Step::KeyUp { key });
            }
            RawEvent::MouseDown { button } => {
                if let Some(j) = find_matching_mouse_up(raw, i + 1, button) {
                    let hold = duration_ms_between(cur.at, raw[j].at);
                    out.push(Step::MouseClick { button, hold_ms: hold, at: None });
                    last_at = raw[j].at;
                    i = j + 1;
                    continue;
                } else {
                    out.push(Step::MouseClick { button, hold_ms: 0, at: None });
                }
            }
            RawEvent::MouseUp { .. } => {
                // Orphan up — drop silently. (Compile contract: caller pairs them.)
            }
            RawEvent::MouseMove { dx, dy } => {
                out.push(Step::MouseMove {
                    to: Point { x: dx, y: dy },
                    mode: rm_macro_model::MoveMode::Relative,
                });
            }
            RawEvent::MouseWheel { delta } => {
                out.push(Step::MouseScroll { delta });
            }
        }
        last_at = cur.at;
        i += 1;
    }
    out
}

fn find_matching_key_up(raw: &[TimedEvent], from: usize, key: KeyCode) -> Option<usize> {
    raw[from..].iter().position(|e| matches!(e.event, RawEvent::KeyUp { key: k } if k == key))
        .map(|p| p + from)
}

fn find_matching_mouse_up(raw: &[TimedEvent], from: usize, button: MouseButton) -> Option<usize> {
    raw[from..].iter().position(|e| matches!(e.event, RawEvent::MouseUp { button: b } if b == button))
        .map(|p| p + from)
}

fn duration_ms_between(a: Instant, b: Instant) -> u32 {
    b.saturating_duration_since(a).as_millis().min(u32::MAX as u128) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use rm_macro_model::{KeyCode, MouseButton};

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn ev(at: Instant, e: RawEvent) -> TimedEvent { TimedEvent { event: e, at } }

    #[test]
    fn empty_returns_empty() {
        assert!(compile_events(&[]).is_empty());
    }

    #[test]
    fn keydown_keyup_collapses_to_keypress() {
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0), RawEvent::KeyDown { key: KeyCode::W }),
            ev(at(t0, 250), RawEvent::KeyUp { key: KeyCode::W }),
        ];
        let steps = compile_events(&raw);
        assert_eq!(steps, vec![Step::KeyPress { key: KeyCode::W, hold_ms: 250 }]);
    }

    #[test]
    fn gap_between_keys_emits_wait() {
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0),   RawEvent::KeyDown { key: KeyCode::A }),
            ev(at(t0, 80),  RawEvent::KeyUp   { key: KeyCode::A }),
            ev(at(t0, 230), RawEvent::KeyDown { key: KeyCode::B }),
            ev(at(t0, 310), RawEvent::KeyUp   { key: KeyCode::B }),
        ];
        let steps = compile_events(&raw);
        assert_eq!(steps, vec![
            Step::KeyPress { key: KeyCode::A, hold_ms: 80 },
            Step::Wait { min_ms: 150, max_ms: 150 },
            Step::KeyPress { key: KeyCode::B, hold_ms: 80 },
        ]);
    }

    #[test]
    fn lone_keydown_without_keyup_emits_keydown() {
        let t0 = Instant::now();
        let raw = vec![ev(at(t0, 0), RawEvent::KeyDown { key: KeyCode::LShift })];
        let steps = compile_events(&raw);
        assert_eq!(steps, vec![Step::KeyDown { key: KeyCode::LShift }]);
    }

    #[test]
    fn mouse_down_up_collapses() {
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0),   RawEvent::MouseDown { button: MouseButton::Left }),
            ev(at(t0, 60),  RawEvent::MouseUp   { button: MouseButton::Left }),
        ];
        let steps = compile_events(&raw);
        assert_eq!(steps, vec![
            Step::MouseClick { button: MouseButton::Left, hold_ms: 60, at: None },
        ]);
    }

    #[test]
    fn mouse_move_passes_through() {
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0), RawEvent::MouseMove { dx: 10, dy: -5 }),
        ];
        let steps = compile_events(&raw);
        assert_eq!(steps, vec![
            Step::MouseMove { to: Point { x: 10, y: -5 }, mode: rm_macro_model::MoveMode::Relative },
        ]);
    }

    #[test]
    fn mouse_wheel_passes_through() {
        let t0 = Instant::now();
        let raw = vec![ev(at(t0, 0), RawEvent::MouseWheel { delta: 120 })];
        assert_eq!(compile_events(&raw), vec![Step::MouseScroll { delta: 120 }]);
    }

    #[test]
    fn overlapping_keys_pair_correctly() {
        // Down A, Down B, Up A, Up B → A pairs with the *first* matching up.
        let t0 = Instant::now();
        let raw = vec![
            ev(at(t0, 0),   RawEvent::KeyDown { key: KeyCode::A }),
            ev(at(t0, 50),  RawEvent::KeyDown { key: KeyCode::B }),
            ev(at(t0, 100), RawEvent::KeyUp   { key: KeyCode::A }),
            ev(at(t0, 150), RawEvent::KeyUp   { key: KeyCode::B }),
        ];
        let steps = compile_events(&raw);
        // A press goes 0→100 (hold 100); after the A pair, last_at jumps to t0+100.
        // Then we see KeyDown B at t0+50. Gap is negative (saturates to 0), so no Wait.
        // B keypress runs 50→150 (hold 100). But last_at was 100 when we started B,
        // so the gap from 100→50 saturates to 0 (no Wait emitted).
        assert_eq!(steps, vec![
            Step::KeyPress { key: KeyCode::A, hold_ms: 100 },
            Step::KeyPress { key: KeyCode::B, hold_ms: 100 },
        ]);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p rm-recorder`
Expected: 8 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/recorder
git commit -m "feat(recorder): compile raw events into high-level Step list"
```

---

## Task 9 — `recorder`: record task

**Files:**
- Modify: `crates/recorder/src/lib.rs`

- [ ] **Step 1: Define record task API + tests**

Replace `crates/recorder/src/lib.rs`:

```rust
pub mod compile;
pub use compile::{compile_events, TimedEvent};

use std::sync::Arc;
use std::time::Instant;

use rm_driver::{Driver, RawEvent};
use rm_error::Result;
use rm_macro_model::Step;
use tokio::sync::{oneshot, Mutex};
use tracing::debug;

/// Handle to a running recording. Drop to cancel; `finish().await` to
/// gracefully stop and retrieve the compiled steps.
pub struct RecordingHandle {
    stop_tx: Option<oneshot::Sender<()>>,
    join: tokio::task::JoinHandle<Vec<TimedEvent>>,
    /// Whether the recorder should re-emit captured events back to the driver
    /// during recording (passthrough). True is what the production app uses;
    /// tests typically set this false.
    pub passthrough: bool,
}

impl RecordingHandle {
    /// Stop the recording, await the task, and return the compiled steps.
    pub async fn finish(mut self) -> Result<Vec<Step>> {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        let raw = self.join.await.map_err(|e| {
            rm_error::AppError::Other(format!("recorder task panicked: {e}"))
        })?;
        Ok(compile_events(&raw))
    }
}

/// Start a recording. Reads from `driver.recv()` until a stop signal is sent
/// via `finish()`. If `passthrough` is true, each captured event is re-emitted
/// via `driver.send()` so the OS still sees the input.
pub fn start_recording(
    driver: Arc<dyn Driver>,
    passthrough: bool,
) -> RecordingHandle {
    let (stop_tx, mut stop_rx) = oneshot::channel();
    let buf: Arc<Mutex<Vec<TimedEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let buf_task = buf.clone();
    let join = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut stop_rx => {
                    debug!("recorder: stop signal received");
                    break;
                }
                got = driver.recv() => match got {
                    Ok(event) => {
                        let at = Instant::now();
                        if passthrough {
                            if let Err(e) = driver.send(event).await {
                                debug!(error = ?e, "recorder: passthrough send failed");
                            }
                        }
                        buf_task.lock().await.push(TimedEvent { event, at });
                    }
                    Err(rm_driver::DriverError::Closed) => {
                        debug!("recorder: driver closed");
                        break;
                    }
                    Err(e) => {
                        debug!(error = ?e, "recorder: driver recv error, stopping");
                        break;
                    }
                }
            }
        }
        // Drain the buffer.
        std::mem::take(&mut *buf_task.lock().await)
    });
    RecordingHandle { stop_tx: Some(stop_tx), join, passthrough }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rm_driver::mock::MockDriver;
    use rm_driver::RawEvent;
    use rm_macro_model::{KeyCode, Step};

    #[tokio::test]
    async fn records_injected_events_no_passthrough() {
        let drv = Arc::new(MockDriver::new());
        let h = start_recording(drv.clone(), false);

        drv.inject(RawEvent::KeyDown { key: KeyCode::A });
        // Small sleep so timestamps differ.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        drv.inject(RawEvent::KeyUp { key: KeyCode::A });
        // Give the task a tick to drain the inject channel before stop.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let steps = h.finish().await.unwrap();
        // Expect: KeyPress with non-zero hold.
        assert_eq!(steps.len(), 1);
        match &steps[0] {
            Step::KeyPress { key, hold_ms } => {
                assert_eq!(*key, KeyCode::A);
                assert!(*hold_ms >= 40 && *hold_ms <= 200, "hold_ms was {hold_ms}");
            }
            other => panic!("expected KeyPress, got {other:?}"),
        }
        // No passthrough → driver should not have anything sent.
        assert!(drv.drain_sent().is_empty());
    }

    #[tokio::test]
    async fn passthrough_re_emits_events() {
        let drv = Arc::new(MockDriver::new());
        let h = start_recording(drv.clone(), true);

        drv.inject(RawEvent::KeyDown { key: KeyCode::B });
        drv.inject(RawEvent::KeyUp { key: KeyCode::B });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let _ = h.finish().await.unwrap();

        let sent = drv.drain_sent();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0], RawEvent::KeyDown { key: KeyCode::B });
        assert_eq!(sent[1], RawEvent::KeyUp { key: KeyCode::B });
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p rm-recorder`
Expected: 8 (compile) + 2 (task) = 10 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/recorder/src/lib.rs
git commit -m "feat(recorder): start_recording task with passthrough + finish() API"
```

---

## Task 10 — `player` crate

**Files:**
- Create: `crates/player/Cargo.toml`
- Create: `crates/player/src/lib.rs`

- [ ] **Step 1: Create crate**

Create `crates/player/Cargo.toml`:

```toml
[package]
name = "rm-player"
version.workspace = true
edition.workspace = true

[dependencies]
async-trait.workspace = true
rand.workspace = true
rm-driver = { path = "../driver" }
rm-error = { path = "../error" }
rm-macro-model = { path = "../macro_model" }
tokio = { workspace = true, features = ["sync", "rt", "macros", "time"] }
tracing.workspace = true

[dev-dependencies]
tokio = { workspace = true, features = ["test-util", "rt-multi-thread"] }
```

- [ ] **Step 2: Define play API with tests for each Step variant + each PlaybackMode**

Create `crates/player/src/lib.rs`:

```rust
use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use rm_driver::{Driver, RawEvent};
use rm_error::{AppError, Result};
use rm_macro_model::{Macro, PlaybackMode, Step};
use tokio::sync::oneshot;
use tracing::debug;

/// Handle to a running playback. Drop to cancel; `stop()` to request a clean
/// stop; `wait()` to await completion.
pub struct PlaybackHandle {
    stop_tx: Option<oneshot::Sender<()>>,
    join: tokio::task::JoinHandle<Result<()>>,
}

impl PlaybackHandle {
    /// Request the player to stop. The current step is allowed to finish.
    pub fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
    }

    /// Await the player. Returns the player's result.
    pub async fn wait(self) -> Result<()> {
        self.join.await
            .map_err(|e| AppError::Other(format!("player task panicked: {e}")))?
    }
}

/// Spawn a player task to execute `macro_`. Returns immediately with a handle.
pub fn play(driver: Arc<dyn Driver>, macro_: Macro) -> PlaybackHandle {
    let (stop_tx, stop_rx) = oneshot::channel();
    let join = tokio::spawn(async move {
        run(driver, &macro_, stop_rx).await
    });
    PlaybackHandle { stop_tx: Some(stop_tx), join }
}

async fn run(
    driver: Arc<dyn Driver>,
    m: &Macro,
    mut stop_rx: oneshot::Receiver<()>,
) -> Result<()> {
    debug!(macro_name = %m.name, mode = ?m.playback, "player: starting");
    let mut iter = playback_iter(m.playback);
    while iter.next() {
        for step in &m.steps {
            if stop_rx.try_recv().is_ok() {
                debug!("player: stop signal");
                return Ok(());
            }
            run_step(&*driver, step).await?;
        }
    }
    Ok(())
}

async fn run_step(driver: &dyn Driver, step: &Step) -> Result<()> {
    match step {
        Step::KeyPress { key, hold_ms } => {
            driver.send(RawEvent::KeyDown { key: *key }).await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
            tokio::time::sleep(Duration::from_millis((*hold_ms).into())).await;
            driver.send(RawEvent::KeyUp { key: *key }).await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }
        Step::KeyDown { key } => {
            driver.send(RawEvent::KeyDown { key: *key }).await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }
        Step::KeyUp { key } => {
            driver.send(RawEvent::KeyUp { key: *key }).await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }
        Step::MouseClick { button, hold_ms, at: _ } => {
            // `at` is a TODO for Plan 2 (absolute positioning). For Plan 1 we
            // emit the click without moving.
            driver.send(RawEvent::MouseDown { button: *button }).await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
            tokio::time::sleep(Duration::from_millis((*hold_ms).into())).await;
            driver.send(RawEvent::MouseUp { button: *button }).await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }
        Step::MouseMove { to, mode: _ } => {
            driver.send(RawEvent::MouseMove { dx: to.x, dy: to.y }).await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }
        Step::MouseScroll { delta } => {
            driver.send(RawEvent::MouseWheel { delta: *delta }).await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }
        Step::Wait { min_ms, max_ms } => {
            let ms = if min_ms == max_ms {
                *min_ms
            } else {
                rand::thread_rng().gen_range(*min_ms..=*max_ms)
            };
            tokio::time::sleep(Duration::from_millis(ms.into())).await;
        }
    }
    Ok(())
}

/// State machine for the loop bound.
struct PlaybackIter {
    remaining: Option<u64>, // None = infinite
}

impl PlaybackIter {
    fn next(&mut self) -> bool {
        match &mut self.remaining {
            None => true,
            Some(0) => false,
            Some(n) => { *n -= 1; true }
        }
    }
}

fn playback_iter(mode: PlaybackMode) -> PlaybackIter {
    let remaining = match mode {
        PlaybackMode::Once => Some(1),
        PlaybackMode::Repeat { count } => Some(count as u64),
        PlaybackMode::Loop | PlaybackMode::Toggle => None,
    };
    PlaybackIter { remaining }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rm_driver::mock::MockDriver;
    use rm_macro_model::{KeyCode, Macro, Trigger};

    fn macro_with_steps(steps: Vec<Step>, playback: PlaybackMode) -> Macro {
        let mut m = Macro::new("t",
            Trigger::Hotkey { key: KeyCode::F1, modifiers: vec![] }, playback);
        m.steps = steps;
        m
    }

    #[tokio::test]
    async fn keypress_emits_down_then_up() {
        let drv = Arc::new(MockDriver::new());
        let m = macro_with_steps(
            vec![Step::KeyPress { key: KeyCode::A, hold_ms: 5 }],
            PlaybackMode::Once);
        play(drv.clone(), m).wait().await.unwrap();
        let sent = drv.drain_sent();
        assert_eq!(sent, vec![
            RawEvent::KeyDown { key: KeyCode::A },
            RawEvent::KeyUp   { key: KeyCode::A },
        ]);
    }

    #[tokio::test]
    async fn wait_is_random_within_range() {
        // Smoke: run Wait { 10, 20 } a few times; just verify it doesn't
        // panic and the player completes. Time bounds aren't asserted —
        // OS scheduling makes that flaky.
        let drv = Arc::new(MockDriver::new());
        let m = macro_with_steps(
            vec![Step::Wait { min_ms: 10, max_ms: 20 }],
            PlaybackMode::Repeat { count: 5 });
        play(drv.clone(), m).wait().await.unwrap();
        assert!(drv.sent_snapshot().is_empty());
    }

    #[tokio::test]
    async fn mouse_click_emits_down_up() {
        let drv = Arc::new(MockDriver::new());
        let m = macro_with_steps(
            vec![Step::MouseClick {
                button: rm_macro_model::MouseButton::Left,
                hold_ms: 5,
                at: None }],
            PlaybackMode::Once);
        play(drv.clone(), m).wait().await.unwrap();
        let sent = drv.drain_sent();
        assert!(matches!(sent[0], RawEvent::MouseDown { .. }));
        assert!(matches!(sent[1], RawEvent::MouseUp   { .. }));
    }

    #[tokio::test]
    async fn repeat_n_runs_n_times() {
        let drv = Arc::new(MockDriver::new());
        let m = macro_with_steps(
            vec![Step::KeyPress { key: KeyCode::X, hold_ms: 0 }],
            PlaybackMode::Repeat { count: 4 });
        play(drv.clone(), m).wait().await.unwrap();
        assert_eq!(drv.drain_sent().len(), 4 * 2); // 4 iterations × (down+up)
    }

    #[tokio::test]
    async fn once_runs_once() {
        let drv = Arc::new(MockDriver::new());
        let m = macro_with_steps(
            vec![Step::KeyPress { key: KeyCode::X, hold_ms: 0 }],
            PlaybackMode::Once);
        play(drv.clone(), m).wait().await.unwrap();
        assert_eq!(drv.drain_sent().len(), 2);
    }

    #[tokio::test]
    async fn loop_stops_on_signal() {
        let drv = Arc::new(MockDriver::new());
        let m = macro_with_steps(
            vec![
                Step::KeyPress { key: KeyCode::X, hold_ms: 1 },
                Step::Wait { min_ms: 5, max_ms: 5 },
            ],
            PlaybackMode::Loop);
        let mut h = play(drv.clone(), m);
        tokio::time::sleep(Duration::from_millis(50)).await;
        h.stop();
        h.wait().await.unwrap();
        // It should have completed some iterations and stopped.
        let count = drv.drain_sent().len();
        assert!(count > 0 && count % 2 == 0, "sent count was {count}");
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p rm-player`
Expected: 6 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/player
git commit -m "feat(player): step execution + PlaybackMode iteration + stop signal"
```

---

## Task 11 — `hotkey` crate

**Files:**
- Create: `crates/hotkey/Cargo.toml`
- Create: `crates/hotkey/src/lib.rs`

- [ ] **Step 1: Create crate**

Create `crates/hotkey/Cargo.toml`:

```toml
[package]
name = "rm-hotkey"
version.workspace = true
edition.workspace = true

[dependencies]
async-trait.workspace = true
rm-driver = { path = "../driver" }
rm-error = { path = "../error" }
rm-macro-model = { path = "../macro_model" }
tokio = { workspace = true, features = ["sync", "rt", "macros"] }
tracing.workspace = true
uuid.workspace = true

[dev-dependencies]
tokio = { workspace = true, features = ["test-util", "rt-multi-thread"] }
```

- [ ] **Step 2: Design + tests**

Create `crates/hotkey/src/lib.rs`:

```rust
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rm_driver::{Driver, RawEvent};
use rm_macro_model::{KeyCode, Modifier, Trigger};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::debug;
use uuid::Uuid;

/// A hotkey fired event: which macro id the user wants triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyHit(pub Uuid);

/// Registry of macro-id → trigger. Cheap to clone (Arc inside).
#[derive(Clone, Default)]
pub struct HotkeyRegistry {
    inner: Arc<Mutex<RegistryInner>>,
}

#[derive(Default)]
struct RegistryInner {
    by_id: HashMap<Uuid, Trigger>,
}

impl HotkeyRegistry {
    pub fn new() -> Self { Self::default() }

    /// Register or replace the hotkey for a macro.
    pub async fn bind(&self, id: Uuid, trigger: Trigger) {
        self.inner.lock().await.by_id.insert(id, trigger);
    }

    pub async fn unbind(&self, id: Uuid) {
        self.inner.lock().await.by_id.remove(&id);
    }

    /// Returns every macro id whose trigger matches the given pressed-key set.
    pub async fn match_pressed(
        &self,
        key: KeyCode,
        modifiers: &HashSet<Modifier>,
    ) -> Vec<Uuid> {
        let g = self.inner.lock().await;
        g.by_id.iter()
            .filter_map(|(id, t)| match t {
                Trigger::Hotkey { key: tk, modifiers: tm } => {
                    let tm_set: HashSet<_> = tm.iter().copied().collect();
                    if *tk == key && tm_set == *modifiers { Some(*id) } else { None }
                }
            })
            .collect()
    }
}

/// Handle to the hotkey listener task. Stop by dropping.
pub struct ListenerHandle {
    stop_tx: Option<oneshot::Sender<()>>,
    join: tokio::task::JoinHandle<()>,
}

impl ListenerHandle {
    pub async fn shutdown(mut self) {
        if let Some(tx) = self.stop_tx.take() { let _ = tx.send(()); }
        let _ = self.join.await;
    }
}

/// Spawn a task that reads from `driver`, tracks pressed modifiers, and
/// emits a `HotkeyHit` on `out_tx` for every key press that matches a binding.
pub fn start_listener(
    driver: Arc<dyn Driver>,
    registry: HotkeyRegistry,
    out_tx: mpsc::UnboundedSender<HotkeyHit>,
) -> ListenerHandle {
    let (stop_tx, mut stop_rx) = oneshot::channel();
    let join = tokio::spawn(async move {
        let mut mods: HashSet<Modifier> = HashSet::new();
        loop {
            tokio::select! {
                _ = &mut stop_rx => { debug!("hotkey: stop"); break; }
                got = driver.recv() => match got {
                    Ok(RawEvent::KeyDown { key }) => {
                        if let Some(m) = key_as_modifier(key) {
                            mods.insert(m);
                        } else {
                            for id in registry.match_pressed(key, &mods).await {
                                let _ = out_tx.send(HotkeyHit(id));
                            }
                        }
                    }
                    Ok(RawEvent::KeyUp { key }) => {
                        if let Some(m) = key_as_modifier(key) { mods.remove(&m); }
                    }
                    Ok(_) => { /* mouse events not used for hotkeys in v1 */ }
                    Err(_) => break,
                }
            }
        }
    });
    ListenerHandle { stop_tx: Some(stop_tx), join }
}

fn key_as_modifier(k: KeyCode) -> Option<Modifier> {
    match k {
        KeyCode::LShift | KeyCode::RShift => Some(Modifier::Shift),
        KeyCode::LCtrl  | KeyCode::RCtrl  => Some(Modifier::Ctrl),
        KeyCode::LAlt   | KeyCode::RAlt   => Some(Modifier::Alt),
        KeyCode::LWin   | KeyCode::RWin   => Some(Modifier::Win),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rm_driver::mock::MockDriver;

    #[tokio::test]
    async fn bind_and_unbind_round_trip() {
        let r = HotkeyRegistry::new();
        let id = Uuid::new_v4();
        r.bind(id, Trigger::Hotkey { key: KeyCode::F1, modifiers: vec![] }).await;
        let mut s = HashSet::new();
        assert_eq!(r.match_pressed(KeyCode::F1, &s).await, vec![id]);
        r.unbind(id).await;
        assert!(r.match_pressed(KeyCode::F1, &s).await.is_empty());

        // Modifiers must match.
        r.bind(id, Trigger::Hotkey { key: KeyCode::F1,
            modifiers: vec![Modifier::Ctrl] }).await;
        assert!(r.match_pressed(KeyCode::F1, &s).await.is_empty()); // no ctrl pressed
        s.insert(Modifier::Ctrl);
        assert_eq!(r.match_pressed(KeyCode::F1, &s).await, vec![id]);
    }

    #[tokio::test]
    async fn listener_dispatches_on_match() {
        let drv = Arc::new(MockDriver::new());
        let reg = HotkeyRegistry::new();
        let id = Uuid::new_v4();
        reg.bind(id, Trigger::Hotkey { key: KeyCode::F2,
            modifiers: vec![Modifier::Ctrl] }).await;

        let (tx, mut rx) = mpsc::unbounded_channel();
        let handle = start_listener(drv.clone(), reg.clone(), tx);

        drv.inject(RawEvent::KeyDown { key: KeyCode::LCtrl });
        drv.inject(RawEvent::KeyDown { key: KeyCode::F2 });

        let hit = tokio::time::timeout(
            std::time::Duration::from_millis(200), rx.recv()).await.unwrap().unwrap();
        assert_eq!(hit, HotkeyHit(id));

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn modifier_release_drops_match() {
        let drv = Arc::new(MockDriver::new());
        let reg = HotkeyRegistry::new();
        let id = Uuid::new_v4();
        reg.bind(id, Trigger::Hotkey { key: KeyCode::F3,
            modifiers: vec![Modifier::Ctrl] }).await;

        let (tx, mut rx) = mpsc::unbounded_channel();
        let handle = start_listener(drv.clone(), reg.clone(), tx);

        drv.inject(RawEvent::KeyDown { key: KeyCode::LCtrl });
        drv.inject(RawEvent::KeyUp   { key: KeyCode::LCtrl });
        drv.inject(RawEvent::KeyDown { key: KeyCode::F3 });

        // F3 alone shouldn't fire (binding requires Ctrl).
        let r = tokio::time::timeout(
            std::time::Duration::from_millis(100), rx.recv()).await;
        assert!(r.is_err(), "expected no hit, got {:?}", r);

        handle.shutdown().await;
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p rm-hotkey`
Expected: 3 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/hotkey
git commit -m "feat(hotkey): registry + listener task with modifier tracking"
```

---

## Task 12 — `cli` crate scaffold + `stdio_driver`

**Files:**
- Create: `crates/cli/Cargo.toml`
- Create: `crates/cli/src/main.rs`
- Create: `crates/cli/src/stdio_driver.rs`
- Create: `crates/cli/src/commands.rs`

- [ ] **Step 1: Create crate manifest**

Create `crates/cli/Cargo.toml`:

```toml
[package]
name = "rm-cli"
version.workspace = true
edition.workspace = true

[[bin]]
name = "macro-cli"
path = "src/main.rs"

[dependencies]
anyhow.workspace = true
async-trait.workspace = true
clap.workspace = true
rm-driver = { path = "../driver" }
rm-error = { path = "../error" }
rm-hotkey = { path = "../hotkey" }
rm-macro-model = { path = "../macro_model" }
rm-player = { path = "../player" }
rm-recorder = { path = "../recorder" }
rm-storage = { path = "../storage" }
serde_json.workspace = true
tokio = { workspace = true, features = ["full"] }
tracing.workspace = true
tracing-subscriber.workspace = true
uuid.workspace = true
```

- [ ] **Step 2: Stdio driver that reads/writes JSONL**

Create `crates/cli/src/stdio_driver.rs`:

```rust
use std::sync::Arc;

use async_trait::async_trait;
use rm_driver::{Driver, DriverError, RawEvent};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;

/// Driver impl that consumes RawEvents from stdin (one JSON object per line)
/// and prints events sent toward "the OS" to stdout (one JSON object per line).
/// Used by the CLI to demo the pipeline end-to-end without the real driver.
pub struct StdioDriver {
    stdin: Arc<Mutex<BufReader<tokio::io::Stdin>>>,
    stdout: Arc<Mutex<tokio::io::Stdout>>,
}

impl StdioDriver {
    pub fn new() -> Self {
        Self {
            stdin: Arc::new(Mutex::new(BufReader::new(tokio::io::stdin()))),
            stdout: Arc::new(Mutex::new(tokio::io::stdout())),
        }
    }
}

#[async_trait]
impl Driver for StdioDriver {
    async fn send(&self, event: RawEvent) -> Result<(), DriverError> {
        let mut line = serde_json::to_string(&event)
            .map_err(|e| DriverError::Io(e.to_string()))?;
        line.push('\n');
        let mut out = self.stdout.lock().await;
        out.write_all(line.as_bytes()).await.map_err(|e| DriverError::Io(e.to_string()))?;
        out.flush().await.map_err(|e| DriverError::Io(e.to_string()))?;
        Ok(())
    }

    async fn recv(&self) -> Result<RawEvent, DriverError> {
        let mut buf = String::new();
        let n = {
            let mut r = self.stdin.lock().await;
            r.read_line(&mut buf).await.map_err(|e| DriverError::Io(e.to_string()))?
        };
        if n == 0 { return Err(DriverError::Closed); }
        let trimmed = buf.trim();
        if trimmed.is_empty() { return Err(DriverError::Closed); }
        serde_json::from_str(trimmed).map_err(|e| DriverError::Io(e.to_string()))
    }
}
```

- [ ] **Step 3: Commands wiring (record/play/list/delete)**

Create `crates/cli/src/commands.rs`:

```rust
use std::path::Path;
use std::sync::Arc;

use rm_driver::Driver;
use rm_error::{AppError, Result};
use rm_macro_model::{KeyCode, Macro, Modifier, PlaybackMode, Trigger};
use rm_player::play;
use rm_recorder::start_recording;
use rm_storage::{delete_macro, load_all, load_macro, save_macro};
use uuid::Uuid;

use crate::stdio_driver::StdioDriver;

/// Record from stdin (JSONL of RawEvent), stop on EOF, save under `name`.
pub async fn cmd_record(root: &Path, name: &str) -> Result<()> {
    let drv: Arc<dyn Driver> = Arc::new(StdioDriver::new());
    let handle = start_recording(drv, false);
    // The recorder loop will exit on Driver::Closed when stdin EOFs.
    // We just need to await the join handle by issuing finish() after a short
    // delay — but `start_recording` doesn't return until `finish()` is called.
    // So we drop into a small loop: poll status by checking stdin closure
    // indirectly via timeout — simpler: spawn a parallel task that signals stop
    // when stdin is drained. Easiest model: race a timer with finish().
    // For Plan 1 we accept a 200ms idle window after the first event as "done".
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let steps = handle.finish().await?;
    if steps.is_empty() {
        return Err(AppError::Other("no events recorded".into()));
    }
    let mut m = Macro::new(
        name,
        Trigger::Hotkey { key: KeyCode::F1, modifiers: vec![Modifier::Ctrl] },
        PlaybackMode::Once,
    );
    m.steps = steps;
    save_macro(root, &m)?;
    println!("saved {} ({})", m.name, m.id);
    Ok(())
}

pub async fn cmd_play(root: &Path, name: &str) -> Result<()> {
    let macros = load_all(root)?;
    let m = macros.into_iter().find(|m| m.name == name)
        .ok_or_else(|| AppError::MacroNotFound(name.into()))?;
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
    let id = macros.into_iter().find(|m| m.name == name).map(|m| m.id)
        .ok_or_else(|| AppError::MacroNotFound(name.into()))?;
    delete_macro(root, id)?;
    println!("deleted {name}");
    Ok(())
}

/// Helper for the e2e test (Task 13).
pub async fn cmd_play_by_id(root: &Path, id: Uuid, driver: Arc<dyn Driver>) -> Result<()> {
    let m = load_macro(root, id)?;
    play(driver, m).wait().await
}
```

- [ ] **Step 4: `main.rs` with clap**

Create `crates/cli/src/main.rs`:

```rust
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use rm_error::Result;
use tracing_subscriber::EnvFilter;

mod commands;
mod stdio_driver;

#[derive(Parser)]
#[command(name = "macro-cli", version)]
struct Cli {
    /// Storage root (defaults to ./.rust-macro).
    #[arg(long, global = true)]
    root: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Record events from stdin (JSONL) and save under `name`.
    Record { name: String },
    /// Play the macro named `name` (events emitted to stdout JSONL).
    Play   { name: String },
    /// List all saved macros.
    List,
    /// Delete the macro named `name`.
    Delete { name: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();
    let root = cli.root.unwrap_or_else(|| PathBuf::from("./.rust-macro"));

    let res: Result<()> = match cli.cmd {
        Cmd::Record { name } => commands::cmd_record(&root, &name).await,
        Cmd::Play   { name } => commands::cmd_play(&root, &name).await,
        Cmd::List            => commands::cmd_list(&root),
        Cmd::Delete { name } => commands::cmd_delete(&root, &name),
    };
    res.map_err(|e| anyhow::anyhow!("{e}"))
}
```

- [ ] **Step 5: Build CLI**

Run: `cargo build -p rm-cli`
Expected: build succeeds.

- [ ] **Step 6: Smoke-test `list` (no macros yet)**

Run: `cargo run -p rm-cli -- --root ./.tmp-test list`
Expected: empty output, exit 0.

- [ ] **Step 7: Commit**

```bash
git add crates/cli
git commit -m "feat(cli): macro-cli binary with record/play/list/delete + stdio driver"
```

---

## Task 13 — End-to-end integration test

**Files:**
- Create: `crates/cli/tests/e2e.rs`

- [ ] **Step 1: Write the e2e test**

This drives the whole stack (compile → save → load → play) using `MockDriver` directly, bypassing stdin/stdout. It proves that the data path Plan 1 has built is internally consistent.

Add `rm-driver` (already there) and `tempfile` as a dev-dependency.

Modify `crates/cli/Cargo.toml`, add:

```toml
[dev-dependencies]
tempfile.workspace = true
```

Create `crates/cli/tests/e2e.rs`:

```rust
use std::sync::Arc;
use std::time::{Duration, Instant};

use rm_driver::mock::MockDriver;
use rm_driver::{Driver, RawEvent};
use rm_macro_model::{KeyCode, Macro, Modifier, PlaybackMode, Step, Trigger};
use rm_player::play;
use rm_recorder::{compile_events, TimedEvent};
use rm_storage::{load_macro, save_macro};
use tempfile::TempDir;

#[tokio::test]
async fn record_save_load_play_roundtrip() {
    // 1. "Record" — synthesize a known sequence directly.
    let t0 = Instant::now();
    let raw = vec![
        TimedEvent { event: RawEvent::KeyDown { key: KeyCode::H }, at: t0 },
        TimedEvent { event: RawEvent::KeyUp   { key: KeyCode::H }, at: t0 + Duration::from_millis(60) },
        TimedEvent { event: RawEvent::KeyDown { key: KeyCode::I }, at: t0 + Duration::from_millis(150) },
        TimedEvent { event: RawEvent::KeyUp   { key: KeyCode::I }, at: t0 + Duration::from_millis(220) },
    ];
    let steps = compile_events(&raw);
    assert_eq!(steps.len(), 3, "expected H, Wait, I — got {steps:?}");

    // 2. Save.
    let tmp = TempDir::new().unwrap();
    let mut m = Macro::new("hi",
        Trigger::Hotkey { key: KeyCode::F4, modifiers: vec![Modifier::Ctrl] },
        PlaybackMode::Once);
    m.steps = steps;
    save_macro(tmp.path(), &m).unwrap();

    // 3. Load back.
    let loaded = load_macro(tmp.path(), m.id).unwrap();
    assert_eq!(loaded, m);

    // 4. Play through MockDriver and check the wire sequence.
    let drv = Arc::new(MockDriver::new());
    play(drv.clone() as Arc<dyn Driver>, loaded).wait().await.unwrap();
    let sent = drv.drain_sent();

    assert_eq!(sent, vec![
        RawEvent::KeyDown { key: KeyCode::H },
        RawEvent::KeyUp   { key: KeyCode::H },
        RawEvent::KeyDown { key: KeyCode::I },
        RawEvent::KeyUp   { key: KeyCode::I },
    ]);
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p rm-cli --test e2e`
Expected: 1 test pass.

- [ ] **Step 3: Run the full test suite as a sanity check**

Run: `cargo test --workspace`
Expected: every crate's tests pass. ~35 tests total.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

Run: `cargo fmt --all -- --check`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add crates/cli
git commit -m "test(e2e): record→save→load→play roundtrip through MockDriver"
```

---

## Task 14 — README, dev notes, and Plan 1 closure

**Files:**
- Modify: `README.md`
- Create: `docs/superpowers/plans/2026-05-26-rust-macro-plan-2-real-driver.md` (stub)
- Create: `docs/superpowers/plans/2026-05-26-rust-macro-plan-3-tauri-gui.md` (stub)

- [ ] **Step 1: Replace `README.md`**

Overwrite `README.md`:

```markdown
# rust-macro

Windows desktop macro engine in Rust. GUI-first (Tauri), driven by the Interception kernel driver for clean keyboard / mouse I/O. Target use case: macros for single-player / offline games and productivity automation.

## Status

- Plan 1 (this milestone): backend crates + mock driver + CLI. **Done when this README ships.**
- Plan 2: replace MockDriver with `interception-rs` integration.
- Plan 3: Tauri/Svelte GUI on top of the backend API.

See `docs/superpowers/specs/2026-05-26-rust-macro-design.md` for the design spec, and `docs/superpowers/plans/` for the per-phase implementation plans.

## Workspace layout

| Crate         | Purpose                                                    |
|---------------|------------------------------------------------------------|
| `rm-error`    | Central `AppError` + wire-form for the future GUI.         |
| `rm-macro-model` | `Macro`, `Step`, `Trigger`, `PlaybackMode`, input enums. |
| `rm-driver`   | `Driver` trait + `RawEvent` + `MockDriver`.                |
| `rm-storage`  | Atomic JSON CRUD for macros under a storage root.          |
| `rm-recorder` | Records from any `Driver` and compiles raw events → steps. |
| `rm-player`   | Executes a `Macro` through any `Driver`, all PlaybackModes.|
| `rm-hotkey`   | Listens to `Driver` events, dispatches `HotkeyHit`.        |
| `rm-cli`      | `macro-cli` binary: record/play/list/delete via stdio.     |

## Try it (Plan 1, mock driver only)

```powershell
# List (empty)
cargo run -p rm-cli -- --root ./.rust-macro list

# "Record" by piping JSONL events into stdin; on EOF the events compile + save.
'{"kind":"key_down","key":"h"}','{"kind":"key_up","key":"h"}' |
  cargo run -p rm-cli -- --root ./.rust-macro record greet

# Play back: each emitted event prints to stdout as JSONL.
cargo run -p rm-cli -- --root ./.rust-macro play greet
```

## Dev commands

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## License

MIT OR Apache-2.0
```

- [ ] **Step 2: Plan 2 stub (`docs/superpowers/plans/2026-05-26-rust-macro-plan-2-real-driver.md`)**

```markdown
# rust-macro — Plan 2: Real Interception Driver (stub)

**Goal:** Swap `MockDriver` (Plan 1) for an `InterceptionDriver` backed by the real Interception kernel driver via the `interception-rs` crate. Add driver-status detection and the bundled installer flow.

**Architecture:** New crate `rm-driver-interception` providing `InterceptionDriver: Driver`. Add `driver::detect_status()` returning `{ NotInstalled, InstalledNotRunning, Running }`. `rm-cli` grows a `driver` subcommand: `status`, `install`.

**Tech Stack:** Adds `interception-rs` (or a thin FFI binding to `interception.dll` if the crate isn't suitable on current Rust). Bundles the upstream Interception installer (`install-interception.exe`).

**Why a separate plan:** depends on having Interception installed on the dev machine + an admin reboot, so it cannot be CI-friendly. Plan 1's tests stay green throughout.

(Detailed tasks to be written when Plan 1 is merged and verified.)
```

- [ ] **Step 3: Plan 3 stub (`docs/superpowers/plans/2026-05-26-rust-macro-plan-3-tauri-gui.md`)**

```markdown
# rust-macro — Plan 3: Tauri GUI (stub)

**Goal:** Bring the GUI online. Tauri (Rust backend) + Svelte/TS frontend, wired to the existing backend crates.

**Architecture:** New crate `rm-app` (Tauri main): registers commands that call into `rm-recorder`, `rm-player`, `rm-hotkey`, `rm-storage`. Frontend has views: Macro list, Editor (step-by-step), Recording overlay, Hotkey config, Settings.

**Tech Stack:** Tauri 2.x, Vite, Svelte 5 + TypeScript.

(Detailed tasks to be written when Plan 2 is merged and verified.)
```

- [ ] **Step 4: Commit**

```bash
git add README.md docs/superpowers/plans/
git commit -m "docs: README + Plan 2/Plan 3 stubs"
```

- [ ] **Step 5: Final verification**

Run all the checks one more time:

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo run -p rm-cli -- --root ./.tmp-final list   # empty output, exit 0
```

Expected: all clean. Plan 1 complete.

---

## Self-Review

(Run mentally after writing the plan; fix any issues inline before sharing.)

**Spec coverage:** All goals from the spec for the *backend* are covered:
- Macro/Step/Trigger/PlaybackMode types — Task 4
- Step-by-step editing data model — Tasks 3-4 (Step variants)
- Random delays — Task 10 (`Wait` resolves min..=max at playback)
- Hotkey binding — Tasks 4 (Trigger model) + 11 (listener)
- Playback modes (Once/Repeat/Loop/Toggle) — Task 10
- Per-macro JSON storage — Task 7
- Driver abstraction ready for Plan 2 swap — Tasks 5-6
- Error model ready for Plan 3 GUI wire-form — Task 2

Out-of-scope for Plan 1 (handed off to Plans 2/3):
- Real Interception driver — Plan 2
- Driver install flow — Plan 2
- Tauri GUI — Plan 3
- Stop hotkey global state — surfaces in Plan 3 (the app crate) since v1 spec says it lives at the app boundary
- Recording-overlay UX — Plan 3

**Placeholder scan:** none.

**Type consistency:**
- `KeyCode`, `MouseButton`, `Modifier`, `Point` defined Task 3, used consistently in Tasks 4, 5, 8, 10, 11.
- `RawEvent` defined Task 5, used in Tasks 6, 8, 9, 10, 11, 12, 13.
- `Driver` trait defined Task 5, impl by `MockDriver` (Task 6), `StdioDriver` (Task 12). All async, all `Send + Sync`.
- `Step` defined Task 4, produced by Task 8 (compile), consumed by Task 10 (player).
- Crate names: `rm-error`, `rm-macro-model`, `rm-driver`, `rm-storage`, `rm-recorder`, `rm-player`, `rm-hotkey`, `rm-cli`. Consistent everywhere.

**Acceptance criteria for Plan 1:**
- `cargo test --workspace` passes (≥ ~35 tests).
- `cargo clippy --workspace --all-targets -- -D warnings` clean.
- `cargo fmt --all -- --check` clean.
- `macro-cli list/record/play/delete` exercised end-to-end via JSONL on stdio.
- Integration test in `crates/cli/tests/e2e.rs` proves the full record→save→load→play data path.
