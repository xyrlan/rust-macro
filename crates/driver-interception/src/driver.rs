//! `InterceptionDriver` — implements the `rm_driver::Driver` trait by bridging
//! Interception's blocking `wait_with_timeout` to async via a dedicated OS
//! thread + `tokio::sync::mpsc` channel.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kanata_interception::{Interception, MouseFlags, MouseState, Stroke};
use rm_driver::{Driver, DriverError, RawEvent};
use rm_macro_model::{KeyCode, MouseButton};
use tokio::sync::{mpsc, Mutex as AsyncMutex};

use crate::mouse::convert_mouse;
use crate::scancode::{keycode_to_scancode, scancode_to_keycode};

/// Maximum strokes returned per `receive()` call. Interception buffers events
/// per device; reading 32 at a time keeps the OS thread responsive without
/// reallocating on every wake-up.
const RECEIVE_BATCH: usize = 32;

/// How long the OS thread blocks in `wait_with_timeout` between shutdown
/// polls. Bounds the worst-case driver-drop latency to ~100ms.
const WAIT_SLICE: Duration = Duration::from_millis(100);

/// Newtype that asserts Send + Sync on `Interception`. SAFETY: per oblitum's
/// Interception README and `interception.h`, all context-bound functions
/// (`interception_send`, `interception_wait`, `interception_receive`) are safe
/// across threads given a single context. `kanata-interception` does not declare
/// these traits because the struct contains a raw pointer. We rely on the C-side
/// guarantee.
struct InterceptionCtx(Interception);
unsafe impl Send for InterceptionCtx {}
unsafe impl Sync for InterceptionCtx {}

pub struct InterceptionDriver {
    ctx: Arc<InterceptionCtx>,
    event_rx: AsyncMutex<mpsc::UnboundedReceiver<RawEvent>>,
    shutdown: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl InterceptionDriver {
    /// Open an Interception context, install filters for all keyboard + mouse
    /// devices, spawn the OS pump thread, and return the driver. Returns an
    /// error if the context cannot be opened (driver not installed / DLL
    /// missing / etc).
    ///
    /// Use this for **recording** — the filters route every user keystroke /
    /// mouse event into the context's queue so we can capture them. The
    /// recorder MUST forward events back via `send()` (passthrough mode) to
    /// keep the OS responsive while recording is active.
    pub fn new() -> Result<Self, DriverError> {
        Self::new_impl(true)
    }

    /// Open an Interception context **without** installing capture filters,
    /// spawn the OS pump thread (which idles), and return the driver.
    ///
    /// Use this for **playback** — we only need to call `send()` to inject
    /// synthesized events; we never read from the queue. With no filters set,
    /// the kernel forwards user input directly to the OS without routing it
    /// through our context, so keyboard and mouse remain fully usable during
    /// and after playback.
    pub fn new_send_only() -> Result<Self, DriverError> {
        Self::new_impl(false)
    }

    fn new_impl(install_filters: bool) -> Result<Self, DriverError> {
        let raw = Interception::new()
            .ok_or_else(|| DriverError::Unavailable("Interception::new() returned None".into()))?;

        if install_filters {
            // Filter everything from all keyboard + mouse devices.
            // KeyFilter::all() and MouseFilter::all() capture every event kind.
            raw.set_filter(
                kanata_interception::is_keyboard,
                kanata_interception::Filter::KeyFilter(
                    kanata_interception::KeyFilter::all(),
                ),
            );
            raw.set_filter(
                kanata_interception::is_mouse,
                kanata_interception::Filter::MouseFilter(
                    kanata_interception::MouseFilter::all(),
                ),
            );
        }

        let ctx = Arc::new(InterceptionCtx(raw));
        let (tx, rx) = mpsc::unbounded_channel();
        let shutdown = Arc::new(AtomicBool::new(false));

        let thread_ctx = ctx.clone();
        let thread_shutdown = shutdown.clone();
        let thread = std::thread::Builder::new()
            .name("interception-pump".into())
            .spawn(move || pump(thread_ctx, tx, thread_shutdown))
            .map_err(|e| DriverError::Io(format!("spawn pump thread: {e}")))?;

        Ok(Self {
            ctx,
            event_rx: AsyncMutex::new(rx),
            shutdown,
            thread: Some(thread),
        })
    }
}

#[async_trait]
impl Driver for InterceptionDriver {
    async fn send(&self, event: RawEvent) -> Result<(), DriverError> {
        let (device, stroke) = match event_to_stroke(event) {
            Some(pair) => pair,
            None => {
                tracing::debug!(?event, "interception: unmapped RawEvent dropped on send");
                return Ok(());
            }
        };
        // `interception_send` is per-context thread-safe; concurrent &self
        // callers serialize at the C boundary, not in our wrapper.
        let sent = self.ctx.0.send(device, &[stroke]);
        if sent == 0 {
            return Err(DriverError::Io("interception_send wrote 0 strokes".into()));
        }
        Ok(())
    }

    async fn recv(&self) -> Result<RawEvent, DriverError> {
        let mut rx = self.event_rx.lock().await;
        rx.recv().await.ok_or(DriverError::Closed)
    }
}

impl Drop for InterceptionDriver {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // Interception context drops here, releasing the kernel handles.
    }
}

fn pump(
    ctx: Arc<InterceptionCtx>,
    event_tx: mpsc::UnboundedSender<RawEvent>,
    shutdown: Arc<AtomicBool>,
) {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }
        let device = ctx.0.wait_with_timeout(WAIT_SLICE);
        if device == 0 {
            continue; // timeout — loop back to shutdown check
        }
        let mut buf = [Stroke::Keyboard {
            code: kanata_interception::ScanCode::Esc,
            state: kanata_interception::KeyState::empty(),
            information: 0,
        }; RECEIVE_BATCH];
        let n = ctx.0.receive(device, &mut buf);
        if n <= 0 {
            continue;
        }
        for stroke in &buf[..n as usize] {
            for ev in convert_stroke(*stroke).iter() {
                if event_tx.send(ev).is_err() {
                    return; // receiver dropped; exit cleanly
                }
            }
        }
    }
}

fn convert_stroke(s: Stroke) -> crate::mouse::StrokeEvents {
    use kanata_interception::KeyState;
    match s {
        Stroke::Keyboard { code, state, .. } => {
            // Drop TermSrv flags (terminal server proxying — not modeled).
            if state.intersects(
                KeyState::TERMSRV_SET_LED | KeyState::TERMSRV_SHADOW | KeyState::TERMSRV_VKPACKET,
            ) {
                return crate::mouse::StrokeEvents::empty();
            }
            // Note: E1=3 in kanata-interception is the same bits as UP|E0,
            // so we cannot reliably detect E1 prefix strokes (Pause key) via
            // bitflags; they are passed through and will fail to map in
            // scancode_to_keycode, producing a debug log instead. Acceptable for v1.
            let is_up = state.intersects(KeyState::UP);
            let is_e0 = state.intersects(KeyState::E0);
            let mut out = crate::mouse::StrokeEvents::empty();
            match scancode_to_keycode(code as u16, is_e0) {
                Some(key) if is_up => out.events[0] = Some(RawEvent::KeyUp { key }),
                Some(key) => out.events[0] = Some(RawEvent::KeyDown { key }),
                None => {
                    tracing::debug!(scancode = code as u16, e0 = is_e0,
                        "interception: unmapped scancode dropped");
                }
            }
            out
        }
        Stroke::Mouse { state, flags, rolling, x, y, .. } => {
            convert_mouse(state.bits() as u16, flags.bits() as u16, rolling, x, y)
        }
    }
}

/// Inverse of `convert_stroke` for a single `RawEvent`. Returns the target
/// device id + the stroke to send. Returns `None` for events we can't
/// represent (and will be debug-logged + dropped by the caller).
///
/// Device ids: Interception keyboards are 1–10, mice are 11–20.
/// We send to device 1 (first keyboard) / 11 (first mouse).
fn event_to_stroke(event: RawEvent) -> Option<(kanata_interception::Device, Stroke)> {
    use kanata_interception::{KeyState, ScanCode};
    match event {
        RawEvent::KeyDown { key } | RawEvent::KeyUp { key } => {
            let (code, e0) = keycode_to_scancode(key);
            let mut state = KeyState::empty();
            if matches!(event, RawEvent::KeyUp { .. }) {
                state |= KeyState::UP;
            }
            if e0 {
                state |= KeyState::E0;
            }
            // Convert raw u16 scancode to ScanCode enum; fall back to Esc on unknown.
            let scan = ScanCode::try_from(code).unwrap_or(ScanCode::Esc);
            Some((
                1, // device 1 — first keyboard
                Stroke::Keyboard {
                    code: scan,
                    state,
                    information: 0,
                },
            ))
        }
        RawEvent::MouseDown { button } | RawEvent::MouseUp { button } => {
            let down = matches!(event, RawEvent::MouseDown { .. });
            let state = mouse_button_to_state(button, down);
            Some((
                11, // device 11 — first mouse
                Stroke::Mouse {
                    state,
                    flags: MouseFlags::empty(),
                    rolling: 0,
                    x: 0,
                    y: 0,
                    information: 0,
                },
            ))
        }
        RawEvent::MouseMove { dx, dy } => Some((
            11,
            Stroke::Mouse {
                state: MouseState::empty(),
                // MOVE_RELATIVE = 0 (the default; no flag bits set)
                flags: MouseFlags::empty(),
                rolling: 0,
                x: dx,
                y: dy,
                information: 0,
            },
        )),
        RawEvent::MouseWheel { delta } => Some((
            11,
            Stroke::Mouse {
                state: MouseState::WHEEL,
                flags: MouseFlags::empty(),
                rolling: delta as i16,
                x: 0,
                y: 0,
                information: 0,
            },
        )),
    }
}

fn mouse_button_to_state(b: MouseButton, down: bool) -> MouseState {
    match (b, down) {
        (MouseButton::Left,   true)  => MouseState::LEFT_BUTTON_DOWN,
        (MouseButton::Left,   false) => MouseState::LEFT_BUTTON_UP,
        (MouseButton::Right,  true)  => MouseState::RIGHT_BUTTON_DOWN,
        (MouseButton::Right,  false) => MouseState::RIGHT_BUTTON_UP,
        (MouseButton::Middle, true)  => MouseState::MIDDLE_BUTTON_DOWN,
        (MouseButton::Middle, false) => MouseState::MIDDLE_BUTTON_UP,
        (MouseButton::X1,     true)  => MouseState::BUTTON_4_DOWN,
        (MouseButton::X1,     false) => MouseState::BUTTON_4_UP,
        (MouseButton::X2,     true)  => MouseState::BUTTON_5_DOWN,
        (MouseButton::X2,     false) => MouseState::BUTTON_5_UP,
    }
}

// Suppress dead_code warnings: KeyCode and MouseButton are used in function
// signatures via RawEvent pattern matching above, but the compiler doesn't
// always see through the match arms.
#[allow(dead_code)]
const _: fn() = || {
    let _: KeyCode = KeyCode::A;
    let _: MouseButton = MouseButton::Left;
};
