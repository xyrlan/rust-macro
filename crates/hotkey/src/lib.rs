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
        // Keep `hub` alive for the duration of the listener so the pump task
        // isn't cancelled by the hub's Drop impl while we still hold a subscriber.
        let _hub = hub;
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
