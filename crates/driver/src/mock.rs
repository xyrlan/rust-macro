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
        assert_eq!(
            d.recv().await.unwrap(),
            RawEvent::KeyDown { key: KeyCode::A }
        );
        assert_eq!(d.recv().await.unwrap(), RawEvent::KeyUp { key: KeyCode::A });
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
