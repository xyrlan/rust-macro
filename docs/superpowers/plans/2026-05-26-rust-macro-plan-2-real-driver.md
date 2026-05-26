# rust-macro — Plan 2: Real Interception Driver (stub)

**Goal:** Swap `MockDriver` (Plan 1) for an `InterceptionDriver` backed by the real Interception kernel driver via the `interception-rs` crate. Add driver-status detection, the bundled installer flow, and a driver multiplexer ("DriverHub") so multiple consumers (recorder, hotkey listener, player) can share one underlying device source.

**Architecture:** New crate `rm-driver-interception` providing `InterceptionDriver: Driver`. Add `driver::detect_status()` returning `{ NotInstalled, InstalledNotRunning, Running }`. `rm-cli` grows a `driver` subcommand: `status`, `install`.

**Required design work — DriverHub:** Plan 1 leaves a single-consumer assumption (only one of recorder / hotkey / player calls `driver.recv()` at a time, because the CLI is sequential). The real app (Plan 3) needs all three concurrently. Plan 2 must introduce a `DriverHub` that owns the single `Driver` impl and exposes:
  * `subscribe() -> mpsc::Receiver<RawEvent>` for fan-out reads;
  * `send(event)` for direct emission;
  * a single internal task that polls `driver.recv()` and broadcasts to all subscribers.
Existing `recorder`, `hotkey`, `player` are refactored to accept a `DriverHub` handle instead of `Arc<dyn Driver>`.

**Tech Stack:** Adds `interception-rs` (or a thin FFI binding to `interception.dll` if the crate isn't suitable on current Rust). Bundles the upstream Interception installer (`install-interception.exe`).

**Why a separate plan:** depends on having Interception installed on the dev machine + an admin reboot, so it cannot be CI-friendly. Plan 1's tests stay green throughout.

(Detailed tasks to be written when Plan 1 is merged and verified.)
