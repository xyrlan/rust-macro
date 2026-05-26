# rust-macro — Plan 2b: Real Interception Driver (stub)

**Goal:** Replace `MockDriver` with an `InterceptionDriver` backed by the real Interception kernel driver via the `interception-rs` crate. Add driver-status detection, the bundled installer flow, and a `driver` CLI subcommand. Builds on top of the `DriverHub` shipped in Plan 2a (no architectural changes to the multiplexer).

**Architecture:** New crate `rm-driver-interception` providing `InterceptionDriver: Driver`. `crates/driver` grows a `detect_status()` returning `{ NotInstalled, InstalledNotRunning, Running }` (implementation: query Windows service manager for `keyboard` / `mouse` Interception services + attempt to open an Interception context). `rm-cli` grows a `driver` subcommand: `status`, `install`.

**Verification before adopting `interception-rs`:** confirm that `InterceptionDriver::send` (and the underlying `interception_send`) is safe under concurrent `&self` calls — this is the trait contract the `DriverHub` send path relies on.

**Tech Stack:** Adds `interception-rs` (or a thin FFI binding to `interception.dll` if the crate isn't suitable on current Rust). Bundles the upstream Interception installer (`install-interception.exe`).

**Why a separate plan:** depends on having Interception installed on the dev machine + an admin reboot, so it cannot be CI-friendly. Plan 2a tests stay green throughout; the new Interception-dependent tests are gated behind a feature flag.

(Detailed tasks to be written when Plan 2a is merged and verified.)
