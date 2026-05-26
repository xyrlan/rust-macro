use std::sync::Arc;
use std::time::{Duration, Instant};

use rm_driver::mock::MockDriver;
use rm_driver::{Driver, RawEvent};
use rm_macro_model::{KeyCode, Macro, Modifier, PlaybackMode, Trigger};
use rm_player::play;
use rm_recorder::{compile_events, TimedEvent};
use rm_storage::{load_macro, save_macro};
use tempfile::TempDir;

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

    // 4. Play through MockDriver and check the wire sequence.
    let drv = Arc::new(MockDriver::new());
    play(drv.clone() as Arc<dyn Driver>, loaded)
        .wait()
        .await
        .unwrap();
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
