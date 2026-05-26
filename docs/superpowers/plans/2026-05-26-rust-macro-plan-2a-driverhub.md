# rust-macro — Plan 2a: DriverHub + Consumer Refactor

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `DriverHub` (broadcast multiplexer over the existing `Driver` trait) to `crates/driver`, then refactor `recorder` / `hotkey` / `player` / `cli` to consume `Arc<DriverHub>` instead of `Arc<dyn Driver>`. End state: `recorder` and `hotkey` can run concurrently against the same hub; CLI builds a hub per driver-using command; `cargo test --workspace` is green on Windows; all Plan 1 e2e behavior preserved.

**Architecture:** Pump task owns a `broadcast::Sender<RawEvent>` shared with the hub via `Arc<Mutex<Option<Sender>>>`. On exit (driver `Closed`, cancellation token, or hub drop), pump `take()`s the Sender; subscribers see `RecvError::Closed`. Consumers must call `hub.subscribe()` **synchronously on the caller thread** and move the `broadcast::Receiver` into their spawned task — this prevents a race with the pump.

**Tech Stack:** Plan 1 stack + `tokio-util = "0.7"` with the `sync` feature (for `CancellationToken`). `tokio::sync::broadcast` (already part of `tokio = { features = ["full"] }`).

**Parent spec:** `docs/superpowers/specs/2026-05-26-rust-macro-plan-2a-driverhub-design.md`

---

## File Map

```
crates/driver/
  Cargo.toml                  ← Modify: add tokio-util dep
  src/
    lib.rs                    ← Modify: pub mod hub; pub use hub::DriverHub;
    hub.rs                    ← NEW: DriverHub, pump, tests
    mock.rs                   ← Unchanged

crates/recorder/
  src/lib.rs                  ← Modify: signature Arc<dyn Driver> -> Arc<DriverHub>;
                                subscribe-in-caller pattern; handle Lagged/Closed;
                                migrate tests to DriverHub::start(Arc::new(MockDriver::new()))

crates/hotkey/
  src/lib.rs                  ← Modify: same pattern as recorder

crates/player/
  src/lib.rs                  ← Modify: signature Arc<dyn Driver> -> Arc<DriverHub>;
                                hub.send() in run_step; migrate tests

crates/cli/
  src/commands.rs             ← Modify: cmd_record + cmd_play build a hub per command
  tests/e2e.rs                ← Modify: existing test wraps MockDriver in hub;
                                NEW test: concurrent_recorder_and_hotkey_share_hub

Cargo.toml                    ← Modify: workspace dep tokio-util

docs/superpowers/plans/
  2026-05-26-rust-macro-plan-2-real-driver.md    ← DELETE
  2026-05-26-rust-macro-plan-2b-real-driver.md   ← NEW stub
```

---

## Task 1 — Workspace dep + hub.rs scaffolding

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Modify: `crates/driver/Cargo.toml`
- Create: `crates/driver/src/hub.rs`
- Modify: `crates/driver/src/lib.rs`

- [ ] **Step 1: Add `tokio-util` to workspace dependencies**

In `Cargo.toml` (workspace root), inside `[workspace.dependencies]`, add the line:

```toml
tokio-util = { version = "0.7", features = ["sync"] }
```

The `sync` feature gates `CancellationToken`. Do **not** use the `rt` feature — that's for `TaskTracker` and is wrong here.

- [ ] **Step 2: Use the new dep in `rm-driver`**

In `crates/driver/Cargo.toml`, under `[dependencies]`, add:

```toml
tokio-util.workspace = true
```

- [ ] **Step 3: Create an empty `hub.rs`**

Create `crates/driver/src/hub.rs` with this placeholder content:

```rust
//! `DriverHub` — broadcast multiplexer over the `Driver` trait. See the spec
//! at `docs/superpowers/specs/2026-05-26-rust-macro-plan-2a-driverhub-design.md`.

// Body added in Task 2.
```

- [ ] **Step 4: Wire `hub` into `lib.rs`**

In `crates/driver/src/lib.rs`, near the existing `pub mod mock;` line, add:

```rust
pub mod hub;

pub use hub::DriverHub;
```

- [ ] **Step 5: Verify the workspace builds**

Run: `cargo build -p rm-driver`
Expected: builds successfully with no warnings about `DriverHub` (it's just a module path with no items yet, which is fine).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/driver/Cargo.toml crates/driver/src/hub.rs crates/driver/src/lib.rs
git commit -m "feat(driver): scaffold DriverHub module + tokio-util dep"
```

---

## Task 2 — DriverHub: minimal `start` / `subscribe` / `send` (TDD)

**Files:**
- Modify: `crates/driver/src/hub.rs`

This task lands the full DriverHub + pump implementation upfront (including the `take()`-on-exit logic). Subsequent tasks add tests that exercise specific properties; using TDD on the lifecycle bits is impractical because the implementation is small and tightly coupled. The first round of tests in this task exercise the happy path.

- [ ] **Step 1: Write failing tests for the happy path**

Replace the body of `crates/driver/src/hub.rs` with:

```rust
//! `DriverHub` — broadcast multiplexer over the `Driver` trait. See the spec
//! at `docs/superpowers/specs/2026-05-26-rust-macro-plan-2a-driverhub-design.md`.

use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::{Driver, DriverError, RawEvent};

const BROADCAST_CAPACITY: usize = 256;

/// Shared slot holding the broadcast `Sender`. Wrapped in `Option` so the pump
/// task can `take()` it on exit, which drops the last `Sender` and gives every
/// existing `broadcast::Receiver` `RecvError::Closed` on its next `recv()`.
type TxSlot = Arc<Mutex<Option<broadcast::Sender<RawEvent>>>>;

/// A broadcast multiplexer over a single `Driver`. Spawns a pump task that
/// drains `driver.recv()` and fans each event out to every subscriber.
///
/// **API invariant — subscribe-before-emit.** Callers that will spawn a task
/// to consume events MUST call [`DriverHub::subscribe`] synchronously on the
/// caller thread, then move the returned `Receiver` into the spawned task.
/// Subscribing inside the spawned task creates a race: the pump can deliver
/// an injected event before the task's subscribe call lands, silently
/// dropping the event.
pub struct DriverHub {
    driver: Arc<dyn Driver>,
    tx: TxSlot,
    shutdown: CancellationToken,
}

impl DriverHub {
    /// Construct a hub over `driver`, spawn the internal pump task, return
    /// an `Arc<DriverHub>`. Clone freely. The pump runs until *either* the
    /// underlying driver returns `DriverError::Closed`, *or* the last `Arc`
    /// is dropped (triggering `shutdown.cancel()`). Whichever happens first,
    /// the pump drops the broadcast `Sender`, so all existing receivers see
    /// `RecvError::Closed` on their next `recv()`.
    pub fn start(driver: Arc<dyn Driver>) -> Arc<Self> {
        let (tx, _seed_rx) = broadcast::channel(BROADCAST_CAPACITY);
        let tx_slot: TxSlot = Arc::new(Mutex::new(Some(tx)));
        let shutdown = CancellationToken::new();

        tokio::spawn(pump(driver.clone(), tx_slot.clone(), shutdown.clone()));
        Arc::new(Self {
            driver,
            tx: tx_slot,
            shutdown,
        })
    }

    /// New subscriber. Returns `None` if the hub has already shut down
    /// (driver closed or hub being dropped). See the type-level invariant
    /// about subscribing on the caller thread.
    pub fn subscribe(&self) -> Option<broadcast::Receiver<RawEvent>> {
        self.tx
            .lock()
            .unwrap()
            .as_ref()
            .map(broadcast::Sender::subscribe)
    }

    /// Emit an event toward the OS. Direct passthrough to the inner driver.
    /// Concurrent callers are serialized by the driver impl (see the spec's
    /// "Send path" section).
    pub async fn send(&self, e: RawEvent) -> Result<(), DriverError> {
        self.driver.send(e).await
    }
}

impl Drop for DriverHub {
    fn drop(&mut self) {
        self.shutdown.cancel();
        // Best-effort: drop the Sender now so subscribers see Closed
        // immediately. The pump task does the same on its exit path;
        // both are idempotent (take() returns None the second time).
        let _ = self.tx.lock().unwrap().take();
    }
}

async fn pump(driver: Arc<dyn Driver>, tx_slot: TxSlot, shutdown: CancellationToken) {
    loop {
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => break,
            got = driver.recv() => match got {
                Ok(event) => {
                    // Lock + send is sync; no .await inside the guard.
                    let g = tx_slot.lock().unwrap();
                    match g.as_ref() {
                        Some(tx) => { let _ = tx.send(event); }  // Err = no subscribers; fine
                        None => break,                            // hub dropped concurrently
                    }
                }
                Err(DriverError::Closed) => break,
                Err(e) => {
                    debug!(error = ?e, "driver hub: recv error, stopping pump");
                    break;
                }
            }
        }
    }
    // On exit by ANY path (driver-closed, shutdown, or already-cleared slot),
    // drop the Sender so existing subscribers see RecvError::Closed.
    let _ = tx_slot.lock().unwrap().take();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock::MockDriver;
    use crate::KeyCode;
    use std::time::Duration;

    #[tokio::test]
    async fn subscribe_receives_pumped_events() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let mut rx = hub.subscribe().expect("subscribe before pump exit");

        drv.inject(RawEvent::KeyDown { key: KeyCode::A });

        let got = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("recv timed out")
            .expect("recv error");
        assert_eq!(got, RawEvent::KeyDown { key: KeyCode::A });
    }

    #[tokio::test]
    async fn two_subscribers_each_receive_every_event() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let mut rx1 = hub.subscribe().unwrap();
        let mut rx2 = hub.subscribe().unwrap();

        drv.inject(RawEvent::KeyDown { key: KeyCode::A });
        drv.inject(RawEvent::KeyDown { key: KeyCode::B });

        for rx in [&mut rx1, &mut rx2] {
            let e1 = tokio::time::timeout(Duration::from_millis(200), rx.recv())
                .await
                .unwrap()
                .unwrap();
            let e2 = tokio::time::timeout(Duration::from_millis(200), rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(e1, RawEvent::KeyDown { key: KeyCode::A });
            assert_eq!(e2, RawEvent::KeyDown { key: KeyCode::B });
        }
    }

    #[tokio::test]
    async fn send_reaches_underlying_driver() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());

        hub.send(RawEvent::KeyDown { key: KeyCode::C })
            .await
            .unwrap();
        hub.send(RawEvent::KeyUp { key: KeyCode::C }).await.unwrap();

        let sent = drv.drain_sent();
        assert_eq!(
            sent,
            vec![
                RawEvent::KeyDown { key: KeyCode::C },
                RawEvent::KeyUp { key: KeyCode::C },
            ]
        );
    }
}
```

- [ ] **Step 2: Run the new tests**

Run: `cargo test -p rm-driver hub::tests`
Expected: 3 tests pass.

- [ ] **Step 3: Run the whole driver crate test suite to confirm nothing regressed**

Run: `cargo test -p rm-driver`
Expected: all tests pass (existing mock and `RawEvent` serde tests + the 3 new hub tests).

- [ ] **Step 4: Commit**

```bash
git add crates/driver/src/hub.rs
git commit -m "feat(driver): DriverHub broadcast multiplexer with subscribe/send happy path"
```

---

## Task 3 — DriverHub: `Closed` propagation regression test (C1)

**Files:**
- Modify: `crates/driver/src/hub.rs`

This task adds the test that locks in the C1 fix: when the underlying driver returns `Closed`, the pump drops the broadcast Sender, and existing subscribers see `RecvError::Closed`. The implementation already supports this (the `take()` at the end of `pump`); this task is purely about putting the regression test in place.

- [ ] **Step 1: Add the test**

In `crates/driver/src/hub.rs`, inside the existing `#[cfg(test)] mod tests` block, add:

```rust
    #[tokio::test]
    async fn pump_exits_propagates_closed_to_subscribers() {
        // A Driver that immediately returns Closed on recv. Send is irrelevant.
        struct AlwaysClosed;
        #[async_trait::async_trait]
        impl Driver for AlwaysClosed {
            async fn send(&self, _: RawEvent) -> Result<(), DriverError> {
                Ok(())
            }
            async fn recv(&self) -> Result<RawEvent, DriverError> {
                Err(DriverError::Closed)
            }
        }

        let hub = DriverHub::start(Arc::new(AlwaysClosed));
        // Subscribe synchronously before the pump task gets to run.
        let mut rx = hub.subscribe().expect("subscribe before pump exit");

        // The next recv should resolve to Closed once the pump observes the
        // driver's Closed and drops the Sender.
        let result = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;
        assert!(
            matches!(result, Ok(Err(broadcast::error::RecvError::Closed))),
            "expected Ok(Err(Closed)), got {result:?}"
        );

        // After the pump has exited, new subscribes return None.
        assert!(
            hub.subscribe().is_none(),
            "subscribe after pump exit should return None"
        );
    }
```

You may also need an additional import at the top of the test module:

```rust
    use async_trait::async_trait;
```

(only if your linter complains; the `#[async_trait::async_trait]` form above doesn't require it).

- [ ] **Step 2: Run the test**

Run: `cargo test -p rm-driver hub::tests::pump_exits_propagates_closed_to_subscribers`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/driver/src/hub.rs
git commit -m "test(driver): C1 regression — pump exit propagates Closed to subscribers"
```

---

## Task 4 — DriverHub: drop-time shutdown test

**Files:**
- Modify: `crates/driver/src/hub.rs`

- [ ] **Step 1: Add the test**

Inside the existing `#[cfg(test)] mod tests` block, add:

```rust
    #[tokio::test]
    async fn drop_cancels_pump_and_closes_subscribers() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv);
        let mut rx = hub.subscribe().unwrap();

        drop(hub);

        let result = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;
        assert!(
            matches!(result, Ok(Err(broadcast::error::RecvError::Closed))),
            "expected Closed after hub drop, got {result:?}"
        );
    }
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p rm-driver hub::tests::drop_cancels_pump_and_closes_subscribers`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/driver/src/hub.rs
git commit -m "test(driver): hub drop propagates Closed to subscribers"
```

---

## Task 5 — DriverHub: lagged subscriber test

**Files:**
- Modify: `crates/driver/src/hub.rs`

- [ ] **Step 1: Add the test**

Inside the test module:

```rust
    #[tokio::test]
    async fn lagged_subscriber_gets_lagged_error_then_continues() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let mut rx = hub.subscribe().unwrap();

        // Inject more events than the broadcast capacity (256). The subscriber
        // never reads in between, so it should fall behind.
        for _ in 0..(BROADCAST_CAPACITY as u32 + 50) {
            drv.inject(RawEvent::KeyDown { key: KeyCode::A });
        }
        // Let the pump drain into the broadcast channel.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // First recv should report the lag, not an event.
        let first = rx.recv().await;
        assert!(
            matches!(first, Err(broadcast::error::RecvError::Lagged(_))),
            "expected Lagged, got {first:?}"
        );

        // Subsequent recvs continue delivering buffered events (not Closed).
        let next = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("timed out");
        assert!(
            matches!(next, Ok(RawEvent::KeyDown { .. })),
            "expected Ok event after Lagged, got {next:?}"
        );
    }
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p rm-driver hub::tests::lagged_subscriber_gets_lagged_error_then_continues`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add crates/driver/src/hub.rs
git commit -m "test(driver): lagged subscriber sees Lagged then continues"
```

---

## Task 6 — DriverHub: invariant documentation test

**Files:**
- Modify: `crates/driver/src/hub.rs`

This is a non-asserting test that documents *why* the API invariant exists. It subscribes inside a spawned task (the wrong way) and shows that injected events can be lost. Future maintainers tempted to "fix" the apparent race in `start_recording` will see this test and understand why subscribe-in-caller is mandatory.

- [ ] **Step 1: Add the test**

Inside the test module:

```rust
    /// **Documentation test for the subscribe-before-emit invariant.**
    ///
    /// Subscribing inside a spawned task — instead of synchronously on the
    /// caller thread — races with the pump. The pump can deliver an injected
    /// event before the task's subscribe call lands, silently dropping it.
    /// This test demonstrates the race: it passes whether the event was lost
    /// (0 events seen) or caught in time (1 event seen). The point is the
    /// documentation, not the assertion.
    #[tokio::test]
    async fn subscribe_inside_spawn_can_lose_events_documenting_invariant() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());

        let hub2 = hub.clone();
        let join = tokio::spawn(async move {
            // WRONG PATTERN: subscribe inside the task.
            let mut rx = match hub2.subscribe() {
                Some(rx) => rx,
                None => return 0u32,
            };
            let mut got = 0u32;
            while let Ok(res) =
                tokio::time::timeout(Duration::from_millis(50), rx.recv()).await
            {
                if res.is_ok() {
                    got += 1;
                }
            }
            got
        });

        // Inject immediately — the spawned task may or may not have subscribed yet.
        drv.inject(RawEvent::KeyDown { key: KeyCode::A });

        let count = join.await.unwrap();
        assert!(count <= 1, "expected 0 or 1 events (race), got {count}");
    }
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p rm-driver hub::tests::subscribe_inside_spawn_can_lose_events_documenting_invariant`
Expected: PASS (either 0 or 1 events; both valid).

- [ ] **Step 3: Run the full driver test suite to confirm everything is green**

Run: `cargo test -p rm-driver`
Expected: 6 hub tests + existing mock + RawEvent tests all pass.

- [ ] **Step 4: Commit**

```bash
git add crates/driver/src/hub.rs
git commit -m "test(driver): document subscribe-before-emit invariant"
```

---

## Task 7 — Refactor `recorder` to consume `Arc<DriverHub>`

**Files:**
- Modify: `crates/recorder/src/lib.rs`

- [ ] **Step 1: Rewrite `start_recording` and its callers in the test module**

Replace `crates/recorder/src/lib.rs` with the version below. The key changes from Plan 1:
- `start_recording` takes `Arc<DriverHub>` instead of `Arc<dyn Driver>`.
- `hub.subscribe()` is called **synchronously on the caller thread**; the resulting `Option<broadcast::Receiver<_>>` is moved into the spawned task.
- The `select!` matches on `rx.recv()` (a broadcast receiver) instead of `driver.recv()`; `Lagged` is logged via `warn!`, `Closed` breaks the loop.
- Passthrough calls `hub.send()` instead of `driver.send()`.
- Tests wrap `MockDriver` in `DriverHub::start(...)`.

```rust
pub mod compile;
pub use compile::{compile_events, TimedEvent};

use std::sync::Arc;
use std::time::Instant;

use rm_driver::DriverHub;
use rm_error::Result;
use rm_macro_model::Step;
use tokio::sync::{broadcast, oneshot, Mutex};
use tracing::{debug, warn};

/// Handle to a running recording. Two ways to end:
///   * `finish().await` — sends an explicit stop signal, then awaits.
///   * `wait_for_close().await` — does NOT send stop; awaits until the hub
///     itself shuts down (driver closed or hub dropped). Use this when the
///     caller knows the event source has finite input.
///
/// Dropping the handle without calling either also cancels the task (drops
/// the stop sender, which fires the stop branch of the recorder's `select!`).
pub struct RecordingHandle {
    stop_tx: Option<oneshot::Sender<()>>,
    join: tokio::task::JoinHandle<Vec<TimedEvent>>,
}

impl RecordingHandle {
    /// Send a stop signal and await the recorder task.
    pub async fn finish(mut self) -> Result<Vec<Step>> {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        let raw = self
            .join
            .await
            .map_err(|e| rm_error::AppError::Other(format!("recorder task panicked: {e}")))?;
        Ok(compile_events(&raw))
    }

    /// Await the recorder task without sending a stop signal — the task will
    /// exit on its own when the hub shuts down (driver closes or hub drops).
    pub async fn wait_for_close(self) -> Result<Vec<Step>> {
        let raw = self
            .join
            .await
            .map_err(|e| rm_error::AppError::Other(format!("recorder task panicked: {e}")))?;
        // self.stop_tx drops here, after the task is already complete — no effect.
        Ok(compile_events(&raw))
    }
}

/// Start a recording. Reads events from the hub's broadcast stream and
/// timestamps each one. When `passthrough` is true, each captured event is
/// re-emitted via `hub.send()` so the OS still sees it (production behavior).
///
/// **Important**: `hub.subscribe()` is called synchronously on the caller's
/// thread before spawning, per the DriverHub API invariant. If the hub is
/// already shut down, the task exits immediately with an empty buffer.
///
/// The `select!` is `biased` so that pending events are processed before
/// checking the stop signal — this guarantees a final burst is captured
/// before a manual `finish()` short-circuits.
pub fn start_recording(hub: Arc<DriverHub>, passthrough: bool) -> RecordingHandle {
    let rx = hub.subscribe();
    let (stop_tx, mut stop_rx) = oneshot::channel();
    let buf: Arc<Mutex<Vec<TimedEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let buf_task = buf.clone();
    let join = tokio::spawn(async move {
        let mut rx = match rx {
            Some(rx) => rx,
            None => return Vec::new(),
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
                        warn!(lagged = n, "recorder: dropped events under load");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("recorder: hub closed");
                        break;
                    }
                },
                _ = &mut stop_rx => {
                    debug!("recorder: stop signal received");
                    break;
                }
            }
        }
        std::mem::take(&mut *buf_task.lock().await)
    });
    RecordingHandle {
        stop_tx: Some(stop_tx),
        join,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rm_driver::mock::MockDriver;
    use rm_driver::{Driver, DriverError, RawEvent};
    use rm_macro_model::{KeyCode, Step};

    #[tokio::test]
    async fn records_injected_events_no_passthrough() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let h = start_recording(hub, false);

        drv.inject(RawEvent::KeyDown { key: KeyCode::A });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        drv.inject(RawEvent::KeyUp { key: KeyCode::A });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let steps = h.finish().await.unwrap();
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
        let hub = DriverHub::start(drv.clone());
        let h = start_recording(hub, true);

        drv.inject(RawEvent::KeyDown { key: KeyCode::B });
        drv.inject(RawEvent::KeyUp { key: KeyCode::B });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let _ = h.finish().await.unwrap();

        let sent = drv.drain_sent();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0], RawEvent::KeyDown { key: KeyCode::B });
        assert_eq!(sent[1], RawEvent::KeyUp { key: KeyCode::B });
    }

    #[tokio::test]
    async fn wait_for_close_resolves_when_driver_closes() {
        // Driver that returns Closed immediately on recv. With the hub in
        // between, the pump sees Closed, drops the Sender, and the recorder's
        // subscribe Receiver resolves to Err(Closed) — same observable
        // behavior as Plan 1's direct-driver path.
        struct AlwaysClosed;
        #[async_trait::async_trait]
        impl Driver for AlwaysClosed {
            async fn send(&self, _e: RawEvent) -> std::result::Result<(), DriverError> {
                Ok(())
            }
            async fn recv(&self) -> std::result::Result<RawEvent, DriverError> {
                Err(DriverError::Closed)
            }
        }
        let drv: Arc<dyn Driver> = Arc::new(AlwaysClosed);
        let hub = DriverHub::start(drv);
        let h = start_recording(hub, false);
        let steps = h.wait_for_close().await.unwrap();
        assert!(steps.is_empty());
    }
}
```

- [ ] **Step 2: Run the recorder tests**

Run: `cargo test -p rm-recorder`
Expected: all 3 tests pass.

- [ ] **Step 3: Verify the rest of the workspace still builds**

Run: `cargo build --workspace`
Expected: builds fail (because hotkey and player and CLI still pass `Arc<dyn Driver>` to functions that now expect `Arc<DriverHub>` — except recorder's signature changed, so any caller passing `Arc<dyn Driver>` breaks). This is expected; subsequent tasks fix it. Note which crates fail so the next task starts in the right place.

- [ ] **Step 4: Commit**

```bash
git add crates/recorder/src/lib.rs
git commit -m "refactor(recorder): consume Arc<DriverHub>; subscribe in caller; handle Lagged/Closed"
```

---

## Task 8 — Refactor `hotkey` to consume `Arc<DriverHub>`

**Files:**
- Modify: `crates/hotkey/src/lib.rs`

- [ ] **Step 1: Rewrite `start_listener` and its tests**

Replace `crates/hotkey/src/lib.rs` with:

```rust
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rm_driver::{DriverHub, RawEvent};
use rm_macro_model::{KeyCode, Modifier, Trigger};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
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
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn bind(&self, id: Uuid, mut trigger: Trigger) {
        let Trigger::Hotkey {
            ref mut modifiers, ..
        } = trigger;
        modifiers.sort();
        modifiers.dedup();
        self.inner.lock().await.by_id.insert(id, trigger);
    }

    pub async fn unbind(&self, id: Uuid) {
        self.inner.lock().await.by_id.remove(&id);
    }

    pub async fn match_pressed(&self, key: KeyCode, modifiers: &HashSet<Modifier>) -> Vec<Uuid> {
        let g = self.inner.lock().await;
        g.by_id
            .iter()
            .filter_map(|(id, t)| match t {
                Trigger::Hotkey {
                    key: tk,
                    modifiers: tm,
                } => {
                    let tm_set: HashSet<_> = tm.iter().copied().collect();
                    if *tk == key && tm_set == *modifiers {
                        Some(*id)
                    } else {
                        None
                    }
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
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        let _ = self.join.await;
    }
}

/// Spawn a task that reads from the hub's broadcast stream, tracks pressed
/// modifiers, and emits a `HotkeyHit` on `out_tx` for every key press that
/// matches a binding.
///
/// **Important**: `hub.subscribe()` is called synchronously on the caller's
/// thread before spawning. If the hub is already shut down, the task exits
/// immediately.
pub fn start_listener(
    hub: Arc<DriverHub>,
    registry: HotkeyRegistry,
    out_tx: mpsc::UnboundedSender<HotkeyHit>,
) -> ListenerHandle {
    let rx = hub.subscribe();
    let (stop_tx, mut stop_rx) = oneshot::channel();
    let join = tokio::spawn(async move {
        let mut rx = match rx {
            Some(rx) => rx,
            None => return,
        };
        let mut mods: HashSet<Modifier> = HashSet::new();
        loop {
            tokio::select! {
                _ = &mut stop_rx => { debug!("hotkey: stop"); break; }
                got = rx.recv() => match got {
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
                        if let Some(m) = key_as_modifier(key) {
                            mods.remove(&m);
                        }
                    }
                    Ok(_) => { /* mouse events not used for hotkeys in v1 */ }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!(lagged = n, "hotkey: dropped events");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        debug!("hotkey: hub closed");
                        break;
                    }
                }
            }
        }
    });
    ListenerHandle {
        stop_tx: Some(stop_tx),
        join,
    }
}

fn key_as_modifier(k: KeyCode) -> Option<Modifier> {
    match k {
        KeyCode::LShift | KeyCode::RShift => Some(Modifier::Shift),
        KeyCode::LCtrl | KeyCode::RCtrl => Some(Modifier::Ctrl),
        KeyCode::LAlt | KeyCode::RAlt => Some(Modifier::Alt),
        KeyCode::LWin | KeyCode::RWin => Some(Modifier::Win),
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
        r.bind(
            id,
            Trigger::Hotkey {
                key: KeyCode::F1,
                modifiers: vec![],
            },
        )
        .await;
        let mut s = HashSet::new();
        assert_eq!(r.match_pressed(KeyCode::F1, &s).await, vec![id]);
        r.unbind(id).await;
        assert!(r.match_pressed(KeyCode::F1, &s).await.is_empty());

        // Modifiers must match.
        r.bind(
            id,
            Trigger::Hotkey {
                key: KeyCode::F1,
                modifiers: vec![Modifier::Ctrl],
            },
        )
        .await;
        assert!(r.match_pressed(KeyCode::F1, &s).await.is_empty());
        s.insert(Modifier::Ctrl);
        assert_eq!(r.match_pressed(KeyCode::F1, &s).await, vec![id]);
    }

    #[tokio::test]
    async fn listener_dispatches_on_match() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let reg = HotkeyRegistry::new();
        let id = Uuid::new_v4();
        reg.bind(
            id,
            Trigger::Hotkey {
                key: KeyCode::F2,
                modifiers: vec![Modifier::Ctrl],
            },
        )
        .await;

        let (tx, mut rx) = mpsc::unbounded_channel();
        let handle = start_listener(hub, reg.clone(), tx);

        drv.inject(RawEvent::KeyDown {
            key: KeyCode::LCtrl,
        });
        drv.inject(RawEvent::KeyDown { key: KeyCode::F2 });

        let hit = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(hit, HotkeyHit(id));

        handle.shutdown().await;
    }

    #[tokio::test]
    async fn modifier_release_drops_match() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let reg = HotkeyRegistry::new();
        let id = Uuid::new_v4();
        reg.bind(
            id,
            Trigger::Hotkey {
                key: KeyCode::F3,
                modifiers: vec![Modifier::Ctrl],
            },
        )
        .await;

        let (tx, mut rx) = mpsc::unbounded_channel();
        let handle = start_listener(hub, reg.clone(), tx);

        drv.inject(RawEvent::KeyDown {
            key: KeyCode::LCtrl,
        });
        drv.inject(RawEvent::KeyUp {
            key: KeyCode::LCtrl,
        });
        drv.inject(RawEvent::KeyDown { key: KeyCode::F3 });

        let r = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
        assert!(r.is_err(), "expected no hit, got {:?}", r);

        handle.shutdown().await;
    }
}
```

- [ ] **Step 2: Run the hotkey tests**

Run: `cargo test -p rm-hotkey`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/hotkey/src/lib.rs
git commit -m "refactor(hotkey): consume Arc<DriverHub>; subscribe in caller"
```

---

## Task 9 — Refactor `player` to consume `Arc<DriverHub>`

**Files:**
- Modify: `crates/player/src/lib.rs`

The player only emits — it doesn't subscribe. This is the smallest refactor.

- [ ] **Step 1: Rewrite the player**

Replace `crates/player/src/lib.rs` with:

```rust
use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use rm_driver::{DriverHub, RawEvent};
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
    pub fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
    }

    pub async fn wait(self) -> Result<()> {
        self.join
            .await
            .map_err(|e| AppError::Other(format!("player task panicked: {e}")))?
    }
}

/// Spawn a player task to execute `macro_`. Returns immediately with a handle.
pub fn play(hub: Arc<DriverHub>, macro_: Macro) -> PlaybackHandle {
    let (stop_tx, stop_rx) = oneshot::channel();
    let join = tokio::spawn(async move { run(hub, &macro_, stop_rx).await });
    PlaybackHandle {
        stop_tx: Some(stop_tx),
        join,
    }
}

async fn run(hub: Arc<DriverHub>, m: &Macro, mut stop_rx: oneshot::Receiver<()>) -> Result<()> {
    debug!(macro_name = %m.name, mode = ?m.playback, "player: starting");
    let mut iter = playback_iter(m.playback);
    while iter.next() {
        for step in &m.steps {
            if stop_rx.try_recv().is_ok() {
                debug!("player: stop signal");
                return Ok(());
            }
            run_step(&hub, step).await?;
        }
    }
    Ok(())
}

async fn run_step(hub: &DriverHub, step: &Step) -> Result<()> {
    match step {
        Step::KeyPress { key, hold_ms } => {
            hub.send(RawEvent::KeyDown { key: *key })
                .await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
            tokio::time::sleep(Duration::from_millis((*hold_ms).into())).await;
            hub.send(RawEvent::KeyUp { key: *key })
                .await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }
        Step::KeyDown { key } => {
            hub.send(RawEvent::KeyDown { key: *key })
                .await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }
        Step::KeyUp { key } => {
            hub.send(RawEvent::KeyUp { key: *key })
                .await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }
        Step::MouseClick {
            button,
            hold_ms,
            at: _,
        } => {
            hub.send(RawEvent::MouseDown { button: *button })
                .await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
            tokio::time::sleep(Duration::from_millis((*hold_ms).into())).await;
            hub.send(RawEvent::MouseUp { button: *button })
                .await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }
        Step::MouseMove { to, mode: _ } => {
            hub.send(RawEvent::MouseMove { dx: to.x, dy: to.y })
                .await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }
        Step::MouseScroll { delta } => {
            hub.send(RawEvent::MouseWheel { delta: *delta })
                .await
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

struct PlaybackIter {
    remaining: Option<u64>,
}

impl PlaybackIter {
    fn next(&mut self) -> bool {
        match &mut self.remaining {
            None => true,
            Some(0) => false,
            Some(n) => {
                *n -= 1;
                true
            }
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
        let mut m = Macro::new(
            "t",
            Trigger::Hotkey {
                key: KeyCode::F1,
                modifiers: vec![],
            },
            playback,
        );
        m.steps = steps;
        m
    }

    #[tokio::test]
    async fn keypress_emits_down_then_up() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![Step::KeyPress {
                key: KeyCode::A,
                hold_ms: 5,
            }],
            PlaybackMode::Once,
        );
        play(hub, m).wait().await.unwrap();
        let sent = drv.drain_sent();
        assert_eq!(
            sent,
            vec![
                RawEvent::KeyDown { key: KeyCode::A },
                RawEvent::KeyUp { key: KeyCode::A },
            ]
        );
    }

    #[tokio::test]
    async fn wait_is_random_within_range() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![Step::Wait {
                min_ms: 10,
                max_ms: 20,
            }],
            PlaybackMode::Repeat { count: 5 },
        );
        play(hub, m).wait().await.unwrap();
        assert!(drv.sent_snapshot().is_empty());
    }

    #[tokio::test]
    async fn mouse_click_emits_down_up() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![Step::MouseClick {
                button: rm_macro_model::MouseButton::Left,
                hold_ms: 5,
                at: None,
            }],
            PlaybackMode::Once,
        );
        play(hub, m).wait().await.unwrap();
        let sent = drv.drain_sent();
        assert!(matches!(sent[0], RawEvent::MouseDown { .. }));
        assert!(matches!(sent[1], RawEvent::MouseUp { .. }));
    }

    #[tokio::test]
    async fn repeat_n_runs_n_times() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![Step::KeyPress {
                key: KeyCode::X,
                hold_ms: 0,
            }],
            PlaybackMode::Repeat { count: 4 },
        );
        play(hub, m).wait().await.unwrap();
        assert_eq!(drv.drain_sent().len(), 4 * 2);
    }

    #[tokio::test]
    async fn once_runs_once() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![Step::KeyPress {
                key: KeyCode::X,
                hold_ms: 0,
            }],
            PlaybackMode::Once,
        );
        play(hub, m).wait().await.unwrap();
        assert_eq!(drv.drain_sent().len(), 2);
    }

    #[tokio::test]
    async fn loop_stops_on_signal() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![
                Step::KeyPress {
                    key: KeyCode::X,
                    hold_ms: 1,
                },
                Step::Wait {
                    min_ms: 5,
                    max_ms: 5,
                },
            ],
            PlaybackMode::Loop,
        );
        let mut h = play(hub, m);
        tokio::time::sleep(Duration::from_millis(50)).await;
        h.stop();
        h.wait().await.unwrap();
        let count = drv.drain_sent().len();
        assert!(
            count > 0 && count.is_multiple_of(2),
            "sent count was {count}"
        );
    }
}
```

- [ ] **Step 2: Run the player tests**

Run: `cargo test -p rm-player`
Expected: 6 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/player/src/lib.rs
git commit -m "refactor(player): consume Arc<DriverHub>; emit via hub.send()"
```

---

## Task 10 — Refactor CLI commands to build a hub per command

**Files:**
- Modify: `crates/cli/src/commands.rs`

- [ ] **Step 1: Rewrite `cmd_record` and `cmd_play` to use the hub**

Replace `crates/cli/src/commands.rs` with:

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

/// Record from stdin (JSONL of RawEvent). The recorder exits naturally when
/// the hub propagates `Closed` (driven by stdin EOF), so this just awaits the
/// task without ever sending a stop signal.
pub async fn cmd_record(root: &Path, name: &str) -> Result<()> {
    let drv: Arc<dyn Driver> = Arc::new(StdioDriver::new());
    let hub = DriverHub::start(drv);
    let handle = start_recording(hub, false);
    let steps = handle.wait_for_close().await?;
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

pub async fn cmd_play(root: &Path, name: &str) -> Result<()> {
    let macros = load_all(root)?;
    let mut m = macros
        .into_iter()
        .find(|m| m.name == name)
        .ok_or_else(|| AppError::MacroNotFound(name.into()))?;
    // The CLI demo has no stop-hotkey or signal handler, so unbounded modes
    // would block the terminal forever. Override to Once with a note.
    if matches!(m.playback, PlaybackMode::Loop | PlaybackMode::Toggle) {
        eprintln!(
            "note: macro playback is {:?}; CLI overrides to Once \
                   (no stop signal available)",
            m.playback
        );
        m.playback = PlaybackMode::Once;
    }
    let drv: Arc<dyn Driver> = Arc::new(StdioDriver::new());
    let hub = DriverHub::start(drv);
    play(hub, m).wait().await
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
```

- [ ] **Step 2: Verify the CLI builds**

Run: `cargo build -p rm-cli`
Expected: builds successfully.

- [ ] **Step 3: Verify the Plan 1 e2e test still passes**

Run: `cargo test -p rm-cli --test e2e`
Expected: `record_save_load_play_roundtrip` passes (it uses `MockDriver` + `play` directly, not the CLI binary; we'll update it in the next task to use the hub).

If the existing e2e currently passes `Arc<dyn Driver>` to `play`, it will fail to compile — fix it as part of the next task.

- [ ] **Step 4: Commit**

```bash
git add crates/cli/src/commands.rs
git commit -m "refactor(cli): build DriverHub per command (cmd_record, cmd_play)"
```

---

## Task 11 — Update the existing e2e and add the concurrent-consumer test

**Files:**
- Modify: `crates/cli/tests/e2e.rs`

- [ ] **Step 1: Add the concurrent test and update the existing one**

Replace `crates/cli/tests/e2e.rs` with:

```rust
use std::sync::Arc;
use std::time::{Duration, Instant};

use rm_driver::mock::MockDriver;
use rm_driver::{DriverHub, RawEvent};
use rm_hotkey::{start_listener, HotkeyHit, HotkeyRegistry};
use rm_macro_model::{KeyCode, Macro, Modifier, PlaybackMode, Trigger};
use rm_player::play;
use rm_recorder::{compile_events, start_recording, TimedEvent};
use rm_storage::{load_macro, save_macro};
use tempfile::TempDir;
use tokio::sync::mpsc;
use uuid::Uuid;

#[tokio::test]
async fn record_save_load_play_roundtrip() {
    // 1. "Record" — synthesize a known sequence directly.
    let t0 = Instant::now();
    let raw = vec![
        TimedEvent {
            event: RawEvent::KeyDown { key: KeyCode::H },
            at: t0,
        },
        TimedEvent {
            event: RawEvent::KeyUp { key: KeyCode::H },
            at: t0 + Duration::from_millis(60),
        },
        TimedEvent {
            event: RawEvent::KeyDown { key: KeyCode::I },
            at: t0 + Duration::from_millis(150),
        },
        TimedEvent {
            event: RawEvent::KeyUp { key: KeyCode::I },
            at: t0 + Duration::from_millis(220),
        },
    ];
    let steps = compile_events(&raw);
    assert_eq!(steps.len(), 3, "expected H, Wait, I — got {steps:?}");

    // 2. Save.
    let tmp = TempDir::new().unwrap();
    let mut m = Macro::new(
        "hi",
        Trigger::Hotkey {
            key: KeyCode::F4,
            modifiers: vec![Modifier::Ctrl],
        },
        PlaybackMode::Once,
    );
    m.steps = steps;
    save_macro(tmp.path(), &m).unwrap();

    // 3. Load back.
    let loaded = load_macro(tmp.path(), m.id).unwrap();
    assert_eq!(loaded, m);

    // 4. Play through a MockDriver wrapped in a hub.
    let drv = Arc::new(MockDriver::new());
    let hub = DriverHub::start(drv.clone());
    play(hub, loaded).wait().await.unwrap();
    let sent = drv.drain_sent();

    assert_eq!(
        sent,
        vec![
            RawEvent::KeyDown { key: KeyCode::H },
            RawEvent::KeyUp { key: KeyCode::H },
            RawEvent::KeyDown { key: KeyCode::I },
            RawEvent::KeyUp { key: KeyCode::I },
        ]
    );
}

#[tokio::test]
async fn concurrent_recorder_and_hotkey_share_hub() {
    // Single hub fans out to both a hotkey listener and a recorder running
    // simultaneously. This is the scenario Plan 1 cannot express, because
    // its consumers each called Driver::recv() directly and would race.
    let drv = Arc::new(MockDriver::new());
    let hub = DriverHub::start(drv.clone());

    // Hotkey: Ctrl+F2 -> some macro id.
    let registry = HotkeyRegistry::new();
    let macro_id = Uuid::new_v4();
    registry
        .bind(
            macro_id,
            Trigger::Hotkey {
                key: KeyCode::F2,
                modifiers: vec![Modifier::Ctrl],
            },
        )
        .await;
    let (hit_tx, mut hit_rx) = mpsc::unbounded_channel::<HotkeyHit>();
    let hk_handle = start_listener(hub.clone(), registry.clone(), hit_tx);

    // Recorder: no passthrough (this is a test; we don't care about re-emit).
    let rec_handle = start_recording(hub.clone(), false);

    // Inject a chord that should both trigger the hotkey AND be recorded.
    drv.inject(RawEvent::KeyDown {
        key: KeyCode::LCtrl,
    });
    drv.inject(RawEvent::KeyDown { key: KeyCode::F2 });
    drv.inject(RawEvent::KeyUp { key: KeyCode::F2 });
    drv.inject(RawEvent::KeyUp {
        key: KeyCode::LCtrl,
    });

    // Let both consumers drain.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Hotkey saw the chord.
    let hit = tokio::time::timeout(Duration::from_millis(200), hit_rx.recv())
        .await
        .expect("hotkey timeout")
        .expect("hotkey channel closed");
    assert_eq!(hit, HotkeyHit(macro_id));

    // Recorder captured the events (compiled into Steps).
    let steps = rec_handle.finish().await.unwrap();
    assert!(
        !steps.is_empty(),
        "recorder should have captured events from the shared hub"
    );

    hk_handle.shutdown().await;
}
```

- [ ] **Step 2: Run the e2e tests**

Run: `cargo test -p rm-cli --test e2e`
Expected: 2 tests pass.

- [ ] **Step 3: Run the full workspace test suite to confirm nothing else regressed**

Run: `cargo test --workspace`
Expected: every crate's tests pass — driver (hub + mock + RawEvent), recorder, hotkey, player, storage, macro_model, error, cli e2e.

- [ ] **Step 4: Commit**

```bash
git add crates/cli/tests/e2e.rs
git commit -m "test(e2e): record/play roundtrip via hub; concurrent recorder+hotkey share one hub"
```

---

## Task 12 — Cleanup: replace Plan 2 stub with Plan 2b stub

**Files:**
- Delete: `docs/superpowers/plans/2026-05-26-rust-macro-plan-2-real-driver.md`
- Create: `docs/superpowers/plans/2026-05-26-rust-macro-plan-2b-real-driver.md`

- [ ] **Step 1: Delete the original stub**

```bash
git rm docs/superpowers/plans/2026-05-26-rust-macro-plan-2-real-driver.md
```

- [ ] **Step 2: Create the Plan 2b stub**

Create `docs/superpowers/plans/2026-05-26-rust-macro-plan-2b-real-driver.md` with:

```markdown
# rust-macro — Plan 2b: Real Interception Driver (stub)

**Goal:** Replace `MockDriver` with an `InterceptionDriver` backed by the real Interception kernel driver via the `interception-rs` crate. Add driver-status detection, the bundled installer flow, and a `driver` CLI subcommand. Builds on top of the `DriverHub` shipped in Plan 2a (no architectural changes to the multiplexer).

**Architecture:** New crate `rm-driver-interception` providing `InterceptionDriver: Driver`. `crates/driver` grows a `detect_status()` returning `{ NotInstalled, InstalledNotRunning, Running }` (implementation: query Windows service manager for `keyboard` / `mouse` Interception services + attempt to open an Interception context). `rm-cli` grows a `driver` subcommand: `status`, `install`.

**Verification before adopting `interception-rs`:** confirm that `InterceptionDriver::send` (and the underlying `interception_send`) is safe under concurrent `&self` calls — this is the trait contract the `DriverHub` send path relies on.

**Tech Stack:** Adds `interception-rs` (or a thin FFI binding to `interception.dll` if the crate isn't suitable on current Rust). Bundles the upstream Interception installer (`install-interception.exe`).

**Why a separate plan:** depends on having Interception installed on the dev machine + an admin reboot, so it cannot be CI-friendly. Plan 2a tests stay green throughout; the new Interception-dependent tests are gated behind a feature flag.

(Detailed tasks to be written when Plan 2a is merged and verified.)
```

- [ ] **Step 3: Verify the file tree is consistent**

Run: `git status`
Expected: shows the deleted `plan-2-real-driver.md` (staged) and the new `plan-2b-real-driver.md` (untracked).

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/plans/2026-05-26-rust-macro-plan-2b-real-driver.md
git commit -m "docs: replace Plan 2 stub with Plan 2b stub (post-2a)"
```

---

## Acceptance verification

After all tasks are committed, confirm the acceptance criteria from the spec hold.

- [ ] **Step 1: Full workspace test pass**

Run: `cargo test --workspace`
Expected: all tests green. Specifically watch for:
- `pump_exits_propagates_closed_to_subscribers` (C1 regression)
- `record_save_load_play_roundtrip` (Plan 1 e2e preserved)
- `concurrent_recorder_and_hotkey_share_hub` (Plan 2a's reason to exist)

- [ ] **Step 2: Clippy clean**

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 3: Format clean**

Run: `cargo fmt --all -- --check`
Expected: no diffs.

- [ ] **Step 4: Confirm no new system dependency**

The test run above should not have prompted for any driver install, admin elevation, or reboot. Plan 2a is CI-safe.
