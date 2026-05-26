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
