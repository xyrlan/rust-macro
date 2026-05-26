# rust-macro — Plan 2a: DriverHub + Consumer Refactor (design)

**Date:** 2026-05-26
**Status:** Approved (brainstorming phase)
**Supersedes:** the "DriverHub" portion of `plans/2026-05-26-rust-macro-plan-2-real-driver.md` (which is now split into 2a and 2b)
**Parent spec:** `specs/2026-05-26-rust-macro-design.md`

## Summary

Plan 2a introduces `DriverHub`, a single-owner multiplexer over the existing `Driver` trait, and refactors `recorder`, `hotkey`, and `player` to consume the hub instead of `Arc<dyn Driver>`. The change unblocks Plan 3's concurrent-consumer requirement (recorder, hotkey, and player must all be active at the same time) while staying entirely on top of `MockDriver` — no Interception integration in this slice. CI stays green; no new system dependencies; no admin reboot to test.

## Motivation

Today `recorder` and `hotkey` both call `Driver::recv()` in independent loops. The Plan 1 CLI sidesteps the conflict because it runs them sequentially. Plan 3's GUI runs the hotkey listener continuously and starts the recorder on demand — `recv()` would be racing for events. `Driver::recv()` cannot be split because it consumes one event per call.

The fix is to put a fan-out layer between the `Driver` and its consumers: one task owns the `Driver`, drains its `recv()` stream, and broadcasts each event to every subscriber. Consumers that need to read switch from `driver.recv()` to a `broadcast::Receiver<RawEvent>`. Consumers that only emit (the player) just call `hub.send()`.

## Goals

- A `DriverHub` type that owns an `Arc<dyn Driver>` and offers `subscribe()` → `broadcast::Receiver<RawEvent>` plus `async send(RawEvent)`.
- `recorder`, `hotkey`, and `player` accept `Arc<DriverHub>` instead of `Arc<dyn Driver>`.
- The CLI builds one `DriverHub` at startup and threads it through every command.
- Recorder and hotkey can run concurrently on the same hub and both see every event.
- Hub shuts down cleanly when dropped (pump task exits, subscribers see `Closed`).
- Test coverage in `crates/driver` for the hub itself; the existing recorder/hotkey/player tests are migrated to use a hub-wrapped `MockDriver` with no behavioral change.

## Non-goals

- **Interception driver integration.** Deferred to Plan 2b.
- **Driver status detection / installer flow.** Deferred to Plan 2b.
- **Per-subscriber filtering.** All subscribers see all events; consumers filter internally (hotkey already ignores mouse events, etc.). Adding a filter API is a YAGNI hazard until Plan 3 needs it.
- **Backpressure beyond broadcast's built-in lag semantics.** See "Risks" below.
- **Changing the `Driver` trait, `RawEvent`, or any data model.**

## Architecture

### Module layout

`DriverHub` lives in the existing `rm-driver` crate, alongside the trait and `MockDriver`:

```
crates/driver/src/
  lib.rs       ← Driver trait, RawEvent, DriverError (unchanged)
  mock.rs      ← MockDriver (unchanged)
  hub.rs       ← NEW: DriverHub
```

`lib.rs` adds `pub mod hub;` and a `pub use hub::DriverHub;`.

### Type

```rust
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const BROADCAST_CAPACITY: usize = 256;

pub struct DriverHub {
    driver: Arc<dyn Driver>,
    tx: broadcast::Sender<RawEvent>,
    shutdown: CancellationToken,
    pump: Option<JoinHandle<()>>,  // taken in Drop; Option avoids needing a mutex
}

impl DriverHub {
    /// Wraps `driver`, spawns the pump task, returns an `Arc<DriverHub>`.
    pub fn spawn(driver: Arc<dyn Driver>) -> Arc<Self>;

    /// New subscriber. Each `Receiver` is independently positioned; events
    /// emitted *before* `subscribe()` are not delivered (broadcast semantics).
    pub fn subscribe(&self) -> broadcast::Receiver<RawEvent>;

    /// Emit an event toward the OS. Direct passthrough to the inner driver.
    pub async fn send(&self, e: RawEvent) -> Result<(), DriverError>;
}

impl Drop for DriverHub {
    fn drop(&mut self) {
        self.shutdown.cancel();
        // pump task observes the token and exits; we don't block on it in Drop.
    }
}
```

Workspace dep additions (root `Cargo.toml`): `tokio-util = { version = "0.7", features = ["rt"] }`.

### Pump task

```rust
async fn pump(driver: Arc<dyn Driver>, tx: broadcast::Sender<RawEvent>, shutdown: CancellationToken) {
    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            got = driver.recv() => match got {
                Ok(event) => { let _ = tx.send(event); }
                Err(DriverError::Closed) => break,
                Err(e) => { tracing::debug!(error = ?e, "driver hub: recv error, stopping pump"); break; }
            }
        }
    }
}
```

`biased` mirrors the recorder pattern in Plan 1: shutdown is checked before recv, so cancellation is observed promptly. `tx.send` returning `Err` (no subscribers) is fine — we drop the event silently. The pump task is not retained by the public API; the `DriverHub` holds its `JoinHandle` to keep it alive as long as the hub lives, and `Drop` cancels it.

### Send path

`hub.send()` is a thin async pass-through to `driver.send()`. Concurrency:
- `MockDriver::send` is `&self` and internally thread-safe (uses `Arc<Mutex<_>>`).
- The future `InterceptionDriver` (Plan 2b) is also `&self` thread-safe; `interception_send` is documented safe per context.
- No mutex inside `DriverHub::send`. If a future driver impl needs serialization, the impl wraps itself.

### Lag and capacity

`broadcast::Receiver::recv()` returns `RecvError::Lagged(n)` when a subscriber falls behind the sender's ring buffer. With capacity 256 and human input (~50 ev/s peak), a subscriber would have to stall ≥5 seconds to lag. Consumers must handle `Lagged` gracefully:
- **Recorder:** logs `warn!` with the lag count and continues. Records are best-effort.
- **Hotkey:** logs `debug!` and continues. A missed hotkey edge is acceptable; the user will press again.

Both consumers wrap `rx.recv()` in a `match` that explicitly handles `Lagged`. `Closed` is treated as end-of-stream.

## Consumer Refactor

### recorder

`crates/recorder/src/lib.rs`:

```rust
// Before:
pub fn start_recording(driver: Arc<dyn Driver>, passthrough: bool) -> RecordingHandle

// After:
pub fn start_recording(hub: Arc<DriverHub>, passthrough: bool) -> RecordingHandle
```

Inside the spawned task: `let mut rx = hub.subscribe();` before the loop, then the `select!` branch matches on `rx.recv().await`:

```rust
got = rx.recv() => match got {
    Ok(event) => {
        let at = Instant::now();
        if passthrough {
            if let Err(e) = hub.send(event).await {
                debug!(error = ?e, "recorder: passthrough send failed");
            }
        }
        buf_task.lock().await.push(TimedEvent { event, at });
    }
    Err(broadcast::error::RecvError::Lagged(n)) => {
        tracing::warn!(lagged = n, "recorder: dropped events under load");
    }
    Err(broadcast::error::RecvError::Closed) => break,
}
```

Existing tests rewrap their `MockDriver` in `DriverHub::spawn(...)` and otherwise read identically. The "subscribe before inject" timing already holds — `start_recording` calls `subscribe()` synchronously before spawning the task, so an inject after the call always reaches the subscriber.

### hotkey

`crates/hotkey/src/lib.rs`:

```rust
// Before:
pub fn start_listener(driver: Arc<dyn Driver>, registry: HotkeyRegistry, out_tx: mpsc::UnboundedSender<HotkeyHit>) -> ListenerHandle

// After:
pub fn start_listener(hub: Arc<DriverHub>, registry: HotkeyRegistry, out_tx: mpsc::UnboundedSender<HotkeyHit>) -> ListenerHandle
```

Same shape as recorder: `subscribe()` before the loop, match on `rx.recv()`, log on `Lagged`, exit on `Closed`. Mouse events are ignored exactly as today.

### player

`crates/player/src/lib.rs`:

```rust
// Before:
pub fn play(driver: Arc<dyn Driver>, macro_: Macro) -> PlaybackHandle

// After:
pub fn play(hub: Arc<DriverHub>, macro_: Macro) -> PlaybackHandle
```

`run_step` signature changes from `&dyn Driver` to `&DriverHub`. Every `driver.send(...)` becomes `hub.send(...)`. No subscribe needed — player only emits.

### cli

`crates/cli/src/main.rs` builds the hub at startup:

```rust
let driver: Arc<dyn Driver> = Arc::new(StdioDriver::new(...));
let hub = DriverHub::spawn(driver);
// pass `hub.clone()` into each subcommand
```

`commands.rs` signatures take `Arc<DriverHub>` and forward it to the appropriate consumer. No UX change.

## Testing

### New hub tests (`crates/driver/src/hub.rs`)

- `subscribe_receives_pumped_events` — `MockDriver::inject(e)` → subscriber gets `Ok(e)`.
- `two_subscribers_each_receive_every_event` — fan-out works; both see all injected events.
- `send_reaches_underlying_driver` — `hub.send(e)` → `MockDriver::drain_sent()` contains `e`.
- `drop_cancels_pump` — drop the `Arc<DriverHub>`, await a short tokio sleep, assert the pump task is no longer alive (use a probe channel: pump sends a sentinel on a `oneshot` from a `Drop` impl on a wrapping helper, or check `JoinHandle::is_finished` after a timeout).
- `lagged_subscriber_gets_lagged_error_then_continues` — fill the buffer past capacity; lagged subscriber sees `Lagged(n)` then resumes with subsequent events.
- `pump_exits_on_driver_closed` — driver that always returns `Closed` → pump exits. Documented contract: hub remains constructed, `hub.send()` continues to forward to the underlying driver (which will likely error), and existing subscribers stop receiving new events but do **not** see `Closed` until the hub itself is dropped. Test asserts: pump `JoinHandle::is_finished()` is true within a short timeout, and a subscriber's `recv()` times out (no events) rather than seeing `Closed` while the hub is alive.

### Migrated tests

- `recorder` tests: same assertions, hub-wrapped MockDriver.
- `hotkey` tests: same assertions, hub-wrapped MockDriver.
- `player` tests: same assertions, hub-wrapped MockDriver.

### New integration test (`crates/cli/tests/e2e.rs`)

`concurrent_recorder_and_hotkey_share_hub`:
1. Build `DriverHub` over `MockDriver`.
2. Start a hotkey listener bound to `Ctrl+F2`.
3. Start a recorder with passthrough=false.
4. Inject `[LCtrl down, F2 down, F2 up, LCtrl up]`.
5. Assert: hotkey listener fires one `HotkeyHit`, recorder captures all four events in order.

This is the test that proves the multiplexer architecture works — Plan 1 cannot express this scenario.

## Risks and Trade-offs

1. **Broadcast capacity of 256.** Sufficient for human input. If Plan 3 introduces a high-throughput producer (e.g., synthetic stress testing), revisit. Symptom would be `Lagged` warnings under normal use.
2. **Late subscribe loses events.** `broadcast` does not replay history. Callers must subscribe before the producing action. The current API already encourages this — `start_recording` subscribes synchronously before returning.
3. **Latency.** Each event goes through one extra channel hop (driver.recv → tx.send → rx.recv). Empirically tens of µs on tokio's broadcast; well within the design budget of <1 ms.
4. **Pump task lifecycle ambiguity.** If `Arc<DriverHub>` is cloned widely, the pump only stops when the last clone drops. Document this clearly and avoid storing hub clones in long-lived statics that outlive the runtime.

## Out of Scope (becomes Plan 2b)

- `rm-driver-interception` crate.
- `driver::detect_status()` and `install_driver` CLI commands.
- Bundling `install-interception.exe`.
- Manual test plan against real hardware.

## Acceptance

Plan 2a is "done" when:
- `cargo test --workspace` is green on Windows CI.
- All Plan 1 behavior is preserved (record→save→load→play roundtrip e2e test still passes).
- The new `concurrent_recorder_and_hotkey_share_hub` e2e passes.
- No new system dependency is required to run the test suite.
- The Plan 2 stub at `plans/2026-05-26-rust-macro-plan-2-real-driver.md` is replaced by Plan 2a (this spec's eventual implementation plan) and a fresh Plan 2b stub for the Interception work.
