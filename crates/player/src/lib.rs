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

    /// Drive the playback to completion, observing an external stop signal.
    /// When `external_stop_rx` fires, sends the internal stop signal and
    /// awaits the player's clean exit between steps.
    pub async fn run_with_stop(
        mut self,
        external_stop_rx: oneshot::Receiver<()>,
    ) -> Result<()> {
        // We split into two phases: a select! that races external_stop_rx
        // against the inner JoinHandle, then a tail that awaits any remaining
        // work after the stop has been signaled.
        let join = self.join;
        tokio::pin!(join);
        let stop_tx = self.stop_tx.take();

        tokio::select! {
            // External stop arrived first. Fire the internal stop_tx (if not
            // already taken) and fall through to await the join.
            _ = external_stop_rx => {
                if let Some(tx) = stop_tx { let _ = tx.send(()); }
                (&mut join).await
                    .map_err(|e| AppError::Other(format!("player task panicked: {e}")))?
            }
            // Natural completion arrived first. Drop the unused stop_tx.
            result = &mut join => {
                drop(stop_tx);
                result.map_err(|e| AppError::Other(format!("player task panicked: {e}")))?
            }
        }
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

/// Minimum sleep injected between iterations of multi-iteration playback
/// modes (Repeat, Loop, Toggle). Prevents a macro with no internal Wait from
/// running at full async-runtime speed — which can saturate the OS input
/// queue and make a Loop unrecoverable without killing the process.
const MIN_ITERATION_GAP: Duration = Duration::from_millis(10);

async fn run(hub: Arc<DriverHub>, m: &Macro, mut stop_rx: oneshot::Receiver<()>) -> Result<()> {
    debug!(macro_name = %m.name, mode = ?m.playback, "player: starting");
    let mut iter = playback_iter(m.playback);
    let enforce_gap = matches!(
        m.playback,
        PlaybackMode::Repeat { .. } | PlaybackMode::Loop | PlaybackMode::Toggle
    );
    let mut first = true;
    while iter.next() {
        if enforce_gap && !first {
            tokio::time::sleep(MIN_ITERATION_GAP).await;
        }
        first = false;
        for step in &m.steps {
            if stop_rx.try_recv().is_ok() {
                debug!("player: stop signal");
                return Ok(());
            }
            run_step(&hub, step, &mut stop_rx).await?;
        }
    }
    Ok(())
}

async fn run_step(
    hub: &DriverHub,
    step: &Step,
    stop_rx: &mut oneshot::Receiver<()>,
) -> Result<()> {
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
        Step::MouseMove { to, mode, duration_ms } => {
            match (mode, duration_ms) {
                (rm_macro_model::MoveMode::Absolute, _)
                | (_, None)
                | (_, Some(0)) => {
                    hub.send(RawEvent::MouseMove { dx: to.x, dy: to.y })
                        .await
                        .map_err(|e| AppError::DriverIo(e.to_string()))?;
                }
                (rm_macro_model::MoveMode::Relative, Some(dur)) => {
                    stream_relative_move(hub, to.x, to.y, *dur, stop_rx).await?;
                }
            }
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

/// Stream a relative mouse move as a sequence of small chunks at 1ms intervals.
/// The cumulative sum of all chunks equals (total_dx, total_dy) exactly,
/// using integer-target arithmetic to avoid drift. Chunks that round to (0, 0)
/// are not sent but still consume their 1ms sleep so the total elapsed time
/// honors `duration_ms`. Aborts early if `stop_rx` fires.
async fn stream_relative_move(
    hub: &DriverHub,
    total_dx: i32,
    total_dy: i32,
    duration_ms: u32,
    stop_rx: &mut oneshot::Receiver<()>,
) -> Result<()> {
    let chunk_count = (duration_ms as i64).max(1);
    let tdx = total_dx as i64;
    let tdy = total_dy as i64;
    let mut sent_x: i64 = 0;
    let mut sent_y: i64 = 0;

    for i in 1..=chunk_count {
        if stop_rx.try_recv().is_ok() {
            return Ok(());
        }

        let target_x = i * tdx / chunk_count;
        let target_y = i * tdy / chunk_count;
        let chunk_dx = (target_x - sent_x) as i32;
        let chunk_dy = (target_y - sent_y) as i32;
        sent_x = target_x;
        sent_y = target_y;

        if chunk_dx != 0 || chunk_dy != 0 {
            hub.send(RawEvent::MouseMove { dx: chunk_dx, dy: chunk_dy })
                .await
                .map_err(|e| AppError::DriverIo(e.to_string()))?;
        }

        tokio::time::sleep(Duration::from_millis(1)).await;
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

    #[tokio::test]
    async fn run_with_stop_signals_clean_exit() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![
                Step::KeyPress { key: KeyCode::X, hold_ms: 1 },
                Step::Wait { min_ms: 5, max_ms: 5 },
            ],
            PlaybackMode::Loop,
        );
        let h = play(hub, m);
        let (stop_tx, stop_rx) = oneshot::channel();
        let join = tokio::spawn(async move { h.run_with_stop(stop_rx).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        stop_tx.send(()).unwrap();
        join.await.unwrap().unwrap();
        let count = drv.drain_sent().len();
        assert!(count > 0 && count.is_multiple_of(2));
    }

    use rm_macro_model::{MoveMode, Point};

    fn count_mouse_moves(events: &[RawEvent]) -> usize {
        events.iter().filter(|e| matches!(e, RawEvent::MouseMove { .. })).count()
    }

    fn sum_mouse_deltas(events: &[RawEvent]) -> (i32, i32) {
        events.iter().fold((0i32, 0i32), |(ax, ay), e| match e {
            RawEvent::MouseMove { dx, dy } => (ax.saturating_add(*dx), ay.saturating_add(*dy)),
            _ => (ax, ay),
        })
    }

    #[tokio::test]
    async fn streamed_move_sends_chunks_summing_to_total() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![Step::MouseMove {
                to: Point { x: 100, y: 0 },
                mode: MoveMode::Relative,
                duration_ms: Some(50),
            }],
            PlaybackMode::Once,
        );
        play(hub, m).wait().await.unwrap();
        let sent = drv.drain_sent();
        let count = count_mouse_moves(&sent);
        assert!(count >= 40 && count <= 50, "expected ~50 chunks, got {count}");
        assert_eq!(sum_mouse_deltas(&sent), (100, 0));
    }

    #[tokio::test]
    async fn streamed_move_with_sparse_motion_skips_zero_chunks() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![Step::MouseMove {
                to: Point { x: 3, y: 0 },
                mode: MoveMode::Relative,
                duration_ms: Some(100),
            }],
            PlaybackMode::Once,
        );
        play(hub, m).wait().await.unwrap();
        let sent = drv.drain_sent();
        assert_eq!(count_mouse_moves(&sent), 3, "expected 3 non-zero sends");
        assert_eq!(sum_mouse_deltas(&sent), (3, 0));
    }

    #[tokio::test]
    async fn streamed_move_honors_stop_signal_mid_stream() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![Step::MouseMove {
                to: Point { x: 500, y: 500 },
                mode: MoveMode::Relative,
                duration_ms: Some(500),
            }],
            PlaybackMode::Once,
        );
        let mut h = play(hub, m);
        tokio::time::sleep(Duration::from_millis(50)).await;
        h.stop();
        h.wait().await.unwrap();
        let sent = drv.drain_sent();
        let count = count_mouse_moves(&sent);
        assert!(count < 200, "stop should have cut the stream short, got {count} chunks");
    }

    #[tokio::test]
    async fn absolute_move_ignores_duration_ms() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![Step::MouseMove {
                to: Point { x: 42, y: 17 },
                mode: MoveMode::Absolute,
                duration_ms: Some(100),
            }],
            PlaybackMode::Once,
        );
        play(hub, m).wait().await.unwrap();
        let sent = drv.drain_sent();
        assert_eq!(count_mouse_moves(&sent), 1);
        assert_eq!(sum_mouse_deltas(&sent), (42, 17));
    }

    #[tokio::test]
    async fn none_or_zero_duration_teleports() {
        let drv = Arc::new(MockDriver::new());
        let hub = DriverHub::start(drv.clone());
        let m = macro_with_steps(
            vec![
                Step::MouseMove {
                    to: Point { x: 10, y: 5 },
                    mode: MoveMode::Relative,
                    duration_ms: None,
                },
                Step::MouseMove {
                    to: Point { x: -1, y: -1 },
                    mode: MoveMode::Relative,
                    duration_ms: Some(0),
                },
            ],
            PlaybackMode::Once,
        );
        play(hub, m).wait().await.unwrap();
        let sent = drv.drain_sent();
        assert_eq!(count_mouse_moves(&sent), 2, "both None and Some(0) should be single sends");
        assert_eq!(sum_mouse_deltas(&sent), (9, 4));
    }
}
