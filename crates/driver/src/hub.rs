//! `DriverHub` — broadcast multiplexer over the `Driver` trait. See the spec
//! at `docs/superpowers/specs/2026-05-26-rust-macro-plan-2a-driverhub-design.md`.

use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::{Driver, DriverError, RawEvent};

const BROADCAST_CAPACITY: usize = 256;

/// Shared slot holding the broadcast Sender. Wrapped in `Option` so the pump
/// task can `take()` it on exit, which drops the last Sender and gives every
/// existing `broadcast::Receiver` `RecvError::Closed`. Held by both the hub
/// (for `subscribe`) and the pump task (for emitting + clearing on exit).
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
}
