pub mod compile;
pub use compile::{compile_events, TimedEvent};

use std::sync::Arc;
use std::time::Instant;

use rm_driver::Driver;
use rm_error::Result;
use rm_macro_model::Step;
use tokio::sync::{oneshot, Mutex};
use tracing::debug;

/// Handle to a running recording. Two ways to end:
///   * `finish().await` — sends an explicit stop signal, then awaits.
///   * `wait_for_close().await` — does NOT send stop; awaits until the driver
///     itself closes (e.g. stdin EOF). Use this when the caller knows the
///     event source has finite input.
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
    /// exit on its own when the driver returns `DriverError::Closed`. The
    /// `stop_tx` is held alive until this future resolves (so the recorder's
    /// `select!` won't see a phantom stop while we're waiting).
    pub async fn wait_for_close(self) -> Result<Vec<Step>> {
        let raw = self
            .join
            .await
            .map_err(|e| rm_error::AppError::Other(format!("recorder task panicked: {e}")))?;
        // self.stop_tx drops here, after the task is already complete — no effect.
        Ok(compile_events(&raw))
    }
}

/// Start a recording. Reads events from `driver.recv()` in a loop and
/// timestamps each one. When `passthrough` is true, each captured event is
/// re-emitted via `driver.send()` so the OS still sees it (this is the
/// production behavior — see the spec).
///
/// The loop exits when either: (a) `driver.recv()` returns `Closed` (or any
/// other error), or (b) the stop signal fires (from `finish()` or from the
/// handle being dropped).
///
/// The `select!` is `biased` so that pending driver events are always
/// processed before checking the stop signal — this guarantees that a final
/// burst of events is captured before a manual `finish()` short-circuits.
pub fn start_recording(driver: Arc<dyn Driver>, passthrough: bool) -> RecordingHandle {
    let (stop_tx, mut stop_rx) = oneshot::channel();
    let buf: Arc<Mutex<Vec<TimedEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let buf_task = buf.clone();
    let join = tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
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
                },
                _ = &mut stop_rx => {
                    debug!("recorder: stop signal received");
                    break;
                }
            }
        }
        // Drain the buffer.
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

    #[tokio::test]
    async fn wait_for_close_resolves_when_driver_closes() {
        // A driver that returns Closed immediately on recv. We simulate this
        // by injecting nothing and dropping the inject sender via close-of-clone.
        struct AlwaysClosed;
        #[async_trait::async_trait]
        impl rm_driver::Driver for AlwaysClosed {
            async fn send(
                &self,
                _e: rm_driver::RawEvent,
            ) -> std::result::Result<(), rm_driver::DriverError> {
                Ok(())
            }
            async fn recv(
                &self,
            ) -> std::result::Result<rm_driver::RawEvent, rm_driver::DriverError> {
                Err(rm_driver::DriverError::Closed)
            }
        }
        let drv: Arc<dyn rm_driver::Driver> = Arc::new(AlwaysClosed);
        let h = start_recording(drv, false);
        let steps = h.wait_for_close().await.unwrap();
        assert!(steps.is_empty());
    }
}
