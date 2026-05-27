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

    /// Drive the recording to completion, observing an external stop signal.
    /// Mirrors `rm_player::PlaybackHandle::run_with_stop`.
    ///
    /// When `external_stop_rx` fires before natural completion, the internal
    /// stop_tx is fired and we await the join. When the recorder ends first
    /// (stop_key, hub close, or error), external_stop_rx is dropped.
    pub async fn run_with_stop(
        mut self,
        external_stop_rx: oneshot::Receiver<()>,
    ) -> Result<Vec<Step>> {
        let join = self.join;
        tokio::pin!(join);
        let stop_tx = self.stop_tx.take();

        tokio::select! {
            biased;
            _ = external_stop_rx => {
                if let Some(tx) = stop_tx { let _ = tx.send(()); }
                let raw = (&mut join).await
                    .map_err(|e| rm_error::AppError::Other(format!("recorder task panicked: {e}")))?;
                Ok(compile_events(&raw))
            }
            result = &mut join => {
                drop(stop_tx);
                let raw = result
                    .map_err(|e| rm_error::AppError::Other(format!("recorder task panicked: {e}")))?;
                Ok(compile_events(&raw))
            }
        }
    }
}

/// Start a recording with optional stop-key filtering.
///
/// When `stop_key` is `Some(k)` and a `RawEvent::KeyDown { key: k }` arrives,
/// the event is dropped (not passthrough'd, not buffered) and the recorder
/// task exits cleanly. The matching `KeyUp` may or may not arrive (depends on
/// user release timing) — by that point the recorder is gone.
///
/// **Implementation order in the loop is critical:**
///   1. `rx.recv()` returns an event.
///   2. If it matches `stop_key` keydown → break the loop (no passthrough,
///      no buffer append).
///   3. Otherwise: passthrough send (if enabled), then buffer append.
///
/// This atomically prevents the stop key from leaking to either path.
///
/// **Important**: `hub.subscribe()` is called synchronously on the caller's
/// thread before spawning, per the DriverHub API invariant. If the hub is
/// already shut down, the task exits immediately with an empty buffer.
///
/// The `select!` is `biased` so that pending events are processed before
/// checking the stop signal — this guarantees a final burst is captured
/// before a manual `finish()` short-circuits.
pub fn start_recording_with_stop_key(
    hub: Arc<DriverHub>,
    passthrough: bool,
    stop_key: Option<rm_macro_model::KeyCode>,
) -> RecordingHandle {
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
                        // Stop-key filter FIRST — before passthrough and buffer.
                        if let Some(sk) = stop_key {
                            if let rm_driver::RawEvent::KeyDown { key } = event {
                                if key == sk {
                                    debug!(stop_key = ?sk, "recorder: stop-key matched, ending");
                                    break;
                                }
                            }
                        }
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

/// Backward-compatible wrapper around `start_recording_with_stop_key` with no
/// stop key (caller drives termination via `finish()` / `wait_for_close()`).
pub fn start_recording(hub: Arc<DriverHub>, passthrough: bool) -> RecordingHandle {
    start_recording_with_stop_key(hub, passthrough, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rm_driver::mock::MockDriver;
    use rm_driver::{Driver, DriverError, RawEvent};
    use rm_macro_model::{KeyCode, Step};
    use tokio::sync::oneshot;

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

    #[tokio::test]
    async fn stop_key_filters_event_and_ends_recording() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let h = start_recording_with_stop_key(hub, true, Some(KeyCode::F10));

        // Pre-stop events:
        drv.inject(RawEvent::KeyDown { key: KeyCode::A });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        drv.inject(RawEvent::KeyUp { key: KeyCode::A });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        // The stop key — must NOT appear in buffer AND must NOT be passthrough'd.
        drv.inject(RawEvent::KeyDown { key: KeyCode::F10 });
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let steps = h.wait_for_close().await.unwrap();

        // Expect exactly the KeyPress { A } step — no F10 trailing.
        assert_eq!(steps.len(), 1);
        match &steps[0] {
            Step::KeyPress { key: KeyCode::A, .. } => {}
            other => panic!("expected KeyPress(A), got {other:?}"),
        }

        // And the passthrough drain should NOT contain F10.
        let sent = drv.drain_sent();
        assert!(
            !sent.iter().any(|e| matches!(e, RawEvent::KeyDown { key: KeyCode::F10 })),
            "F10 leaked into passthrough: {sent:?}"
        );
    }

    #[tokio::test]
    async fn run_with_stop_external_signal_collects_buffer() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let h = start_recording_with_stop_key(hub, false, None);

        drv.inject(RawEvent::KeyDown { key: KeyCode::A });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        drv.inject(RawEvent::KeyUp { key: KeyCode::A });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;

        let (tx, rx) = oneshot::channel();
        let join = tokio::spawn(async move { h.run_with_stop(rx).await });
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        tx.send(()).unwrap();
        let steps = join.await.unwrap().unwrap();
        assert_eq!(steps.len(), 1);
        assert!(matches!(&steps[0], Step::KeyPress { key: KeyCode::A, .. }));
    }
}
