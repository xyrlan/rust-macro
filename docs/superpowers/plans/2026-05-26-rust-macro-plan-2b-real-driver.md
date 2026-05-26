# rust-macro — Plan 2b: Real Interception Driver — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a real keyboard/mouse driver backed by the Interception kernel driver, so the CLI can record and play back macros against real Windows applications (e.g. Notepad). Default CI build remains all-mock; everything Interception-specific is gated behind an opt-in `interception` Cargo feature on `rm-cli`.

**Architecture:** New workspace crate `rm-driver-interception` implements the existing `rm_driver::Driver` trait via the `kanata-interception` crate. A dedicated `std::thread` per `InterceptionDriver` runs `wait_with_timeout` and pumps decoded `RawEvent`s into a `tokio::sync::mpsc` channel that `Driver::recv()` (async) reads. `DriverHub` from Plan 2a is unchanged. `rm-cli` gains a `--driver {stdio|interception}` flag on `record`/`play` and a `driver status` subcommand, all gated by `--features interception`.

**Tech Stack:** Rust stable (MSVC toolchain), `kanata-interception = "0.3"`, `windows-sys` (Win32 service manager bindings), `tokio` (existing), `tracing` (existing). Target: Windows 10/11 x64.

**Spec:** `docs/superpowers/specs/2026-05-26-rust-macro-plan-2b-real-driver-design.md` (revision 2).

---

## File Structure

**Files to create:**
- `crates/driver-interception/Cargo.toml`
- `crates/driver-interception/src/lib.rs` — module decls + re-exports
- `crates/driver-interception/src/scancode.rs` — `KeyCode <-> (u16 scancode, bool e0)` bidirectional tables
- `crates/driver-interception/src/mouse.rs` — `StrokeEvents`, `convert_mouse`, `mouse_event_to_stroke`
- `crates/driver-interception/src/status.rs` — `DriverStatus`, `detect_status`, `ServiceQuery` trait + SCM impl
- `crates/driver-interception/src/driver.rs` — `InterceptionDriver`, OS pump thread, `Drop`, `Driver` impl
- `crates/driver-interception/tests/smoke.rs` — smoke tests gated by `--features smoke`, requires Interception installed
- `LICENSES.md` — repo-root doc on the LGPL-3.0 transitive dependency

**Files to modify:**
- `Cargo.toml` (repo root) — add `crates/driver-interception` to `[workspace.members]`; add `kanata-interception` and `windows-sys` to `[workspace.dependencies]`
- `crates/cli/Cargo.toml` — add `[features]` table with `interception = ["dep:rm-driver-interception"]`; add optional dependency
- `crates/cli/src/main.rs` — `DriverKind` enum, `Driver` subcommand (both feature-gated), pass `DriverKind` to commands
- `crates/cli/src/commands.rs` — `cmd_record` and `cmd_play` take `DriverKind`, add `open_interception` helper, add Ctrl+C stop branch

**Tasks decomposed by file boundary.** Each task produces one focused commit.

---

## Task 1: Add workspace dependencies and new crate member

**Files:**
- Modify: `Cargo.toml` (repo root)
- Create: `crates/driver-interception/` (empty directory, populated in Task 2)

- [ ] **Step 1: Add `crates/driver-interception` to workspace members and add `kanata-interception` + `windows-sys` to workspace deps**

Edit `Cargo.toml` (repo root). After the existing `[workspace.dependencies]` block, the file should look like:

```toml
[workspace]
resolver = "2"
members = [
    "crates/error",
    "crates/macro_model",
    "crates/driver",
    "crates/driver-interception",
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
kanata-interception = "0.3"
rand = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
tokio = { version = "1", features = ["full"] }
tokio-util = "0.7"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
uuid = { version = "1", features = ["v4", "serde"] }
windows-sys = { version = "0.59", features = [
    "Win32_System_Services",
    "Win32_Foundation",
] }

# dev
tempfile = "3"
```

- [ ] **Step 2: Verify workspace still parses (no crate exists yet, so `cargo check --workspace` will fail — verify only the parsing error message)**

Run: `cargo metadata --no-deps --format-version 1 1>$null`
Expected: error message mentioning `crates/driver-interception/Cargo.toml` does not exist. That's correct — Task 2 creates it.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore(workspace): add driver-interception member + kanata-interception/windows-sys deps"
```

---

## Task 2: Scaffold `rm-driver-interception` crate (empty)

**Files:**
- Create: `crates/driver-interception/Cargo.toml`
- Create: `crates/driver-interception/src/lib.rs`

- [ ] **Step 1: Write `crates/driver-interception/Cargo.toml`**

```toml
[package]
name = "rm-driver-interception"
version.workspace = true
edition.workspace = true

[features]
default = []
# Gates the live-driver smoke tests in tests/smoke.rs (require Interception
# installed on the host). Unit tests for the pure-Rust mapping logic always run.
smoke = []

[dependencies]
async-trait.workspace = true
kanata-interception.workspace = true
rm-driver = { path = "../driver" }
rm-macro-model = { path = "../macro_model" }
thiserror.workspace = true
tokio = { workspace = true, features = ["sync", "rt"] }
tracing.workspace = true
windows-sys.workspace = true

[dev-dependencies]
tokio = { workspace = true, features = ["macros", "rt", "rt-multi-thread", "sync", "time"] }
```

- [ ] **Step 2: Write `crates/driver-interception/src/lib.rs` (stub with module decls only)**

```rust
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
```

- [ ] **Step 3: Create empty module files so `cargo check` parses (replaced in later tasks)**

Create `crates/driver-interception/src/driver.rs` with:
```rust
// Implemented in Task 7.
```

Create `crates/driver-interception/src/mouse.rs` with:
```rust
// Implemented in Task 5.
```

Create `crates/driver-interception/src/scancode.rs` with:
```rust
// Implemented in Task 4.
```

Create `crates/driver-interception/src/status.rs` with:
```rust
// Implemented in Task 6.
```

- [ ] **Step 4: Update `lib.rs` to NOT re-export from empty modules yet**

Temporarily edit `crates/driver-interception/src/lib.rs` to remove the `pub use`s until the items exist:

```rust
//! Real-driver implementation of `rm_driver::Driver` backed by the Interception
//! kernel driver via the `kanata-interception` crate. Windows-only.
//!
//! See `docs/superpowers/specs/2026-05-26-rust-macro-plan-2b-real-driver-design.md`.

pub mod driver;
pub mod mouse;
pub mod scancode;
pub mod status;
```

The re-exports will be added back as their target items land (Tasks 6 and 7).

- [ ] **Step 5: Verify the crate compiles**

Run: `cargo check -p rm-driver-interception`
Expected: PASS (warnings about unused deps are OK at this point; will resolve in later tasks).

- [ ] **Step 6: Commit**

```bash
git add crates/driver-interception/
git commit -m "feat(driver-interception): scaffold crate with empty modules"
```

---

## Task 3: Add a `RawEvent` re-export sanity test

**Files:**
- Modify: `crates/driver-interception/src/lib.rs`

Purpose: prove the crate links against `rm-driver` and `rm-macro-model` correctly, before adding any real logic.

- [ ] **Step 1: Write the failing test**

Append to `crates/driver-interception/src/lib.rs`:

```rust
#[cfg(test)]
mod sanity {
    use rm_driver::RawEvent;
    use rm_macro_model::KeyCode;

    #[test]
    fn raw_event_constructible() {
        let _e = RawEvent::KeyDown { key: KeyCode::A };
    }
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p rm-driver-interception sanity::raw_event_constructible`
Expected: PASS (1 test).

- [ ] **Step 3: Commit**

```bash
git add crates/driver-interception/src/lib.rs
git commit -m "test(driver-interception): sanity that RawEvent re-exports link"
```

---

## Task 4: Scancode bidirectional table (`scancode.rs`)

**Files:**
- Modify: `crates/driver-interception/src/scancode.rs`

- [ ] **Step 1: Write the failing roundtrip test first**

Replace `crates/driver-interception/src/scancode.rs` content with the test only:

```rust
//! Bidirectional mapping between Windows Set 1 ("XT") scancodes and the
//! `rm_macro_model::KeyCode` enum. Reference: https://wiki.osdev.org/PS/2_Keyboard
//! and the Interception SDK header. The `e0` bool corresponds to Interception's
//! `KeyState::E0` flag — needed to distinguish e.g. LCtrl (0x1D, e0=false) from
//! RCtrl (0x1D, e0=true), and to identify extended cluster keys (arrows, Insert,
//! Delete, Home/End, PageUp/PageDown — all e0=true).

use rm_macro_model::KeyCode;

pub fn scancode_to_keycode(code: u16, e0: bool) -> Option<KeyCode> {
    // Implemented in Step 2.
    let _ = (code, e0);
    None
}

pub fn keycode_to_scancode(_k: KeyCode) -> (u16, bool) {
    // Implemented in Step 2.
    (0, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant of `KeyCode`. Update if the enum gains new variants.
    fn all_keycodes() -> Vec<KeyCode> {
        use KeyCode::*;
        vec![
            A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
            Num0, Num1, Num2, Num3, Num4, Num5, Num6, Num7, Num8, Num9,
            F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
            LShift, RShift, LCtrl, RCtrl, LAlt, RAlt, LWin, RWin,
            Space, Enter, Tab, Backspace, Escape, CapsLock,
            Up, Down, Left, Right,
            Insert, Delete, Home, End, PageUp, PageDown,
            Minus, Equals, LBracket, RBracket, Backslash, Semicolon,
            Apostrophe, Backtick, Comma, Period, Slash,
        ]
    }

    #[test]
    fn roundtrip_every_keycode() {
        for k in all_keycodes() {
            let (code, e0) = keycode_to_scancode(k);
            let back = scancode_to_keycode(code, e0);
            assert_eq!(back, Some(k), "roundtrip failed for {:?}", k);
        }
    }

    #[test]
    fn lctrl_and_rctrl_disambiguate_by_e0() {
        assert_eq!(scancode_to_keycode(0x1D, false), Some(KeyCode::LCtrl));
        assert_eq!(scancode_to_keycode(0x1D, true), Some(KeyCode::RCtrl));
        assert_eq!(keycode_to_scancode(KeyCode::LCtrl), (0x1D, false));
        assert_eq!(keycode_to_scancode(KeyCode::RCtrl), (0x1D, true));
    }

    #[test]
    fn arrows_are_e0_extended() {
        for k in [KeyCode::Up, KeyCode::Down, KeyCode::Left, KeyCode::Right] {
            let (_, e0) = keycode_to_scancode(k);
            assert!(e0, "{:?} must be e0-prefixed", k);
        }
    }

    #[test]
    fn unknown_scancode_returns_none() {
        assert_eq!(scancode_to_keycode(0x00, false), None);
        assert_eq!(scancode_to_keycode(0xFFFF, false), None);
    }
}
```

- [ ] **Step 2: Run tests — they fail**

Run: `cargo test -p rm-driver-interception scancode::tests`
Expected: FAIL — `roundtrip_every_keycode` panics on the first variant (`A` returns `None` from `scancode_to_keycode(0, false)`).

- [ ] **Step 3: Write the bidirectional table**

Replace the two stub functions in `crates/driver-interception/src/scancode.rs` with the real table. Replace the whole file (preserve the doc-comment block at the top and the `#[cfg(test)] mod tests` block at the bottom; only the two functions change):

```rust
pub fn scancode_to_keycode(code: u16, e0: bool) -> Option<KeyCode> {
    match (code, e0) {
        // Letter row (top, middle, bottom)
        (0x10, false) => Some(KeyCode::Q),
        (0x11, false) => Some(KeyCode::W),
        (0x12, false) => Some(KeyCode::E),
        (0x13, false) => Some(KeyCode::R),
        (0x14, false) => Some(KeyCode::T),
        (0x15, false) => Some(KeyCode::Y),
        (0x16, false) => Some(KeyCode::U),
        (0x17, false) => Some(KeyCode::I),
        (0x18, false) => Some(KeyCode::O),
        (0x19, false) => Some(KeyCode::P),
        (0x1E, false) => Some(KeyCode::A),
        (0x1F, false) => Some(KeyCode::S),
        (0x20, false) => Some(KeyCode::D),
        (0x21, false) => Some(KeyCode::F),
        (0x22, false) => Some(KeyCode::G),
        (0x23, false) => Some(KeyCode::H),
        (0x24, false) => Some(KeyCode::J),
        (0x25, false) => Some(KeyCode::K),
        (0x26, false) => Some(KeyCode::L),
        (0x2C, false) => Some(KeyCode::Z),
        (0x2D, false) => Some(KeyCode::X),
        (0x2E, false) => Some(KeyCode::C),
        (0x2F, false) => Some(KeyCode::V),
        (0x30, false) => Some(KeyCode::B),
        (0x31, false) => Some(KeyCode::N),
        (0x32, false) => Some(KeyCode::M),
        // Digit row (top of letters)
        (0x02, false) => Some(KeyCode::Num1),
        (0x03, false) => Some(KeyCode::Num2),
        (0x04, false) => Some(KeyCode::Num3),
        (0x05, false) => Some(KeyCode::Num4),
        (0x06, false) => Some(KeyCode::Num5),
        (0x07, false) => Some(KeyCode::Num6),
        (0x08, false) => Some(KeyCode::Num7),
        (0x09, false) => Some(KeyCode::Num8),
        (0x0A, false) => Some(KeyCode::Num9),
        (0x0B, false) => Some(KeyCode::Num0),
        // Function row
        (0x3B, false) => Some(KeyCode::F1),
        (0x3C, false) => Some(KeyCode::F2),
        (0x3D, false) => Some(KeyCode::F3),
        (0x3E, false) => Some(KeyCode::F4),
        (0x3F, false) => Some(KeyCode::F5),
        (0x40, false) => Some(KeyCode::F6),
        (0x41, false) => Some(KeyCode::F7),
        (0x42, false) => Some(KeyCode::F8),
        (0x43, false) => Some(KeyCode::F9),
        (0x44, false) => Some(KeyCode::F10),
        (0x57, false) => Some(KeyCode::F11),
        (0x58, false) => Some(KeyCode::F12),
        // Modifiers (Ctrl/Alt are E0-discriminated for L/R; Shift uses different codes)
        (0x2A, false) => Some(KeyCode::LShift),
        (0x36, false) => Some(KeyCode::RShift),
        (0x1D, false) => Some(KeyCode::LCtrl),
        (0x1D, true)  => Some(KeyCode::RCtrl),
        (0x38, false) => Some(KeyCode::LAlt),
        (0x38, true)  => Some(KeyCode::RAlt),
        (0x5B, true)  => Some(KeyCode::LWin),
        (0x5C, true)  => Some(KeyCode::RWin),
        // Whitespace + control
        (0x39, false) => Some(KeyCode::Space),
        (0x1C, false) => Some(KeyCode::Enter),
        (0x0F, false) => Some(KeyCode::Tab),
        (0x0E, false) => Some(KeyCode::Backspace),
        (0x01, false) => Some(KeyCode::Escape),
        (0x3A, false) => Some(KeyCode::CapsLock),
        // Arrows (all E0-extended)
        (0x48, true) => Some(KeyCode::Up),
        (0x50, true) => Some(KeyCode::Down),
        (0x4B, true) => Some(KeyCode::Left),
        (0x4D, true) => Some(KeyCode::Right),
        // Edit cluster (all E0-extended)
        (0x52, true) => Some(KeyCode::Insert),
        (0x53, true) => Some(KeyCode::Delete),
        (0x47, true) => Some(KeyCode::Home),
        (0x4F, true) => Some(KeyCode::End),
        (0x49, true) => Some(KeyCode::PageUp),
        (0x51, true) => Some(KeyCode::PageDown),
        // Punctuation (US layout)
        (0x0C, false) => Some(KeyCode::Minus),
        (0x0D, false) => Some(KeyCode::Equals),
        (0x1A, false) => Some(KeyCode::LBracket),
        (0x1B, false) => Some(KeyCode::RBracket),
        (0x2B, false) => Some(KeyCode::Backslash),
        (0x27, false) => Some(KeyCode::Semicolon),
        (0x28, false) => Some(KeyCode::Apostrophe),
        (0x29, false) => Some(KeyCode::Backtick),
        (0x33, false) => Some(KeyCode::Comma),
        (0x34, false) => Some(KeyCode::Period),
        (0x35, false) => Some(KeyCode::Slash),
        _ => None,
    }
}

pub fn keycode_to_scancode(k: KeyCode) -> (u16, bool) {
    match k {
        // Letters
        KeyCode::A => (0x1E, false), KeyCode::B => (0x30, false),
        KeyCode::C => (0x2E, false), KeyCode::D => (0x20, false),
        KeyCode::E => (0x12, false), KeyCode::F => (0x21, false),
        KeyCode::G => (0x22, false), KeyCode::H => (0x23, false),
        KeyCode::I => (0x17, false), KeyCode::J => (0x24, false),
        KeyCode::K => (0x25, false), KeyCode::L => (0x26, false),
        KeyCode::M => (0x32, false), KeyCode::N => (0x31, false),
        KeyCode::O => (0x18, false), KeyCode::P => (0x19, false),
        KeyCode::Q => (0x10, false), KeyCode::R => (0x13, false),
        KeyCode::S => (0x1F, false), KeyCode::T => (0x14, false),
        KeyCode::U => (0x16, false), KeyCode::V => (0x2F, false),
        KeyCode::W => (0x11, false), KeyCode::X => (0x2D, false),
        KeyCode::Y => (0x15, false), KeyCode::Z => (0x2C, false),
        // Digits
        KeyCode::Num0 => (0x0B, false), KeyCode::Num1 => (0x02, false),
        KeyCode::Num2 => (0x03, false), KeyCode::Num3 => (0x04, false),
        KeyCode::Num4 => (0x05, false), KeyCode::Num5 => (0x06, false),
        KeyCode::Num6 => (0x07, false), KeyCode::Num7 => (0x08, false),
        KeyCode::Num8 => (0x09, false), KeyCode::Num9 => (0x0A, false),
        // Function row
        KeyCode::F1 => (0x3B, false), KeyCode::F2 => (0x3C, false),
        KeyCode::F3 => (0x3D, false), KeyCode::F4 => (0x3E, false),
        KeyCode::F5 => (0x3F, false), KeyCode::F6 => (0x40, false),
        KeyCode::F7 => (0x41, false), KeyCode::F8 => (0x42, false),
        KeyCode::F9 => (0x43, false), KeyCode::F10 => (0x44, false),
        KeyCode::F11 => (0x57, false), KeyCode::F12 => (0x58, false),
        // Modifiers
        KeyCode::LShift => (0x2A, false), KeyCode::RShift => (0x36, false),
        KeyCode::LCtrl  => (0x1D, false), KeyCode::RCtrl  => (0x1D, true),
        KeyCode::LAlt   => (0x38, false), KeyCode::RAlt   => (0x38, true),
        KeyCode::LWin   => (0x5B, true),  KeyCode::RWin   => (0x5C, true),
        // Whitespace + control
        KeyCode::Space     => (0x39, false), KeyCode::Enter   => (0x1C, false),
        KeyCode::Tab       => (0x0F, false), KeyCode::Backspace => (0x0E, false),
        KeyCode::Escape    => (0x01, false), KeyCode::CapsLock => (0x3A, false),
        // Arrows
        KeyCode::Up    => (0x48, true), KeyCode::Down  => (0x50, true),
        KeyCode::Left  => (0x4B, true), KeyCode::Right => (0x4D, true),
        // Edit cluster
        KeyCode::Insert   => (0x52, true), KeyCode::Delete  => (0x53, true),
        KeyCode::Home     => (0x47, true), KeyCode::End     => (0x4F, true),
        KeyCode::PageUp   => (0x49, true), KeyCode::PageDown => (0x51, true),
        // Punctuation
        KeyCode::Minus      => (0x0C, false), KeyCode::Equals    => (0x0D, false),
        KeyCode::LBracket   => (0x1A, false), KeyCode::RBracket  => (0x1B, false),
        KeyCode::Backslash  => (0x2B, false), KeyCode::Semicolon => (0x27, false),
        KeyCode::Apostrophe => (0x28, false), KeyCode::Backtick  => (0x29, false),
        KeyCode::Comma      => (0x33, false), KeyCode::Period    => (0x34, false),
        KeyCode::Slash      => (0x35, false),
    }
}
```

- [ ] **Step 4: Run tests — they pass**

Run: `cargo test -p rm-driver-interception scancode::tests`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/driver-interception/src/scancode.rs
git commit -m "feat(driver-interception): bidirectional Set-1 scancode <-> KeyCode table"
```

---

## Task 5: Mouse stroke decomposition (`mouse.rs`)

**Files:**
- Modify: `crates/driver-interception/src/mouse.rs`

The actual `kanata-interception` `MouseState`/`MouseFlags` types and the `Stroke::Mouse` variant fields are referenced here. If the field names diverge from this plan during implementation (e.g. the crate calls it `BUTTON_4_DOWN` vs `BUTTON4_DOWN`), adjust the constant references to match the upstream API; the conversion logic itself is unchanged.

- [ ] **Step 1: Write `StrokeEvents` + tests first**

Replace `crates/driver-interception/src/mouse.rs` content:

```rust
//! Decompose Interception `MouseStroke`s into 0..N `RawEvent`s. One stroke can
//! carry multiple button bits + wheel + movement simultaneously; we emit in a
//! stable order: buttons (L, R, M, X1, X2), then wheel, then movement.

use rm_driver::RawEvent;
use rm_macro_model::MouseButton;

/// Decomposed events for a single Interception stroke. Returned by value to
/// avoid a heap allocation per event; consumers iterate `events.iter().flatten()`.
/// Sized at 6 to cover the worst case of a mouse stroke carrying every button
/// bit + wheel + move simultaneously (extremely rare but theoretically possible).
#[derive(Debug, Default, Clone, Copy)]
pub struct StrokeEvents {
    pub events: [Option<RawEvent>; 6],
}

impl StrokeEvents {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn iter(&self) -> impl Iterator<Item = RawEvent> + '_ {
        self.events.iter().filter_map(|o| *o)
    }
}

/// Inputs mirror the relevant fields from `kanata_interception::Stroke::Mouse`.
/// We accept the bit-flag types as plain integers so this module is testable
/// without depending on the Interception bitflag constants in unit tests.
///
/// Bit semantics (matches `interception.h`):
///   state bits — 0x01=L_DOWN, 0x02=L_UP, 0x04=R_DOWN, 0x08=R_UP,
///                0x10=M_DOWN, 0x20=M_UP, 0x40=B4_DOWN, 0x80=B4_UP,
///                0x100=B5_DOWN, 0x200=B5_UP, 0x400=WHEEL, 0x800=HWHEEL
///   flags bit  — 0x01=MOVE_RELATIVE (default), 0x02=MOVE_ABSOLUTE,
///                0x04=VIRTUAL_DESKTOP, 0x08=ATTRIBUTES_CHANGED,
///                0x10=MOVE_NOCOALESCE, 0x20=TERMSRV_SRC_SHADOW
pub fn convert_mouse(state: u16, flags: u16, rolling: i16, x: i32, y: i32) -> StrokeEvents {
    let mut out = StrokeEvents::empty();
    let mut n = 0usize;
    let mut push = |slot: &mut StrokeEvents, n: &mut usize, ev: RawEvent| {
        if *n < slot.events.len() {
            slot.events[*n] = Some(ev);
            *n += 1;
        }
    };

    // Buttons (left, right, middle, X1, X2 — down before up within each button).
    if state & 0x0001 != 0 { push(&mut out, &mut n, RawEvent::MouseDown { button: MouseButton::Left }); }
    if state & 0x0002 != 0 { push(&mut out, &mut n, RawEvent::MouseUp   { button: MouseButton::Left }); }
    if state & 0x0004 != 0 { push(&mut out, &mut n, RawEvent::MouseDown { button: MouseButton::Right }); }
    if state & 0x0008 != 0 { push(&mut out, &mut n, RawEvent::MouseUp   { button: MouseButton::Right }); }
    if state & 0x0010 != 0 { push(&mut out, &mut n, RawEvent::MouseDown { button: MouseButton::Middle }); }
    if state & 0x0020 != 0 { push(&mut out, &mut n, RawEvent::MouseUp   { button: MouseButton::Middle }); }
    if state & 0x0040 != 0 { push(&mut out, &mut n, RawEvent::MouseDown { button: MouseButton::X1 }); }
    if state & 0x0080 != 0 { push(&mut out, &mut n, RawEvent::MouseUp   { button: MouseButton::X1 }); }
    if state & 0x0100 != 0 { push(&mut out, &mut n, RawEvent::MouseDown { button: MouseButton::X2 }); }
    if state & 0x0200 != 0 { push(&mut out, &mut n, RawEvent::MouseUp   { button: MouseButton::X2 }); }

    // Vertical wheel (v1 — horizontal wheel deferred).
    if state & 0x0400 != 0 && rolling != 0 {
        push(&mut out, &mut n, RawEvent::MouseWheel { delta: rolling as i32 });
    }

    // Movement. `MOVE_ABSOLUTE` (flags & 0x02) is rare on raw hardware; if seen,
    // we log and pass through unchanged. RawEvent::MouseMove is relative by
    // definition.
    if x != 0 || y != 0 {
        if flags & 0x0002 != 0 {
            tracing::debug!(x, y, "interception: absolute mouse movement converted as relative");
        }
        push(&mut out, &mut n, RawEvent::MouseMove { dx: x, dy: y });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(s: &StrokeEvents) -> Vec<RawEvent> {
        s.iter().collect()
    }

    #[test]
    fn left_button_down_then_up() {
        let s = convert_mouse(0x0001 | 0x0002, 0, 0, 0, 0);
        assert_eq!(events(&s), vec![
            RawEvent::MouseDown { button: MouseButton::Left },
            RawEvent::MouseUp   { button: MouseButton::Left },
        ]);
    }

    #[test]
    fn each_button_bit_maps_correctly() {
        use MouseButton::*;
        let cases = [
            (0x0001, RawEvent::MouseDown { button: Left }),
            (0x0002, RawEvent::MouseUp   { button: Left }),
            (0x0004, RawEvent::MouseDown { button: Right }),
            (0x0008, RawEvent::MouseUp   { button: Right }),
            (0x0010, RawEvent::MouseDown { button: Middle }),
            (0x0020, RawEvent::MouseUp   { button: Middle }),
            (0x0040, RawEvent::MouseDown { button: X1 }),
            (0x0080, RawEvent::MouseUp   { button: X1 }),
            (0x0100, RawEvent::MouseDown { button: X2 }),
            (0x0200, RawEvent::MouseUp   { button: X2 }),
        ];
        for (state, expected) in cases {
            let s = convert_mouse(state, 0, 0, 0, 0);
            assert_eq!(events(&s), vec![expected], "state={:#06x}", state);
        }
    }

    #[test]
    fn wheel_emits_event_with_rolling_value() {
        let s = convert_mouse(0x0400, 0, 120, 0, 0);
        assert_eq!(events(&s), vec![RawEvent::MouseWheel { delta: 120 }]);
    }

    #[test]
    fn wheel_bit_without_rolling_emits_nothing() {
        let s = convert_mouse(0x0400, 0, 0, 0, 0);
        assert!(events(&s).is_empty());
    }

    #[test]
    fn zero_movement_emits_no_move_event() {
        let s = convert_mouse(0, 0, 0, 0, 0);
        assert!(events(&s).is_empty());
    }

    #[test]
    fn nonzero_movement_emits_relative_move() {
        let s = convert_mouse(0, 0x01, 0, 5, -3);
        assert_eq!(events(&s), vec![RawEvent::MouseMove { dx: 5, dy: -3 }]);
    }

    #[test]
    fn combined_button_wheel_and_move_emit_in_order() {
        // Left-down + wheel down + movement, all in one stroke.
        let s = convert_mouse(0x0001 | 0x0400, 0x01, -120, 10, 20);
        assert_eq!(events(&s), vec![
            RawEvent::MouseDown { button: MouseButton::Left },
            RawEvent::MouseWheel { delta: -120 },
            RawEvent::MouseMove { dx: 10, dy: 20 },
        ]);
    }
}
```

- [ ] **Step 2: Run tests — they pass**

Run: `cargo test -p rm-driver-interception mouse::tests`
Expected: PASS (7 tests).

- [ ] **Step 3: Commit**

```bash
git add crates/driver-interception/src/mouse.rs
git commit -m "feat(driver-interception): mouse stroke decomposition with stable event order"
```

---

## Task 6: `detect_status` with injectable `ServiceQuery`

**Files:**
- Modify: `crates/driver-interception/src/status.rs`
- Modify: `crates/driver-interception/src/lib.rs` (add the re-export back)

- [ ] **Step 1: Write `status.rs` with the trait abstraction + tests**

Replace `crates/driver-interception/src/status.rs`:

```rust
//! `detect_status()` — probe whether the Interception kernel driver is present
//! and running. Detection works in two layers: (1) try to open an Interception
//! context (definitive when it succeeds); (2) on failure, query the Windows
//! Service Control Manager for the two driver services to distinguish
//! NotInstalled from InstalledNotRunning.
//!
//! NOTE: The exact service names ("keyboard" and "mouse") must be verified
//! against a live Interception install before merge. If they differ, adjust
//! `INTERCEPTION_SERVICE_NAMES` and the test fixtures.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverStatus {
    /// Interception services are not present on the system.
    NotInstalled,
    /// Services exist but are not running (user-disabled or pending reboot).
    InstalledNotRunning,
    /// Services running and a context can be opened.
    Running,
}

/// The two driver services Oblitum's installer registers. **Verify before merge.**
const INTERCEPTION_SERVICE_NAMES: &[&str] = &["keyboard", "mouse"];

/// Outcome of `ServiceQuery::query_all`. Distilled to three cases that map
/// directly to `DriverStatus` when the context-open path fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    AllRunning,
    AllPresentSomeStopped,
    AnyMissing,
}

/// Abstracts the Windows SCM. Production impl talks to the real SCM via
/// `windows-sys`; tests inject a fake.
pub trait ServiceQuery {
    fn query_all(&self, names: &[&str]) -> ServiceState;
}

/// Probe Interception. `open_ctx` is a closure that attempts to open a context
/// and returns true on success — injected so this function is testable without
/// the real driver.
pub fn detect_status_with<F, S>(open_ctx: F, services: &S) -> DriverStatus
where
    F: FnOnce() -> bool,
    S: ServiceQuery,
{
    if open_ctx() {
        return DriverStatus::Running;
    }
    match services.query_all(INTERCEPTION_SERVICE_NAMES) {
        ServiceState::AllRunning            => DriverStatus::Running,
        ServiceState::AllPresentSomeStopped => DriverStatus::InstalledNotRunning,
        ServiceState::AnyMissing            => DriverStatus::NotInstalled,
    }
}

/// Public entry point: live SCM + live Interception context open.
pub fn detect_status() -> DriverStatus {
    detect_status_with(try_open_real_context, &Scm)
}

fn try_open_real_context() -> bool {
    // `Interception::new()` returns Option<Interception>. We additionally wrap
    // in catch_unwind defensively — the FFI shouldn't panic, but unknown C
    // boundary.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        kanata_interception::Interception::new().is_some()
    }))
    .unwrap_or(false)
}

/// Real Windows SCM-backed implementation.
struct Scm;

impl ServiceQuery for Scm {
    fn query_all(&self, names: &[&str]) -> ServiceState {
        scm::query(names)
    }
}

mod scm {
    use super::ServiceState;
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::ERROR_SERVICE_DOES_NOT_EXIST;
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus,
        SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_STATUS,
    };

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(once(0)).collect()
    }

    pub fn query(names: &[&str]) -> ServiceState {
        // SAFETY: Win32 API calls — pointers are valid for the lifetime of
        // each call, handles are explicitly closed.
        unsafe {
            let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
            if scm == 0 {
                tracing::debug!(error = std::io::Error::last_os_error().to_string(),
                    "OpenSCManagerW failed; assuming services absent");
                return ServiceState::AnyMissing;
            }
            let mut all_running = true;
            for name in names {
                let w = wide(name);
                let svc = OpenServiceW(scm, w.as_ptr(), SERVICE_QUERY_STATUS);
                if svc == 0 {
                    let err = std::io::Error::last_os_error();
                    let code = err.raw_os_error().unwrap_or(0) as u32;
                    if code == ERROR_SERVICE_DOES_NOT_EXIST {
                        CloseServiceHandle(scm);
                        return ServiceState::AnyMissing;
                    }
                    tracing::debug!(service = name, ?err, "OpenServiceW failed; treating as missing");
                    CloseServiceHandle(scm);
                    return ServiceState::AnyMissing;
                }
                let mut st: SERVICE_STATUS = std::mem::zeroed();
                let ok = QueryServiceStatus(svc, &mut st);
                CloseServiceHandle(svc);
                if ok == 0 || st.dwCurrentState != SERVICE_RUNNING {
                    all_running = false;
                }
            }
            CloseServiceHandle(scm);
            if all_running { ServiceState::AllRunning } else { ServiceState::AllPresentSomeStopped }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeServices(ServiceState);
    impl ServiceQuery for FakeServices {
        fn query_all(&self, _names: &[&str]) -> ServiceState { self.0 }
    }

    #[test]
    fn open_succeeds_returns_running_regardless_of_services() {
        let s = detect_status_with(|| true, &FakeServices(ServiceState::AnyMissing));
        assert_eq!(s, DriverStatus::Running);
    }

    #[test]
    fn open_fails_services_missing_returns_not_installed() {
        let s = detect_status_with(|| false, &FakeServices(ServiceState::AnyMissing));
        assert_eq!(s, DriverStatus::NotInstalled);
    }

    #[test]
    fn open_fails_services_present_but_stopped_returns_installed_not_running() {
        let s = detect_status_with(|| false, &FakeServices(ServiceState::AllPresentSomeStopped));
        assert_eq!(s, DriverStatus::InstalledNotRunning);
    }

    #[test]
    fn open_fails_but_services_all_running_returns_running() {
        // Race window: SCM reports running but context-open lost a race.
        // We trust SCM in that case.
        let s = detect_status_with(|| false, &FakeServices(ServiceState::AllRunning));
        assert_eq!(s, DriverStatus::Running);
    }
}
```

- [ ] **Step 2: Re-add the `status` re-export in `lib.rs`**

Edit `crates/driver-interception/src/lib.rs`:

```rust
//! Real-driver implementation of `rm_driver::Driver` backed by the Interception
//! kernel driver via the `kanata-interception` crate. Windows-only.
//!
//! See `docs/superpowers/specs/2026-05-26-rust-macro-plan-2b-real-driver-design.md`.

pub mod driver;
pub mod mouse;
pub mod scancode;
pub mod status;

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
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p rm-driver-interception status::tests`
Expected: PASS (4 tests).

Run also `cargo test -p rm-driver-interception` (all tests in the crate).
Expected: PASS (all tests from Tasks 3, 4, 5, 6 — total 16 tests).

- [ ] **Step 4: Commit**

```bash
git add crates/driver-interception/src/status.rs crates/driver-interception/src/lib.rs
git commit -m "feat(driver-interception): detect_status with injectable ServiceQuery + SCM impl"
```

---

## Task 7: `InterceptionDriver` (Driver trait impl + OS pump thread)

**Files:**
- Modify: `crates/driver-interception/src/driver.rs`
- Modify: `crates/driver-interception/src/lib.rs` (add the re-export back)

This is the largest task — it's one cohesive unit (driver type + thread + Drop must all land together; the type doesn't compile otherwise). No unit tests live in this file; coverage is via the smoke test (Task 8) and the scancode/mouse unit tests already in place.

- [ ] **Step 1: Write `crates/driver-interception/src/driver.rs`**

```rust
//! `InterceptionDriver` — implements the `rm_driver::Driver` trait by bridging
//! Interception's blocking `wait_with_timeout` to async via a dedicated OS
//! thread + `tokio::sync::mpsc` channel.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kanata_interception::{Interception, MouseFlags, MouseState, Stroke};
use rm_driver::{Driver, DriverError, RawEvent};
use rm_macro_model::{KeyCode, MouseButton};
use tokio::sync::{mpsc, Mutex as AsyncMutex};

use crate::mouse::convert_mouse;
use crate::scancode::{keycode_to_scancode, scancode_to_keycode};

/// Maximum strokes returned per `receive()` call. Interception buffers events
/// per device; reading 32 at a time keeps the OS thread responsive without
/// reallocating on every wake-up.
const RECEIVE_BATCH: usize = 32;

/// How long the OS thread blocks in `wait_with_timeout` between shutdown
/// polls. Bounds the worst-case driver-drop latency to ~100ms.
const WAIT_SLICE: Duration = Duration::from_millis(100);

/// Newtype that asserts Send + Sync on `Interception`. SAFETY: per oblitum's
/// Interception README and `interception.h`, all context-bound functions
/// (`interception_send`, `interception_wait`, `interception_receive`) are safe
/// across threads given a single context. `kanata-interception` does not declare
/// these traits because the struct contains a raw pointer. We rely on the C-side
/// guarantee.
struct InterceptionCtx(Interception);
unsafe impl Send for InterceptionCtx {}
unsafe impl Sync for InterceptionCtx {}

pub struct InterceptionDriver {
    ctx: Arc<InterceptionCtx>,
    event_rx: AsyncMutex<mpsc::UnboundedReceiver<RawEvent>>,
    shutdown: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl InterceptionDriver {
    /// Open an Interception context, install filters for all keyboard + mouse
    /// devices, spawn the OS pump thread, and return the driver. Returns an
    /// error if the context cannot be opened (driver not installed / DLL
    /// missing / etc).
    pub fn new() -> Result<Self, DriverError> {
        let raw = Interception::new()
            .ok_or_else(|| DriverError::Unavailable("Interception::new() returned None".into()))?;

        // Filter everything from all keyboard + mouse devices. `Filter::all()`
        // sets the bitmask to capture every event kind for the targeted device
        // class.
        raw.set_filter(
            kanata_interception::is_keyboard,
            kanata_interception::Filter::KeyFilter(kanata_interception::KeyFilter::all()),
        );
        raw.set_filter(
            kanata_interception::is_mouse,
            kanata_interception::Filter::MouseFilter(kanata_interception::MouseFilter::all()),
        );

        let ctx = Arc::new(InterceptionCtx(raw));
        let (tx, rx) = mpsc::unbounded_channel();
        let shutdown = Arc::new(AtomicBool::new(false));

        let thread_ctx = ctx.clone();
        let thread_shutdown = shutdown.clone();
        let thread = std::thread::Builder::new()
            .name("interception-pump".into())
            .spawn(move || pump(thread_ctx, tx, thread_shutdown))
            .map_err(|e| DriverError::Io(format!("spawn pump thread: {e}")))?;

        Ok(Self {
            ctx,
            event_rx: AsyncMutex::new(rx),
            shutdown,
            thread: Some(thread),
        })
    }
}

#[async_trait]
impl Driver for InterceptionDriver {
    async fn send(&self, event: RawEvent) -> Result<(), DriverError> {
        let (device, stroke) = match event_to_stroke(event) {
            Some(pair) => pair,
            None => {
                tracing::debug!(?event, "interception: unmapped RawEvent dropped on send");
                return Ok(());
            }
        };
        // `interception_send` is per-context thread-safe; concurrent &self
        // callers serialize at the C boundary, not in our wrapper.
        let sent = self.ctx.0.send(device, &[stroke]);
        if sent == 0 {
            return Err(DriverError::Io("interception_send wrote 0 strokes".into()));
        }
        Ok(())
    }

    async fn recv(&self) -> Result<RawEvent, DriverError> {
        let mut rx = self.event_rx.lock().await;
        rx.recv().await.ok_or(DriverError::Closed)
    }
}

impl Drop for InterceptionDriver {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // Interception context drops here, releasing the kernel handles.
    }
}

fn pump(
    ctx: Arc<InterceptionCtx>,
    event_tx: mpsc::UnboundedSender<RawEvent>,
    shutdown: Arc<AtomicBool>,
) {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        let device = ctx.0.wait_with_timeout(WAIT_SLICE);
        if device == 0 {
            continue; // timeout — loop back to shutdown check
        }
        let mut buf = [Stroke::Keyboard {
            code: 0.into(),
            state: kanata_interception::KeyState::empty(),
            information: 0,
        }; RECEIVE_BATCH];
        let n = ctx.0.receive(device, &mut buf);
        if n <= 0 {
            continue;
        }
        for stroke in &buf[..n as usize] {
            for ev in convert_stroke(*stroke).iter() {
                if event_tx.send(ev).is_err() {
                    return; // receiver dropped; exit cleanly
                }
            }
        }
    }
    // Drop event_tx implicitly on return — rx.recv() resolves to None,
    // Driver::recv returns DriverError::Closed, hub propagates Closed
    // to subscribers (same path Plan 2a's tests exercise).
}

fn convert_stroke(s: Stroke) -> crate::mouse::StrokeEvents {
    use kanata_interception::KeyState;
    match s {
        Stroke::Keyboard { code, state, .. } => {
            // Drop TermSrv flags (terminal server proxying — not modeled).
            if state.intersects(
                KeyState::TERMSRV_SET_LED | KeyState::TERMSRV_SHADOW | KeyState::TERMSRV_VKPACKET,
            ) {
                return crate::mouse::StrokeEvents::empty();
            }
            // Drop E1 (Pause prefix — keyboard pause is a multi-stroke E1
            // sequence we don't model in v1).
            if state.intersects(KeyState::E1) {
                return crate::mouse::StrokeEvents::empty();
            }
            let is_up = state.intersects(KeyState::UP);
            let is_e0 = state.intersects(KeyState::E0);
            let mut out = crate::mouse::StrokeEvents::empty();
            match scancode_to_keycode(code as u16, is_e0) {
                Some(key) if is_up => out.events[0] = Some(RawEvent::KeyUp { key }),
                Some(key) => out.events[0] = Some(RawEvent::KeyDown { key }),
                None => {
                    tracing::debug!(scancode = code as u16, e0 = is_e0,
                        "interception: unmapped scancode dropped");
                }
            }
            out
        }
        Stroke::Mouse { state, flags, rolling, x, y, .. } => {
            convert_mouse(state.bits() as u16, flags.bits() as u16, rolling, x, y)
        }
    }
}

/// Inverse of `convert_stroke` for a single `RawEvent`. Returns the target
/// device kind + the stroke to send. Returns `None` for events we can't
/// represent (and will be debug-logged + dropped by the caller).
fn event_to_stroke(event: RawEvent) -> Option<(kanata_interception::Device, Stroke)> {
    use kanata_interception::{Device, KeyState};
    match event {
        RawEvent::KeyDown { key } | RawEvent::KeyUp { key } => {
            let (code, e0) = keycode_to_scancode(key);
            let mut state = KeyState::empty();
            if matches!(event, RawEvent::KeyUp { .. }) {
                state |= KeyState::UP;
            }
            if e0 {
                state |= KeyState::E0;
            }
            Some((
                Device::Keyboard(1), // device 1 — send to the first keyboard
                Stroke::Keyboard {
                    code: (code as u16).into(),
                    state,
                    information: 0,
                },
            ))
        }
        RawEvent::MouseDown { button } | RawEvent::MouseUp { button } => {
            let down = matches!(event, RawEvent::MouseDown { .. });
            let state = mouse_button_to_state(button, down);
            Some((
                Device::Mouse(1),
                Stroke::Mouse {
                    state,
                    flags: MouseFlags::empty(),
                    rolling: 0,
                    x: 0,
                    y: 0,
                    information: 0,
                },
            ))
        }
        RawEvent::MouseMove { dx, dy } => Some((
            Device::Mouse(1),
            Stroke::Mouse {
                state: MouseState::empty(),
                flags: MouseFlags::MOVE_RELATIVE,
                rolling: 0,
                x: dx,
                y: dy,
                information: 0,
            },
        )),
        RawEvent::MouseWheel { delta } => Some((
            Device::Mouse(1),
            Stroke::Mouse {
                state: MouseState::WHEEL,
                flags: MouseFlags::empty(),
                rolling: delta as i16,
                x: 0,
                y: 0,
                information: 0,
            },
        )),
    }
}

fn mouse_button_to_state(b: MouseButton, down: bool) -> MouseState {
    match (b, down) {
        (MouseButton::Left,   true)  => MouseState::LEFT_BUTTON_DOWN,
        (MouseButton::Left,   false) => MouseState::LEFT_BUTTON_UP,
        (MouseButton::Right,  true)  => MouseState::RIGHT_BUTTON_DOWN,
        (MouseButton::Right,  false) => MouseState::RIGHT_BUTTON_UP,
        (MouseButton::Middle, true)  => MouseState::MIDDLE_BUTTON_DOWN,
        (MouseButton::Middle, false) => MouseState::MIDDLE_BUTTON_UP,
        (MouseButton::X1,     true)  => MouseState::BUTTON_4_DOWN,
        (MouseButton::X1,     false) => MouseState::BUTTON_4_UP,
        (MouseButton::X2,     true)  => MouseState::BUTTON_5_DOWN,
        (MouseButton::X2,     false) => MouseState::BUTTON_5_UP,
    }
}

// Imports referenced by the function above but unused if only KeyCode/MouseButton
// are touched at the top — keep this file self-contained at link time.
#[allow(dead_code)]
fn _imports_anchor(_: KeyCode, _: MouseButton) {}
```

(Note: The exact symbol names from `kanata-interception` — `KeyFilter::all()`, `MouseFilter::all()`, `is_keyboard`, `is_mouse`, `MouseState::WHEEL`, `MouseFlags::MOVE_RELATIVE`, `MouseState::LEFT_BUTTON_DOWN` etc. — are based on the `interception.h` constants the crate exposes. If the crate uses a slightly different name (e.g. `KeyFilter::ALL` vs `KeyFilter::all()`), substitute the equivalent at implementation time. The logic is unchanged.)

- [ ] **Step 2: Re-add the `driver` re-export in `lib.rs`**

Replace `crates/driver-interception/src/lib.rs`:

```rust
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
```

- [ ] **Step 3: Compile-check the crate**

Run: `cargo check -p rm-driver-interception`
Expected: PASS. Any error here is an upstream API-naming mismatch with `kanata-interception 0.3` — resolve by reading the crate's docs.rs page and adjusting symbol names. The structure of the code is unchanged.

- [ ] **Step 4: Run all unit tests still pass**

Run: `cargo test -p rm-driver-interception`
Expected: PASS — same 16 tests as after Task 6 (driver.rs has no unit tests of its own; smoke tests come in Task 8).

- [ ] **Step 5: Commit**

```bash
git add crates/driver-interception/src/driver.rs crates/driver-interception/src/lib.rs
git commit -m "feat(driver-interception): InterceptionDriver with OS pump thread"
```

---

## Task 8: Smoke test (manual — gated behind `--features smoke`)

**Files:**
- Create: `crates/driver-interception/tests/smoke.rs`

- [ ] **Step 1: Write the smoke test**

```rust
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
```

- [ ] **Step 2: Verify the test compiles under the feature**

Run: `cargo check -p rm-driver-interception --features smoke --tests`
Expected: PASS.

Run also: `cargo test -p rm-driver-interception` (without `--features smoke`).
Expected: PASS — smoke tests are gated out; only the 16 unit tests run.

- [ ] **Step 3: Commit**

```bash
git add crates/driver-interception/tests/smoke.rs
git commit -m "test(driver-interception): smoke tests gated behind --features smoke"
```

---

## Task 9: `rm-cli` feature flag + optional dep

**Files:**
- Modify: `crates/cli/Cargo.toml`

- [ ] **Step 1: Add the optional dep and feature**

Replace `crates/cli/Cargo.toml`:

```toml
[package]
name = "rm-cli"
version.workspace = true
edition.workspace = true

[[bin]]
name = "macro-cli"
path = "src/main.rs"

[features]
default = []
# Opt-in: pulls in rm-driver-interception and enables the --driver / `driver`
# CLI surface. Requires interception.dll on PATH at runtime.
interception = ["dep:rm-driver-interception"]

[dependencies]
anyhow.workspace = true
async-trait.workspace = true
clap.workspace = true
dirs = "5"
rm-driver = { path = "../driver" }
rm-driver-interception = { path = "../driver-interception", optional = true }
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

[dev-dependencies]
tempfile.workspace = true
```

- [ ] **Step 2: Verify both feature configurations parse**

Run: `cargo check -p rm-cli`
Expected: PASS — default build, no `rm-driver-interception`.

Run: `cargo check -p rm-cli --features interception`
Expected: PASS — pulls in `rm-driver-interception` as a real dep.

- [ ] **Step 3: Commit**

```bash
git add crates/cli/Cargo.toml
git commit -m "feat(cli): add opt-in `interception` feature gating rm-driver-interception"
```

---

## Task 10: Modify `cmd_record` and `cmd_play` signatures to accept `DriverKind`

**Files:**
- Modify: `crates/cli/src/commands.rs`
- Modify: `crates/cli/src/main.rs`

- [ ] **Step 1: Add `DriverKind` enum + update `cmd_record` / `cmd_play` in `commands.rs`**

Replace `crates/cli/src/commands.rs`:

```rust
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
```

- [ ] **Step 2: Update `main.rs` to thread `DriverKind` through and add `Driver` subcommand**

Replace `crates/cli/src/main.rs`:

```rust
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use rm_error::Result;
use tracing_subscriber::EnvFilter;

mod commands;
mod stdio_driver;

use commands::DriverKind;

#[derive(Parser)]
#[command(name = "macro-cli", version)]
struct Cli {
    /// Storage root. Defaults to `<data_dir>/rust-macro` (e.g. on Windows,
    /// `%APPDATA%/rust-macro`). Matches what the Tauri app will use in Plan 3.
    #[arg(long, global = true)]
    root: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Record events from the selected driver and save under `name`.
    Record {
        name: String,
        #[cfg(feature = "interception")]
        #[arg(long, value_enum, default_value_t = DriverKindArg::Stdio)]
        driver: DriverKindArg,
    },
    /// Play the macro named `name` via the selected driver.
    Play {
        name: String,
        #[cfg(feature = "interception")]
        #[arg(long, value_enum, default_value_t = DriverKindArg::Stdio)]
        driver: DriverKindArg,
    },
    /// List all saved macros.
    List,
    /// Delete the macro named `name`.
    Delete { name: String },
    /// Interception driver utilities (status / install instructions).
    #[cfg(feature = "interception")]
    Driver {
        #[command(subcommand)]
        sub: DriverCmd,
    },
}

#[cfg(feature = "interception")]
#[derive(Subcommand)]
enum DriverCmd {
    /// Print Interception driver status.
    Status,
}

#[cfg(feature = "interception")]
#[derive(clap::ValueEnum, Clone, Copy, Debug)]
enum DriverKindArg {
    Stdio,
    Interception,
}

#[cfg(feature = "interception")]
impl From<DriverKindArg> for DriverKind {
    fn from(d: DriverKindArg) -> Self {
        match d {
            DriverKindArg::Stdio => DriverKind::Stdio,
            DriverKindArg::Interception => DriverKind::Interception,
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let root = cli.root.unwrap_or_else(|| {
        dirs::data_dir()
            .map(|d| d.join("rust-macro"))
            .unwrap_or_else(|| PathBuf::from("./.rust-macro"))
    });

    let res: Result<()> = match cli.cmd {
        Cmd::Record {
            name,
            #[cfg(feature = "interception")]
            driver,
        } => {
            #[cfg(feature = "interception")]
            let kind = driver.into();
            #[cfg(not(feature = "interception"))]
            let kind = DriverKind::Stdio;
            commands::cmd_record(&root, &name, kind).await
        }
        Cmd::Play {
            name,
            #[cfg(feature = "interception")]
            driver,
        } => {
            #[cfg(feature = "interception")]
            let kind = driver.into();
            #[cfg(not(feature = "interception"))]
            let kind = DriverKind::Stdio;
            commands::cmd_play(&root, &name, kind).await
        }
        Cmd::List => commands::cmd_list(&root),
        Cmd::Delete { name } => commands::cmd_delete(&root, &name),
        #[cfg(feature = "interception")]
        Cmd::Driver { sub } => match sub {
            DriverCmd::Status => commands::cmd_driver_status(),
        },
    };
    res.map_err(|e| anyhow::anyhow!("{e}"))
}
```

- [ ] **Step 3: Verify both feature configurations compile**

Run: `cargo check -p rm-cli`
Expected: PASS — default build, no `--driver` arg in CLI.

Run: `cargo check -p rm-cli --features interception`
Expected: PASS — `--driver` arg + `Driver` subcommand wired in.

- [ ] **Step 4: Run the existing CLI e2e test (no feature) to confirm no regression**

Run: `cargo test -p rm-cli`
Expected: PASS — same `record_save_load_play_roundtrip` + `concurrent_recorder_and_hotkey_share_hub` tests as before, with no behavioral change.

- [ ] **Step 5: Commit**

```bash
git add crates/cli/src/commands.rs crates/cli/src/main.rs
git commit -m "feat(cli): --driver flag and `driver status` subcommand (feature-gated)"
```

---

## Task 11: Repo-root `LICENSES.md` for LGPL-3.0 transitive dep

**Files:**
- Create: `LICENSES.md`

- [ ] **Step 1: Write `LICENSES.md`**

```markdown
# Licenses

This project is licensed under **MIT OR Apache-2.0** (see `Cargo.toml`).

## Third-party notices

### Interception (transitive, behind the `interception` Cargo feature)

When built with `--features interception`, this project depends on
`kanata-interception` (BSL-1.0), which in turn depends on `interception-sys`
(**LGPL-3.0**). `interception-sys` dynamically loads `interception.dll` from
the [Interception kernel driver project](https://github.com/oblitum/Interception)
at runtime.

**LGPL-3.0 implications.** Static linking from this project's binaries into
`interception-sys` (the Rust FFI wrapper) inherits LGPL obligations. If
distributing binaries built with `--features interception`, the LGPL requires
either:

1. Providing the source of `interception-sys` (it's freely available on
   [crates.io](https://crates.io/crates/interception-sys) and via
   `cargo vendor`), AND
2. Distributing in a form that allows the user to relink against a modified
   version of the LGPL component (for Rust, this typically means shipping
   the object files of the LGPL crate or making the build process reproducible
   from public sources).

For development and personal use, no distribution obligation applies.
For binary distribution, revisit the obligations or switch to a custom thin
FFI binding (option B in the design spec).

### Other deps

Workspace `Cargo.toml` lists all direct dependencies. Run `cargo license`
for a current full enumeration.
```

- [ ] **Step 2: Commit**

```bash
git add LICENSES.md
git commit -m "docs: LICENSES.md documenting LGPL-3.0 obligation behind interception feature"
```

---

## Task 12: Replace stub plan with this plan file

**Files:**
- Modify: `docs/superpowers/plans/2026-05-26-rust-macro-plan-2b-real-driver.md` (already replaced as part of writing-plans output)

- [ ] **Step 1: Verify the stub is gone**

The file already contains this implementation plan (overwrote the stub in the writing-plans output). Verify:

```bash
head -3 docs/superpowers/plans/2026-05-26-rust-macro-plan-2b-real-driver.md
```

Expected: starts with `# rust-macro — Plan 2b: Real Interception Driver — Implementation Plan`.

- [ ] **Step 2: Stage and verify the replacement is the only diff on the plan file**

Run: `git status docs/superpowers/plans/`
Expected: `2026-05-26-rust-macro-plan-2b-real-driver.md` modified.

- [ ] **Step 3: Commit the plan replacement**

```bash
git add docs/superpowers/plans/2026-05-26-rust-macro-plan-2b-real-driver.md
git commit -m "docs(plans): replace Plan 2b stub with full implementation plan"
```

---

## Task 13: Final verification (no code change)

- [ ] **Step 1: Workspace passes without the feature**

Run: `cargo test --workspace`
Expected: PASS — 60+ existing tests + 16 new tests from rm-driver-interception (which compile without the feature; only smoke tests are gated).

Wait — that's not right. `cargo test --workspace` will try to build every crate including `rm-driver-interception`. Verify it builds:

Run: `cargo test --workspace --no-fail-fast`
Expected: all crate tests pass. New count: ~76 tests (60 from Plan 2a + 16 from rm-driver-interception).

- [ ] **Step 2: Feature-gated paths compile**

Run: `cargo check --workspace --features rm-cli/interception`
Expected: PASS.

- [ ] **Step 3: Document manual demo prerequisites in working memory**

(No file change.) Confirm the dev machine has:
1. Interception driver installed (per https://github.com/oblitum/Interception/releases).
2. Reboot completed after install.
3. `interception.dll` reachable on PATH (the installer places it in `C:\Windows\System32\`).

Then run the manual demo from the spec ("Manual demo test plan (Notepad)"):

```powershell
cargo run --features interception -- driver status
# expect: Interception driver: Running.

cargo run --features interception -- record demo --driver interception
# open Notepad, type "hello world", return to terminal, Ctrl+C
# expect: stopping... saved demo (<uuid>)

cargo run --features interception -- list
# expect: <uuid>  demo  steps=N

# Place cursor in Notepad on a fresh line, then:
cargo run --features interception -- play demo --driver interception
# expect: "hello world" (possibly + trailing ^C chars) appears in Notepad
```

If the demo passes, Plan 2b is shipped.

---

## Acceptance Checklist (from the spec)

- [ ] `cargo test --workspace` (no features) is green.
- [ ] `cargo check --workspace --features rm-cli/interception` is green.
- [ ] `cargo test -p rm-driver-interception` is green locally (16 tests).
- [ ] Smoke test `cargo test -p rm-driver-interception --features smoke` passes on the dev machine with Interception installed.
- [ ] Manual Notepad demo passes end-to-end.
- [ ] `LICENSES.md` exists at repo root.
- [ ] The stale comment in `crates/macro_model/src/input.rs` is updated (done in the spec commit, before this plan started).

---

## Open Implementation Notes

- **Service-name verification.** Task 6's `INTERCEPTION_SERVICE_NAMES = &["keyboard", "mouse"]` is the spec's best guess. Verify against a live install (run `sc query keyboard` and `sc query mouse` after Interception is installed) before Task 13's manual demo. If names differ, edit the constant and re-run scancode tests (no test depends on the names directly; only the live `detect_status` does).
- **`kanata-interception` symbol names.** Tasks 7 and 5 use names like `Filter::KeyFilter`, `MouseState::WHEEL`, `KeyFilter::all()`. Cross-check against `https://docs.rs/kanata-interception/0.3` at implementation time and adjust if the casing or path differs. The logic does not change.
- **DLL on PATH at test-runtime.** `cargo test -p rm-driver-interception` builds a test binary that links against `interception.dll`. On dev machines with Interception installed, the DLL is on PATH (System32) and tests run. On CI (no Interception), `cargo test --workspace` may fail to launch the test binary. Mitigation: only enable the `rm-driver-interception` member in `cargo test --workspace` runs after verifying CI is on Windows + has the DLL via the `interception-sys` vendored copy. If this turns out to be a real issue, the fix is `cargo test --workspace --exclude rm-driver-interception` in CI, with a separate `cargo check` job verifying the crate compiles.
