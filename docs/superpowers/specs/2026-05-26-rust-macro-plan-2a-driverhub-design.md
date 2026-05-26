# rust-macro — Plan 2a: DriverHub + Consumer Refactor (design)

**Date:** 2026-05-26
**Status:** Approved after engineering review (brainstorming phase, revision 1)
**Supersedes:** the original `plans/2026-05-26-rust-macro-plan-2-real-driver.md` stub is split into this spec (2a) + a fresh `plans/2026-05-26-rust-macro-plan-2b-real-driver.md` stub. The Plan 2 implementation plan filename is **replaced** in-place by a Plan 2a file once writing-plans runs.
**Parent spec:** `specs/2026-05-26-rust-macro-design.md`

## Summary

Plan 2a introduces `DriverHub`, a single-owner multiplexer over the existing `Driver` trait, and refactors `recorder`, `hotkey`, and `player` to consume the hub instead of `Arc<dyn Driver>`. The change unblocks Plan 3's concurrent-consumer requirement (recorder, hotkey, and player must all be active at the same time) while staying entirely on top of `MockDriver` — no Interception integration in this slice. CI stays green; no new system dependencies; no admin reboot to test.

## Motivation

Today `recorder` and `hotkey` both call `Driver::recv()` in independent loops. The Plan 1 CLI sidesteps the conflict because it runs them sequentially. Plan 3's GUI runs the hotkey listener continuously and starts the recorder on demand — `recv()` would be racing for events. `Driver::recv()` cannot be split because it consumes one event per call.

The fix is to put a fan-out layer between the `Driver` and its consumers: one task owns the `Driver`, drains its `recv()` stream, and broadcasts each event to every subscriber. Consumers that need to read switch from `driver.recv()` to a `broadcast::Receiver<RawEvent>`. Consumers that only emit (the player) just call `hub.send()`.

## Goals

- A `DriverHub` type that owns an `Arc<dyn Driver>` and offers `subscribe()` → `broadcast::Receiver<RawEvent>` plus `async send(RawEvent)`.
- `recorder`, `hotkey`, and `player` accept `Arc<DriverHub>` instead of `Arc<dyn Driver>`.
- The CLI builds a `DriverHub` **per driver-using command** (`cmd_record`, `cmd_play`); `cmd_list` and `cmd_delete` stay driver-less.
- Recorder and hotkey can run concurrently on the same hub and both see every event.
- Hub propagates source-closed to subscribers: when the underlying `Driver` returns `DriverError::Closed`, all `broadcast::Receiver`s see `RecvError::Closed` on their next `recv()`. (This is what makes `RecordingHandle::wait_for_close` keep working through the hub — see C1 in the engineering review history.)
- Hub shuts down cleanly when the last `Arc<DriverHub>` is dropped (pump task exits via cancellation token, subscribers see `Closed`).
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
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

const BROADCAST_CAPACITY: usize = 256;

/// Shared slot holding the broadcast Sender. Wrapped in `Option` so the pump
/// task can `take()` it on exit, which drops the last Sender and gives every
/// existing `broadcast::Receiver` `RecvError::Closed`. Held by both the hub
/// (for `subscribe`) and the pump task (for emitting + clearing on exit).
type TxSlot = Arc<Mutex<Option<broadcast::Sender<RawEvent>>>>;

pub struct DriverHub {
    driver: Arc<dyn Driver>,
    tx: TxSlot,
    shutdown: CancellationToken,
}

impl DriverHub {
    /// Construct a hub over `driver`, spawn the internal pump task, return
    /// an `Arc<DriverHub>`. Clone freely — the pump runs until *either* the
    /// underlying driver returns `DriverError::Closed`, *or* the last `Arc`
    /// is dropped (triggering `shutdown.cancel()`). Whichever happens first,
    /// the pump drops the broadcast `Sender`, so all existing receivers see
    /// `Closed` on their next `recv()`.
    pub fn start(driver: Arc<dyn Driver>) -> Arc<Self> {
        let (tx, _seed_rx) = broadcast::channel(BROADCAST_CAPACITY);
        let tx_slot: TxSlot = Arc::new(Mutex::new(Some(tx)));
        let shutdown = CancellationToken::new();

        tokio::spawn(pump(driver.clone(), tx_slot.clone(), shutdown.clone()));
        Arc::new(Self { driver, tx: tx_slot, shutdown })
    }

    /// New subscriber. Returns `None` if the hub has already shut down
    /// (driver closed or hub being dropped). Each `Receiver` is independently
    /// positioned; events emitted *before* this call are not delivered.
    ///
    /// **API invariant — subscribe-before-emit.** Callers that will spawn a
    /// task to consume events MUST call `subscribe()` synchronously on the
    /// caller thread, then move the returned `Receiver` into the spawned
    /// task. Subscribing inside the task creates a race: the pump can
    /// deliver an injected event before the task's subscribe call lands,
    /// silently dropping the event. See "Consumer Refactor" for the pattern.
    pub fn subscribe(&self) -> Option<broadcast::Receiver<RawEvent>> {
        self.tx.lock().unwrap().as_ref().map(broadcast::Sender::subscribe)
    }

    /// Emit an event toward the OS. Direct passthrough to the inner driver.
    pub async fn send(&self, e: RawEvent) -> Result<(), DriverError> {
        self.driver.send(e).await
    }
}

impl Drop for DriverHub {
    fn drop(&mut self) {
        self.shutdown.cancel();
        // Best-effort: drop the Sender now so subscribers see Closed
        // immediately. The pump task does the same on its exit path; both
        // are idempotent (take() returns None the second time).
        let _ = self.tx.lock().unwrap().take();
    }
}
```

Workspace dep additions (root `Cargo.toml`): `tokio-util = { version = "0.7", features = ["sync"] }` (the `sync` feature gates `CancellationToken`; do not use `rt` — that's for `TaskTracker`).

`std::sync::Mutex` is intentional: the critical sections are tiny (one `as_ref()` + clone, or one `take()`), and nothing inside them awaits. Tokio guidance permits std mutexes for short, sync-only critical sections.

The pump's `JoinHandle` is intentionally **not** retained by the hub. Tokio tasks run independently of their handles; we control lifetime via the `CancellationToken`, not via aborting the handle. This was a finding in the engineering review (W1).

### Pump task

```rust
async fn pump(
    driver: Arc<dyn Driver>,
    tx_slot: TxSlot,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            got = driver.recv() => match got {
                Ok(event) => {
                    // Lock + send is sync (no .await inside the guard).
                    let g = tx_slot.lock().unwrap();
                    match g.as_ref() {
                        Some(tx) => { let _ = tx.send(event); }  // Err = no subscribers; fine
                        None => break,                            // hub dropped concurrently
                    }
                }
                Err(DriverError::Closed) => break,
                Err(e) => {
                    tracing::debug!(error = ?e, "driver hub: recv error, stopping pump");
                    break;
                }
            }
        }
    }
    // On exit by ANY path (driver-closed, shutdown, or already-cleared slot),
    // drop the Sender so existing subscribers see RecvError::Closed.
    let _ = tx_slot.lock().unwrap().take();
}
```

`biased` mirrors the recorder pattern in Plan 1: shutdown is checked before recv, so cancellation is observed promptly. The `take()` at the end is what makes `wait_for_close` keep working for the recorder — it's the fix for C1 from the engineering review.

### Send path

`hub.send()` is a thin async pass-through to `driver.send()`. The hub does not add a mutex on `send` — that's the `Driver` impl's responsibility.

**Driver trait contract (clarified here, no trait change):** Implementations of `Driver` must allow concurrent `&self` calls to `send`. Verified for existing impls:
- `MockDriver::send` — uses `std::sync::Mutex<Vec<RawEvent>>`; concurrent senders serialize internally.
- `StdioDriver::send` — uses `tokio::sync::Mutex<Stdout>`; concurrent senders serialize on the stdout lock.

Plan 2b must verify this property for `InterceptionDriver` before adopting it; per Interception docs, `interception_send` is safe per-context.

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

**Critical:** `start_recording` MUST call `hub.subscribe()` **synchronously on the caller's thread** and move the resulting `Receiver` into the spawned task. Subscribing inside the spawned task introduces a race with the pump (see API invariant in the `Type` section). If `subscribe()` returns `None` (hub already shut down), `start_recording` returns a `RecordingHandle` whose task immediately exits — `finish()` / `wait_for_close()` resolve to an empty `Vec<Step>`.

```rust
pub fn start_recording(hub: Arc<DriverHub>, passthrough: bool) -> RecordingHandle {
    let rx = hub.subscribe();   // sync, in caller thread
    let (stop_tx, mut stop_rx) = oneshot::channel();
    let buf: Arc<Mutex<Vec<TimedEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let buf_task = buf.clone();
    let join = tokio::spawn(async move {
        let mut rx = match rx {
            Some(rx) => rx,
            None => return Vec::new(),  // hub closed before we got here
        };
        loop {
            tokio::select! {
                biased;
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
                },
                _ = &mut stop_rx => break,
            }
        }
        std::mem::take(&mut *buf_task.lock().await)
    });
    RecordingHandle { stop_tx: Some(stop_tx), join }
}
```

Existing tests rewrap their `MockDriver` in `DriverHub::start(...)`. The subscribe-before-inject invariant now holds **because** `start_recording` subscribes synchronously before returning — any `drv.inject(...)` after the call is guaranteed to be observed.

### hotkey

`crates/hotkey/src/lib.rs`:

```rust
// Before:
pub fn start_listener(driver: Arc<dyn Driver>, registry: HotkeyRegistry, out_tx: mpsc::UnboundedSender<HotkeyHit>) -> ListenerHandle

// After:
pub fn start_listener(hub: Arc<DriverHub>, registry: HotkeyRegistry, out_tx: mpsc::UnboundedSender<HotkeyHit>) -> ListenerHandle
```

Same shape as recorder: `let rx = hub.subscribe();` synchronously in the caller, move `Option<Receiver>` into the spawn, exit early on `None`. Match on `rx.recv()` with `Lagged` → `debug!`, `Closed` → `break`. Mouse events are ignored exactly as today.

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

Hubs are scoped **per command**, not built at startup. Rationale: `cmd_play` and `cmd_record` need a driver but `cmd_list` / `cmd_delete` do not; building a global hub at startup means `cmd_play` runs a pump task that reads from stdin needlessly while playback runs (visible weirdness for the user, plus a benign race at drop time).

`main.rs` stays driver-less:

```rust
let res: Result<()> = match cli.cmd {
    Cmd::Record { name } => commands::cmd_record(&root, &name).await,
    Cmd::Play   { name } => commands::cmd_play(&root, &name).await,
    Cmd::List            => commands::cmd_list(&root),
    Cmd::Delete { name } => commands::cmd_delete(&root, &name),
};
```

`commands.rs`:

```rust
pub async fn cmd_record(root: &Path, name: &str) -> Result<()> {
    let drv: Arc<dyn Driver> = Arc::new(StdioDriver::new());
    let hub = DriverHub::start(drv);
    let handle = start_recording(hub, false);
    let steps = handle.wait_for_close().await?;
    // ... unchanged from Plan 1
}

pub async fn cmd_play(root: &Path, name: &str) -> Result<()> {
    // ... load + override Loop/Toggle unchanged
    let drv: Arc<dyn Driver> = Arc::new(StdioDriver::new());
    let hub = DriverHub::start(drv);
    play(hub, m).wait().await
}
```

`cmd_list` and `cmd_delete` are unchanged (no driver involvement).

`wait_for_close` keeps working because the hub's pump propagates `Closed` from `StdioDriver` (stdin EOF) to the broadcast Sender drop, which the recorder sees as `RecvError::Closed`. This is what C1 from the engineering review fixes.

## Testing

### New hub tests (`crates/driver/src/hub.rs`)

All tests use `DriverHub::start(...)` and subscribe **before** injecting. They use `MockDriver` unless noted.

- `subscribe_receives_pumped_events` — subscribe, then `MockDriver::inject(e)` → subscriber gets `Ok(e)`.
- `two_subscribers_each_receive_every_event` — two subscribes before inject; both see all events in order.
- `send_reaches_underlying_driver` — `hub.send(e)` → `MockDriver::drain_sent()` contains `e`.
- `lagged_subscriber_gets_lagged_error_then_continues` — fill the buffer past capacity from one subscriber's perspective (a slow consumer); subsequent `recv()` returns `Lagged(n)` then resumes.
- `pump_exits_propagates_closed_to_subscribers` — `Driver` that returns `Closed` on the first `recv()`. Subscriber's `rx.recv()` resolves to `Err(RecvError::Closed)` within a short timeout. **This is the regression test for C1.** A second `hub.subscribe()` after the pump has exited returns `None`.
- `drop_cancels_pump_and_closes_subscribers` — subscribe, drop the `Arc<DriverHub>`, the subscriber's next `recv()` returns `Err(RecvError::Closed)` within a short timeout.
- `subscribe_inside_spawn_can_lose_events_documenting_invariant` — *documentation test* that subscribes inside a spawned task and demonstrates that an inject racing with the spawn can be missed. Marked `#[ignore]` (or just an assertion that we got either 0 or 1 event — both are valid outcomes), with a doc-comment pointing to the API invariant. Purpose: when someone in the future tries to "fix" the apparent race, the comment explains why subscribe-in-caller is mandatory.

### Migrated tests

- `recorder` tests: same assertions, but `Arc::new(MockDriver::new())` is replaced by `DriverHub::start(Arc::new(MockDriver::new()))`. The `AlwaysClosed`-style driver in `wait_for_close_resolves_when_driver_closes` still works because the hub now propagates `Closed` to subscribers (C1 fix).
- `hotkey` tests: same migration. The `bind_and_unbind_round_trip` test doesn't use a driver and stays unchanged.
- `player` tests: same migration. The player doesn't subscribe, just calls `hub.send()`.

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
2. **Latency.** Each event goes through one extra channel hop (`driver.recv → tx.send → rx.recv`). Empirically tens of µs on tokio's broadcast; well within the design budget of <1 ms.
3. **`subscribe()` returning `None` is observable.** Callers must handle the case (recorder returns empty `Vec<Step>`, hotkey listener no-ops). This is a small API ergonomics cost in exchange for correct Closed propagation. Documented in the API invariant.

(The "subscribe-before-emit" race that previously appeared here is now an API invariant enforced by the consumer pattern — see the `Type` section.)

## Out of Scope (becomes Plan 2b)

- `rm-driver-interception` crate.
- `driver::detect_status()` and `install_driver` CLI commands.
- Bundling `install-interception.exe`.
- Manual test plan against real hardware.

## Acceptance

Plan 2a is "done" when:
- `cargo test --workspace` is green on Windows CI.
- All Plan 1 behavior is preserved (`record_save_load_play_roundtrip` e2e still passes).
- The new `concurrent_recorder_and_hotkey_share_hub` e2e passes.
- The C1 regression test `pump_exits_propagates_closed_to_subscribers` passes.
- No new system dependency is required to run the test suite.
- The original stub `plans/2026-05-26-rust-macro-plan-2-real-driver.md` is **deleted** in the same PR that lands Plan 2a's implementation. Replaced by:
  - `plans/2026-05-26-rust-macro-plan-2a-driverhub.md` — the writing-plans output for this spec.
  - `plans/2026-05-26-rust-macro-plan-2b-real-driver.md` — a fresh stub for the Interception work (scope: `rm-driver-interception` crate, `detect_status`, installer bundling, CLI `driver` subcommand).
