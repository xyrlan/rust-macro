# rm-app — rust-macro Tauri GUI (Plan 3a)

Plan 3a delivers the first iteration of the desktop GUI: list saved macros,
edit metadata (rename, change hotkey via dropdown, change playback mode),
delete them, and play/stop them via the existing `rm-player` +
`InterceptionDriver`. **In-app recording, the step editor, live hotkey
capture, and the driver install flow are Plan 3b.**

## Prerequisites

- Windows 10/11.
- Rust toolchain (stable, MSVC).
- `tauri-cli` v2: `cargo install tauri-cli --version "^2"`.
- Node.js 20+ and npm.
- WebView2 runtime (pre-installed on Windows 11).
- (For Play to actually drive input) Interception kernel driver installed —
  see `docs/superpowers/specs/2026-05-26-rust-macro-plan-2b-real-driver-design.md`.

## Run in dev

```powershell
# From repo root:
cd crates/app/ui
npm install
cd ..
cargo tauri dev
```

The window opens, Vite hot-reloads the UI on change, and the Rust backend
recompiles on changes via `cargo tauri dev`.

## Build a release binary

```powershell
cd crates/app
cargo tauri build
```

Output: `target/release/rust-macro.exe` plus installer bundles under
`target/release/bundle/`.

## Manual smoke test (Plan 3a acceptance)

Before merging, the implementer should walk through:

1. **Empty state.** Run on a machine with no macros saved. The list shows
   "No macros yet. Use the CLI to record one…".
2. **List render.** Use the CLI to record one or more macros
   (`cargo run -p rm-cli -- record demo`, then close stdin). Restart the GUI;
   the macro appears with the correct name, hotkey, mode, and step count.
3. **Edit metadata.** Click ✎ on a row. Change the name, toggle a modifier,
   pick a different key, change mode to Repeat(3). Save. The row updates;
   restarting the app shows the persisted change.
4. **Delete.** Click ✕, confirm. The row disappears. Restart the app — still
   gone.
5. **Play (driver missing).** This step requires the implementer to either
   not have Interception installed, or temporarily build with
   `cargo tauri dev --no-default-features` (which disables the
   `interception` feature). Without the feature, every Play click should
   produce a persistent error toast: "Interception driver not installed…".
   With the feature on but no driver, same toast. **If you have Interception
   installed and won't uninstall it for the test, skip this step and note
   it as "deferred to CI smoke once we have one".**
6. **Play (with driver).** With Interception installed and running, click ▶
   on a macro. The PlaybackBanner appears. Playback executes against the OS.
   When it finishes, a green "Playback finished" toast appears and the banner
   disappears.
7. **Stop.** During a Loop macro, click "■ Stop" in the banner. A yellow
   "Playback stopped" toast appears; the banner disappears within ~100ms.
8. **PlaybackActive guard.** While a playback is running, click ▶ on any
   row. A short yellow toast: "Already playing — stop it first.".

## Architecture

See `docs/superpowers/specs/2026-05-26-rust-macro-plan-3a-tauri-gui-design.md`.

## Known limitations (deferred to Plan 3b)

- In-app recording.
- Step-by-step macro editor.
- Live hotkey capture ("press a key combo to bind").
- `rm-hotkey` integration (global hotkey listener for triggering macros while
  another window is focused).
- Driver status indicator + install button.
- Settings page.
- Toast persistence across reloads.
- Multi-macro concurrent playback.
- Window state persistence (size/position memory, tray icon).
