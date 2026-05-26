# rust-macro — Plan 2b: Real Interception Driver (design)

**Date:** 2026-05-26
**Status:** Approved (brainstorming phase, revision 2 after engineering-review-plan: C1/C2 + W1 + S1 applied)
**Supersedes:** the stub `plans/2026-05-26-rust-macro-plan-2b-real-driver.md` (replaced in-place once writing-plans runs).
**Parent spec:** `specs/2026-05-26-rust-macro-design.md`
**Builds on:** `specs/2026-05-26-rust-macro-plan-2a-driverhub-design.md` (DriverHub, consumer refactor) — Plan 2a is shipped.

## Summary

Plan 2b adds a real keyboard/mouse driver backed by the [Interception](https://github.com/oblitum/Interception) kernel driver, via the `kanata-interception` crate. It introduces a new workspace member `rm-driver-interception` that implements the existing `Driver` trait; adds a `detect_status()` function plus a CLI `driver status` subcommand; gates everything Interception-specific behind a `interception` Cargo feature so the default build and CI stay all-mock; and threads a `--driver {stdio|interception}` flag through `record` and `play`. The shippable artifact is the **Notepad record/play demo**: install Interception on Windows, run `macro-cli --driver interception record demo`, type something in Notepad, hit `Ctrl+C`, then `macro-cli --driver interception play demo` and watch the same text appear.

**Out of scope** (deferred to later plans): bundling `install-interception.exe`, programmatic UAC install, per-device filtering UX, GUI driver modal.

## Motivation

Plan 1 ships a clean backend pipeline with `MockDriver` / `StdioDriver`. Plan 2a ships `DriverHub` so multiple consumers can share a driver. Neither plan touches real hardware — the CLI can demo the pipeline shape but not actually capture or emit keystrokes. Plan 2b closes that gap so the user can (a) prove the architecture works end-to-end with real input before investing in Plan 3's GUI, and (b) start using the tool for real macros from the terminal.

The major risk we're absorbing here is platform: Interception requires admin install + reboot, only runs on Windows, and cannot run in CI. Splitting the driver work from the GUI work keeps each plan's risk surface bounded.

## Goals

- New crate `rm-driver-interception` containing `InterceptionDriver: Driver` and `detect_status() -> DriverStatus`.
- Bidirectional `ScanCode <-> KeyCode` and `MouseStroke <-> RawEvent` mappings covering everything in `rm_macro_model::input` (full `KeyCode` enum + all five `MouseButton` variants + relative-move + wheel).
- `rm-cli` gains a `driver status` subcommand and a `--driver {stdio|interception}` flag on `record` / `play`.
- `record` with `--driver interception` stops gracefully on `Ctrl+C` (terminal SIGINT), saving everything captured so far.
- The Cargo feature `interception` on `rm-cli` gates compilation of all Interception-related code paths. Default-off. CI (`cargo test --workspace`, no features) continues to be green and unchanged.
- New gated tests in `crates/driver-interception` cover the scancode/mouse mapping logic with pure-Rust fixtures (no driver needed). The smoke test "open context + drop" is run manually, not in CI.

## Non-goals

- **Bundling `install-interception.exe`.** Deferred. `driver status` only detects and instructs.
- **Programmatic UAC elevation.** Deferred — same reason; the install-flow UX belongs in the GUI.
- **GUI status modal / install button.** Plan 3.
- **Per-device disambiguation in the CLI** (filtering which physical keyboard/mouse to capture). Interception supports it; the v1 CLI captures all keyboard + mouse devices.
- **Hotkey-driven stop for `record`.** Would require co-running a hotkey listener on the same hub and filtering the stop-key out of the recording — see Risks. CLI uses `Ctrl+C`; hotkey-driven stop is a Plan 3 concern.
- **Changing the `Driver` trait, `DriverHub`, `RawEvent`, or `KeyCode` enum shape.** Mappings are added; the types are not edited.
- **Replacing `MockDriver` / `StdioDriver`.** Both stay, used by tests and by the `--driver stdio` mode.

## Architecture

### Workspace and crate layout

```
crates/
  driver-interception/             ← NEW
    Cargo.toml                     deps: rm-driver, rm-macro-model, kanata-interception 0.3,
                                         tokio, tracing, thiserror, windows-sys (for SCM)
    src/
      lib.rs                       pub use driver::InterceptionDriver;
                                   pub use status::{DriverStatus, detect_status};
      driver.rs                    InterceptionDriver: Driver
      scancode.rs                  KeyCode <-> (scancode, e0_flag) tables
      mouse.rs                     MouseStroke <-> Vec<RawEvent> converters
      status.rs                    detect_status()
      thread.rs                    blocking OS thread loop
```

`crates/driver-interception` is added to `[workspace.members]`. `kanata-interception = "0.3"` and `windows-sys = { version = "0.59", features = [...] }` are added to `[workspace.dependencies]`.

Critically, **`rm-driver` is not modified**. `InterceptionDriver` implements the existing trait without changing it; consumers (`recorder`, `hotkey`, `player`) keep consuming `Arc<DriverHub>` and don't care which `Driver` impl is wrapped. The hub broadcast lag/closed semantics from Plan 2a apply unchanged.

### Why a separate crate, not a feature on `rm-driver`

Three reasons:
1. **Optional system dependency.** `kanata-interception` statically links against a bundled `interception.lib` and the resulting binary requires `interception.dll` at runtime. Keeping that dependency in a separate crate means `rm-driver` (consumed by every other crate) doesn't pull it in.
2. **License hygiene.** `kanata-interception` transitively depends on `interception-sys` which is LGPL-3.0. Confining that boundary to a single crate makes the licensing story for the rest of the workspace (MIT OR Apache-2.0) easier to reason about. See "Licensing" below.
3. **CI cleanliness.** No `[features]` plumbing across multiple crates; only `rm-cli` needs the feature flag.

### Concurrency model — why a dedicated OS thread

`kanata-interception`'s API: `Interception::wait()` blocks the calling thread until input arrives. It is **not** cancellation-aware; there's no async variant and no `wait_with_timeout` that takes a closeable handle. The `Driver` trait, however, is async (`async fn recv(&self) -> Result<RawEvent, ...>`).

```
┌───────────────────────────────┐         ┌──────────────────────────────────────┐
│ Tokio runtime                 │         │ std::thread (one per InterceptionDriver) │
│                               │         │                                      │
│  Driver::recv() ──► event_rx ◄┼─mpsc────┼── event_tx ◄── loop {                │
│       (await)                 │         │                  ctx.wait_with_timeout│
│                               │         │                            (100ms);  │
│  Driver::send() ──────────────────────────► ctx.send() (per-context safe)      │
│       (no await, direct call) │         │                  ctx.receive(device);│
│                               │         │                  for stroke ⇒        │
│                               │         │                    map → RawEvent ⇒  │
│                               │         │                    event_tx.send();  │
│                               │         │                  check shutdown flag.│
│                               │         │              }                       │
└───────────────────────────────┘         └──────────────────────────────────────┘
```

- One `std::thread` per `InterceptionDriver` instance. The CLI builds at most one driver per command (`cmd_record` or `cmd_play`), so this is one thread.
- The thread loops on `wait_with_timeout(Some(100))`. On wake, it calls `receive` for the woken device and converts each stroke into 0..N `RawEvent`s (see "Mouse stroke decomposition" — one mouse stroke can yield multiple events). The 100ms timeout exists so the thread can re-check the shutdown flag and exit promptly when the driver is dropped, even if no input is arriving.
- The channel is `tokio::sync::mpsc::unbounded_channel`. Unbounded is acceptable because the human-input rate (~50 ev/s peak) is far below the consumer rate, and we're already bounded downstream by `DriverHub`'s broadcast capacity of 256. The recorder/hotkey absorb bursts; lag is a hub concern, not a driver concern.

### `InterceptionDriver` type

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::mpsc;

pub struct InterceptionDriver {
    /// Wrapped in Arc so the OS thread and the &self send-path both have it.
    /// The wrapper provides `unsafe impl Sync` — kanata-interception does not
    /// declare Sync on `Interception` because the struct holds a raw pointer,
    /// but `interception_send` and `interception_wait`/`receive` are documented
    /// as safe per-context across threads. We rely on that.
    ctx: Arc<InterceptionCtx>,

    /// Tokio side of the OS-thread → async bridge.
    event_rx: AsyncMutex<mpsc::UnboundedReceiver<RawEvent>>,

    /// Set by Drop, polled by the OS thread between wait_with_timeout calls.
    shutdown: Arc<AtomicBool>,

    /// Retained so Drop can join, ensuring clean teardown.
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Newtype wrapper that asserts Sync. SAFETY: the Interception C API is
/// per-context thread-safe (per oblitum/Interception README + interception.h).
struct InterceptionCtx(kanata_interception::Interception);
unsafe impl Sync for InterceptionCtx {}
// Send is auto-derived if the inner type is Send; the raw pointer makes it not
// Send by default. We also `unsafe impl Send for InterceptionCtx {}` for the
// same per-context-safe reason.
unsafe impl Send for InterceptionCtx {}

impl InterceptionDriver {
    /// Construct a driver. Returns an error if Interception isn't available
    /// (DLL missing, services not running, etc.). On success, spawns the OS
    /// thread immediately and starts pumping events into the internal channel.
    pub fn new() -> Result<Self, DriverError> { ... }
}

#[async_trait]
impl Driver for InterceptionDriver {
    async fn send(&self, e: RawEvent) -> Result<(), DriverError> {
        // Map RawEvent → (Device, Stroke) and call self.ctx.0.send(...).
        // Per-context safe under concurrent &self; no locking here.
        ...
    }

    async fn recv(&self) -> Result<RawEvent, DriverError> {
        let mut rx = self.event_rx.lock().await;
        rx.recv().await.ok_or(DriverError::Closed)
    }
}

impl Drop for InterceptionDriver {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // OS thread will exit within ~100ms (wait_with_timeout slice).
        // Joining ensures the thread observes channel-tx drop in order.
        if let Some(t) = self.thread.take() { let _ = t.join(); }
    }
}
```

The `AsyncMutex` on `event_rx` exists for the same reason `MockDriver` uses one — `mpsc::UnboundedReceiver::recv` takes `&mut self`, but `Driver::recv` takes `&self`. Only one task ever calls `Driver::recv` at a time in practice (the hub's pump task), so the lock is uncontended in steady state. This matches the existing pattern.

### OS thread loop

```rust
fn pump_thread(ctx: Arc<InterceptionCtx>, event_tx: mpsc::UnboundedSender<RawEvent>,
               shutdown: Arc<AtomicBool>) {
    use kanata_interception::{Device, Stroke};

    let timeout_ms = 100u64;
    loop {
        if shutdown.load(Ordering::SeqCst) { break; }
        // Returns 0 on timeout, or the device number that has input.
        let device = ctx.0.wait_with_timeout(std::time::Duration::from_millis(timeout_ms));
        if device == 0 { continue; }   // timeout: re-check shutdown

        let mut strokes = [Stroke::Keyboard { ..Default::default() }; 32];
        let n = ctx.0.receive(device, &mut strokes);
        if n <= 0 { continue; }

        for s in &strokes[..n as usize] {
            for ev in convert_stroke(*s) {
                // Send-error means the Tokio side dropped the receiver.
                if event_tx.send(ev).is_err() { return; }
            }
        }
    }
    // Drop event_tx implicitly on return — rx.recv() resolves to None →
    // Driver::recv returns DriverError::Closed → hub pump sees Closed and
    // propagates it to subscribers (same path Plan 2a tests for).
}
```

(`Stroke::Keyboard { .. }` initialization is illustrative — `kanata_interception::Stroke` is a tagged enum; the actual buffer initialization will use `Stroke::default()` or the equivalent the crate provides.)

### `convert_stroke` — keyboard

```rust
/// Decomposed events for a single Interception stroke. Returned by value to
/// avoid a heap allocation per event; consumers iterate `0..len` over `events`.
/// Sized at 6 to cover the worst case of a mouse stroke carrying every button
/// bit + wheel + move simultaneously (extremely rare but theoretically possible).
pub struct StrokeEvents {
    pub events: [Option<RawEvent>; 6],
}

fn convert_stroke(s: Stroke) -> StrokeEvents {
    match s {
        Stroke::Keyboard { code, state, .. } => convert_keyboard(code, state),
        Stroke::Mouse    { state, flags, rolling, x, y, .. } =>
            convert_mouse(state, flags, rolling, x, y),
    }
}

fn convert_keyboard(code: ScanCode, state: KeyState) -> StrokeEvents {
    use kanata_interception::KeyState;
    let is_up = state.intersects(KeyState::UP);
    let is_e0 = state.intersects(KeyState::E0);
    // Drop E1 (Pause prefix), TermSrv flags — not modeled in RawEvent.
    if state.intersects(KeyState::E1 | KeyState::TERMSRV_SET_LED | KeyState::TERMSRV_SHADOW
                      | KeyState::TERMSRV_VKPACKET) {
        return StrokeEvents::empty();
    }
    let mut out = StrokeEvents::empty();
    match scancode_to_keycode(code, is_e0) {
        Some(key) if is_up   => { out.events[0] = Some(RawEvent::KeyUp { key }); }
        Some(key)            => { out.events[0] = Some(RawEvent::KeyDown { key }); }
        None => {
            tracing::debug!(?code, ?state, "interception: unmapped scancode dropped");
        }
    }
    out
}
```

`StrokeEvents` is a stack-allocated fixed array of `Option<RawEvent>; 6` — no new crate dep, no heap allocation per event. Consumers iterate `events.iter().flatten()`. Sized at 6 to cover the worst-case mouse stroke (5 button bits + 1 wheel; movement is decomposed into a separate event but typically a stroke is button-OR-wheel-OR-move).

Unmapped scancodes are dropped with a debug log, not propagated as errors. Rationale: the user shouldn't get a fatal error because they pressed a key the model doesn't represent (e.g., media keys). The same loss occurs in the reverse direction (`send` of an unmodeled event is a no-op + debug log).

### `convert_stroke` — mouse

A single `MouseStroke` can carry multiple state bits AND a non-zero (x, y) AND a wheel delta. We decompose into 0..N events emitted in a stable order: **buttons first, then wheel, then move**. Movement events with `(0, 0)` are dropped.

```rust
fn convert_mouse(state: MouseState, flags: MouseFlags, rolling: i16, x: i32, y: i32)
    -> StrokeEvents
{
    let mut out = StrokeEvents::empty();
    let mut n = 0;
    let mut push = |ev: RawEvent| {
        if n < out.events.len() { out.events[n] = Some(ev); n += 1; }
    };

    // Buttons. Bits come in down/up pairs in MouseState.
    if state.contains(MouseState::LEFT_BUTTON_DOWN)   { push(RawEvent::MouseDown { button: MouseButton::Left }); }
    if state.contains(MouseState::LEFT_BUTTON_UP)     { push(RawEvent::MouseUp   { button: MouseButton::Left }); }
    if state.contains(MouseState::RIGHT_BUTTON_DOWN)  { push(RawEvent::MouseDown { button: MouseButton::Right }); }
    if state.contains(MouseState::RIGHT_BUTTON_UP)    { push(RawEvent::MouseUp   { button: MouseButton::Right }); }
    if state.contains(MouseState::MIDDLE_BUTTON_DOWN) { push(RawEvent::MouseDown { button: MouseButton::Middle }); }
    if state.contains(MouseState::MIDDLE_BUTTON_UP)   { push(RawEvent::MouseUp   { button: MouseButton::Middle }); }
    if state.contains(MouseState::BUTTON_4_DOWN)      { push(RawEvent::MouseDown { button: MouseButton::X1 }); }
    if state.contains(MouseState::BUTTON_4_UP)        { push(RawEvent::MouseUp   { button: MouseButton::X1 }); }
    if state.contains(MouseState::BUTTON_5_DOWN)      { push(RawEvent::MouseDown { button: MouseButton::X2 }); }
    if state.contains(MouseState::BUTTON_5_UP)        { push(RawEvent::MouseUp   { button: MouseButton::X2 }); }

    // Wheel — vertical only in v1 (HWHEEL deferred until needed).
    if state.contains(MouseState::WHEEL) && rolling != 0 {
        push(RawEvent::MouseWheel { delta: rolling as i32 });
    }

    // Movement. We only emit when there is actual delta; absolute movement
    // (MoveFlags::MOVE_ABSOLUTE) is not used by Interception's stream of raw
    // hardware events but we convert to relative-equivalent if seen.
    if x != 0 || y != 0 {
        if flags.contains(MouseFlags::MOVE_ABSOLUTE) {
            tracing::debug!(x, y, "interception: absolute mouse movement converted as relative");
        }
        push(RawEvent::MouseMove { dx: x, dy: y });
    }

    out
}
```

Reverse direction (`send`) is the inverse: each `RawEvent` becomes exactly one `MouseStroke` or `KeyStroke`. Wheel + button-down in the same step is not expressible by our model — that's fine; users compose macros as discrete steps.

### Scancode mapping

The `KeyCode` enum has ~75 variants. Each maps to a single Interception `ScanCode` (Set 1 / "XT") plus an `is_e0` flag. The mapping is a pair of `match` tables in `scancode.rs`:

```rust
pub fn scancode_to_keycode(code: ScanCode, e0: bool) -> Option<KeyCode> {
    match (code as u16, e0) {
        (0x1E, false) => Some(KeyCode::A),
        (0x30, false) => Some(KeyCode::B),
        // ... (full letter row, digits, F-row, etc.)
        (0x1D, false) => Some(KeyCode::LCtrl),
        (0x1D, true)  => Some(KeyCode::RCtrl),    // E0-prefixed
        (0x38, false) => Some(KeyCode::LAlt),
        (0x38, true)  => Some(KeyCode::RAlt),     // AltGr / Right Alt
        (0x48, true)  => Some(KeyCode::Up),       // Arrows are all E0
        (0x50, true)  => Some(KeyCode::Down),
        (0x4B, true)  => Some(KeyCode::Left),
        (0x4D, true)  => Some(KeyCode::Right),
        // ... edit cluster (Insert/Delete/Home/End/PageUp/PageDown all E0)
        _ => None,
    }
}

pub fn keycode_to_scancode(k: KeyCode) -> (u16, bool /* e0 */) {
    match k {
        KeyCode::A => (0x1E, false),
        // ... inverse of the above
    }
}
```

Coverage is the full `KeyCode` enum from `crates/macro_model/src/input.rs`. A roundtrip property test (`for k in all_keycodes() { assert_eq!(scancode_to_keycode(keycode_to_scancode(k)), Some(k)); }`) is part of the unit tests and runs with no driver dependency. The authoritative scancode reference is the Windows Set 1 scancode table; we'll cite the URL in a code comment.

`KeyCode` already carries a comment in Plan 1 — *"Plan 2 will add `From<interception::ScanCode>` impls"* — but on second thought, putting `From` impls on `KeyCode` would require `rm-macro-model` to depend on `kanata-interception`, polluting a domain crate with a system dep. The mappings live in `rm-driver-interception` instead, as plain functions. The Plan 1 comment will be removed in this plan's PR.

### `detect_status`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverStatus {
    /// Interception services not present on the system at all.
    NotInstalled,
    /// Services exist but are stopped (rare — usually user-disabled).
    InstalledNotRunning,
    /// Services running and a context can be opened.
    Running,
}

pub fn detect_status() -> DriverStatus {
    // 1. Try to open a context. If this succeeds, the driver is fully operational.
    if let Some(_ctx) = try_open_context() {
        return DriverStatus::Running;
    }
    // 2. Otherwise check the Windows SCM for the two driver services.
    match query_services() {
        ServiceState::AllRunning  => DriverStatus::Running,   // unreachable in practice
        ServiceState::AllPresent  => DriverStatus::InstalledNotRunning,
        ServiceState::AnyMissing  => DriverStatus::NotInstalled,
    }
}
```

`try_open_context()` is `kanata_interception::Interception::new().ok()` wrapped in `std::panic::catch_unwind` (defensive — the underlying FFI shouldn't panic, but unknown territory).

`query_services()` uses `windows-sys` to call `OpenSCManagerW(NULL, NULL, SC_MANAGER_CONNECT)` then `OpenServiceW` for each of the two Interception driver service names. The service names per Oblitum's installer are `"keyboard"` and `"mouse"` — **the implementation must verify these names from a live install before merging** (see Risks). If either `OpenServiceW` returns `ERROR_SERVICE_DOES_NOT_EXIST`, the status is `NotInstalled`. Otherwise we `QueryServiceStatus` and report based on `dwCurrentState`.

### CLI changes

`Cmd` enum gains a feature-gated variant and a feature-gated flag:

```rust
#[derive(Subcommand)]
enum Cmd {
    Record {
        name: String,
        #[cfg(feature = "interception")]
        #[arg(long, value_enum, default_value_t = DriverKind::Stdio)]
        driver: DriverKind,
    },
    Play {
        name: String,
        #[cfg(feature = "interception")]
        #[arg(long, value_enum, default_value_t = DriverKind::Stdio)]
        driver: DriverKind,
    },
    List,
    Delete { name: String },
    #[cfg(feature = "interception")]
    /// Driver status / install instructions.
    Driver {
        #[command(subcommand)]
        sub: DriverCmd,
    },
}

#[cfg(feature = "interception")]
#[derive(Subcommand)]
enum DriverCmd {
    /// Print Interception driver status: Running | InstalledNotRunning | NotInstalled.
    Status,
}

#[cfg(feature = "interception")]
#[derive(clap::ValueEnum, Clone, Copy)]
enum DriverKind { Stdio, Interception }
```

When the feature is off (default), the CLI is identical to today's Plan 2a CLI.

When the feature is on:
- `macro-cli driver status` calls `detect_status()` and prints:
  - `Running` → `"Interception driver: Running."` (exit 0)
  - `InstalledNotRunning` → message + `"Reboot may be required."` (exit 0; informational)
  - `NotInstalled` → message + `"Install from https://github.com/oblitum/Interception/releases"` (exit 0)
- `--driver interception` on `record` or `play` swaps `StdioDriver` for `InterceptionDriver::new()` when building the hub.

#### Ctrl+C stop in `cmd_record`

`cmd_record` for `--driver interception` runs concurrently with `tokio::signal::ctrl_c()`:

```rust
pub async fn cmd_record(root: &Path, name: &str, driver: DriverKind) -> Result<()> {
    let (drv, passthrough) = match driver {
        DriverKind::Stdio        => (Arc::new(StdioDriver::new()) as Arc<dyn Driver>, false),
        DriverKind::Interception => (Arc::new(open_interception()?), true),
    };
    let hub = DriverHub::start(drv);
    let handle = start_recording(hub, passthrough);

    let steps = match driver {
        DriverKind::Stdio => handle.wait_for_close().await?,    // stdin EOF, unchanged
        DriverKind::Interception => {
            tokio::signal::ctrl_c().await
                .map_err(|e| AppError::Other(format!("ctrl_c handler: {e}")))?;
            eprintln!("\nstopping...");
            handle.finish().await?
        }
    };
    if steps.is_empty() { return Err(AppError::Other("no events recorded".into())); }
    // ... save, unchanged
}
```

`passthrough=true` is forced on for `--driver interception` because Interception intercepts events at the driver level — without re-emitting via `hub.send()`, the user typing into Notepad would see nothing on screen during recording. `passthrough=false` is preserved for `--driver stdio` to match Plan 1/2a behavior (`StdioDriver` re-emits to stdout, so passthrough would double-print).

#### `open_interception` — preserves error-kind fidelity

Direct `InterceptionDriver::new()` failures lose information: the caller can't tell "driver isn't installed" from "driver installed but the service is stopped". `rm-error` already defines `DriverNotInstalled` and `DriverNotRunning` exactly for this distinction (`crates/error/src/lib.rs:7-10`); their stable `kind()` strings are the contract Plan 3's frontend will switch on.

```rust
fn open_interception() -> Result<InterceptionDriver> {
    InterceptionDriver::new().map_err(|orig| match detect_status() {
        DriverStatus::NotInstalled         => AppError::DriverNotInstalled,
        DriverStatus::InstalledNotRunning  => AppError::DriverNotRunning,
        DriverStatus::Running              => AppError::DriverIo(orig.to_string()),
        // ^ shouldn't normally happen — new() failed but status says Running.
        // Could be a race with a concurrent context-open; pass the original error.
    })
}
```

#### Ctrl+C and Interception — important subtlety

When the user presses `Ctrl+C` in the terminal, Windows delivers `SIGINT` to the process. The `Ctrl` and `C` keystrokes themselves also flow through Interception and into the recorder buffer. Since `start_recording` is `biased` on event-first then stop-signal (see `crates/recorder/src/lib.rs`), and `handle.finish()` is called *after* the ctrl_c future resolves, the recorder MAY include `LCtrl down`, `C down`, `C up`, `LCtrl up` at the tail of the recording.

Acceptance: this is acceptable for v1. The user can trim the tail in Plan 3's editor. Documented in CLI help text: *"On stop, the trailing Ctrl+C keystrokes are captured into the macro; edit them out later."*

### Recorder / hotkey / player

**No changes** to `crates/recorder`, `crates/hotkey`, `crates/player`. They consume `Arc<DriverHub>` and don't care which `Driver` impl the hub wraps. This is the entire point of Plan 2a.

### Feature flag scheme

A single workspace-aware flag, on the `rm-cli` crate only:

```toml
# crates/cli/Cargo.toml
[features]
default = []
interception = ["dep:rm-driver-interception"]

[dependencies]
rm-driver-interception = { path = "../driver-interception", optional = true }
```

`crates/driver-interception` itself has no feature flag — it always compiles `kanata-interception`. If you depend on it, you pay the cost. The crate is only ever a dependency of `rm-cli` (and later, Plan 3's `app`), and only behind those crates' `interception` features.

CI (`cargo test --workspace`) does not enable the feature, so `kanata-interception` is not compiled, no `interception.dll` is needed, and tests run on `windows-latest` exactly as today. A second CI job `cargo check --workspace --features rm-cli/interception` proves the feature-gated code at least compiles — but does not run tests against it (no driver).

### DLL distribution

`interception-sys`'s `build.rs` copies `interception.dll` into `OUT_DIR` and emits the link directive. For the resulting `macro-cli.exe` to launch, `interception.dll` must be in the same directory or on `PATH`. Two paths:

- **Dev**: the user's Interception install already places `interception.dll` somewhere on `PATH` (`C:\Windows\System32\` per Oblitum installer). Builds from `cargo run --features interception` on a machine with Interception installed should work without further action.
- **Release**: distribute the `.exe` with `interception.dll` next to it. A `build.rs` in `rm-cli` (only when the feature is enabled) copies the DLL from `OUT_DIR` next to the final binary. Implementation note: `cargo` doesn't have a clean post-build hook to drop files next to the binary; we'll use a simple `cargo xtask` script as an opt-in build step rather than fighting build.rs.

For Plan 2b, the dev-machine path is what matters — the user (this project's owner) installs Interception, runs `cargo run --features interception -- driver status`. The release-bundling concern is real but small, and will be solved properly by the Tauri bundler in Plan 3.

## Licensing

`kanata-interception` 0.3.x depends on `interception-sys` 0.1.3, licensed **LGPL-3.0**. Static linkage from a Rust binary into an LGPL'd Rust crate inherits LGPL obligations (per LGPL section 4, the user must be able to relink with a modified version of the library). Practical options:

1. **Accept the obligation, document in `LICENSES.md`.** Ship the source of `interception-sys` (Cargo already records it in `Cargo.lock`; `cargo vendor` produces a redistributable tree). The Rust binary is already statically linked from object files, which `cargo build` can be coerced to preserve via `--message-format=json`.
2. **Switch to a custom thin FFI binding** (option B from the original research) to bypass the LGPL'd Rust wrapper. The underlying `interception.dll` remains LGPL but it's dynamically loaded — that's compliant by default.

**Decision: option 1 for Plan 2b.** This is a personal project, the user (project owner) is not currently distributing binaries; the LGPL obligation becomes load-bearing only at Plan 3's release time. We add a `LICENSES.md` documenting the dependency now, and revisit option 2 only if the obligation becomes a blocker for distribution. This is called out in Risks.

## Testing Strategy

### Unit tests (run in CI, all targets, no features)

Existing 60 tests unchanged. Plan 2b adds **none** to the always-on path; everything new is feature-gated.

### Unit tests gated by `--features interception` (compile-checked in CI, run manually)

Located in `crates/driver-interception/src/{scancode.rs, mouse.rs, status.rs}`:

- **`scancode.rs`**:
  - Roundtrip property: for every `KeyCode` variant, `scancode_to_keycode(keycode_to_scancode(k))` == `Some(k)`.
  - Spot tests on tricky cases: `LCtrl`/`RCtrl` (same scancode, E0 distinguishes), arrows (all E0), `LAlt`/`RAlt`.
  - E1/TermSrv state bits drop and return empty.
  - Unknown scancode returns `None` and yields a debug log (test via `tracing` test subscriber).
- **`mouse.rs`**:
  - Each `MouseState` button bit produces the expected `RawEvent`, in declared order.
  - Combined stroke (button-down + move + wheel) decomposes into 3 events in order: button, wheel, move.
  - `(0, 0)` movement is dropped.
  - `MOVE_ABSOLUTE` flag is logged but doesn't error.
- **`status.rs`**:
  - `query_services` against a mocked SCM-API trait — extract a `ServiceQuery` trait so we can inject a fake. Verifies all three states produce the expected `DriverStatus`.
  - `try_open_context` failure (`catch_unwind` engaged) does not panic the calling thread; integration boundary only.

These tests run via `cargo test -p rm-driver-interception` (no feature flag — the crate has only the optional `smoke` feature, never an `interception` feature; the `interception` feature lives on `rm-cli` and only controls whether `rm-cli` depends on this crate). `kanata-interception` must compile (so the `interception.dll` must be link-resolvable at build time, which `interception-sys`'s vendored copy provides).

### Smoke test (manual — requires Interception installed)

`crates/driver-interception/tests/smoke.rs`, gated `#[cfg(feature = "smoke")]` so it's never run in CI:

- `open_close_context` — `InterceptionDriver::new()` succeeds, then drop completes within 200ms (proves the OS thread shuts down on Drop within the wait-with-timeout slice).
- `detect_status_returns_known_variant` — `detect_status()` returns one of the three variants and doesn't panic.

### Manual demo test plan (Notepad)

The shippable artifact, run on the user's machine after Plan 2b lands:

1. Install Interception (per `driver status` instructions). Reboot.
2. `cargo run --features interception -- driver status` → prints `Running`.
3. Open Notepad. Foreground the terminal.
4. `cargo run --features interception -- record demo`
5. Click into Notepad, type `hello world`.
6. Return to terminal, press `Ctrl+C`.
7. CLI prints `stopping... saved demo (<uuid>)`.
8. `cargo run --features interception -- list` shows `demo`.
9. Click into Notepad (cursor at end of `hello world`). Press Enter to start a new line.
10. Return to terminal: `cargo run --features interception -- play demo`
11. Watch `hello world` appear in Notepad on the new line. Trailing `^C` characters (from step 6) may also appear — acceptable per design.

This is the "done" criterion for Plan 2b.

## Risks and Trade-offs

1. **Service name verification.** The detect_status path assumes the Interception driver registers services named `"keyboard"` and `"mouse"`. This needs to be verified against a live install before merge. If the names are different, the implementation adjusts; the design is unchanged.
2. **`kanata-interception` maintenance.** The crate is a fork of an abandoned 2020 crate. Active maintainer (kanata project), but a single upstream breakage could leave us stuck. Mitigation: pin to `0.3.x`, vendor via `cargo vendor` if we ever ship binaries. Switching to a custom FFI binding (~300 LoC) is always available as a fallback.
3. **LGPL-3.0 license.** Documented in "Licensing" above. Becomes load-bearing at distribution time, not before.
4. **OS thread shutdown latency.** Up to 100ms per `InterceptionDriver` drop. Acceptable for CLI; revisit if Plan 3 creates/destroys drivers often (it won't — driver is process-lifetime).
5. **Mouse `MOVE_ABSOLUTE` strokes.** Touchpads can emit these. We convert to relative for now (passes the value through unchanged with a log). If a user reports broken touchpad recording, we add real absolute→relative conversion using last-known position. v1 punt is acceptable because the target use case is gaming mice (always relative).
6. **Trailing Ctrl+C keystrokes in recordings.** Documented in CLI help. Plan 3 editor will let users trim. Not worth implementing key-filtering in CLI given how cheap manual editing is.
7. **Concurrent `Driver::send` correctness.** We rely on `interception_send` being per-context thread-safe (`unsafe impl Sync for InterceptionCtx`). This is documented by the Interception project but unverified by us. Mitigation: an integration test (gated) that hammers `send` from many tasks at once and asserts no panic / no DLL error code. Listed in the implementation plan's task list.
8. **Unbounded channel growth during `cmd_play` with `--driver interception`.** `InterceptionDriver::new()` unconditionally spawns the OS pump thread, which pushes captured events into an unbounded `mpsc`. During `cmd_play`, nobody subscribes to the hub (player only emits), so any keystrokes the user types incidentally during playback accumulate in the channel. Bounded in practice by typing rate (~50 ev/s) × playback duration; for the typical Plan 2b demo (seconds of playback) this is negligible. Becomes load-bearing only if Plan 3 keeps a driver alive for hours of looping playback. Mitigation if/when needed: add a `InterceptionDriver::new_send_only()` constructor that builds a driver without spawning the pump (`Driver::recv` then returns `Closed` immediately). Not implemented in Plan 2b.

## Acceptance criteria

Plan 2b is "done" when:

1. `cargo test --workspace` (no features) is green on Windows CI. **No behavior change vs. Plan 2a.**
2. `cargo check --workspace --features rm-cli/interception` is green on Windows CI (proves the feature-gated paths compile).
3. `cargo test -p rm-driver-interception` is green locally on the dev machine (scancode + mouse + mocked-SCM tests).
4. Smoke test `crates/driver-interception/tests/smoke.rs` passes locally with Interception installed.
5. The manual Notepad demo plan above passes end-to-end on the dev machine after a clean Interception install + reboot.
6. `LICENSES.md` exists at repo root documenting the LGPL-3.0 transitive dependency.
7. The stub `docs/superpowers/plans/2026-05-26-rust-macro-plan-2b-real-driver.md` is deleted in the same PR that lands Plan 2b's implementation, replaced by `2026-05-26-rust-macro-plan-2b-real-driver.md` (the writing-plans output).
8. The "Plan 2 will add `From<interception::ScanCode>` impls" comment in `crates/macro_model/src/input.rs` is removed (mappings live in `rm-driver-interception` instead).

## Out of scope (becomes a later plan)

- Bundling `install-interception.exe` and `driver install` UAC flow.
- GUI driver status modal / install button (Plan 3).
- Per-device disambiguation UI (CLI or GUI).
- Horizontal mouse wheel support.
- Absolute→relative mouse coordinate conversion for touchpads.
- Hotkey-driven stop for `record` (requires either filtering the stop key out of captured events, or using a Win32-level hotkey that bypasses the recorder — both belong with the GUI).
