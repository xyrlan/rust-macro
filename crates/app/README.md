# rm-app — rust-macro Tauri GUI (Plans 3a + 3b)

A desktop GUI for rust-macro: list saved macros, record new ones in-app,
edit metadata and steps, delete, and play/stop them via the existing
`rm-player` + `InterceptionDriver`.

## Prerequisites

- Windows 10/11.
- Rust toolchain (stable, MSVC).
- `tauri-cli` v2: `cargo install tauri-cli --version "^2"`.
- Node.js 20+ and npm.
- WebView2 runtime (pre-installed on Windows 11).
- Interception kernel driver installed — see
  `docs/superpowers/specs/2026-05-26-rust-macro-plan-2b-real-driver-design.md`.
  (Required for both Play and in-app Record.)

## Run in dev

```powershell
# From repo root:
cd crates/app/ui
npm install
cd ..
cargo tauri dev
```

## Build a release binary

```powershell
cd crates/app
cargo tauri build
```

Output: `target/release/rust-macro.exe` plus installer bundles under
`target/release/bundle/`.

## Manual smoke test (Plan 3a + 3b acceptance)

1. **Empty state.** Run on a machine with no macros saved. The list shows
   "No macros yet. Click '+ Record new' to capture one."
2. **Record a new macro.** Click "+ Record new" → start modal → Start →
   window minimizes → type something in another app → press F10 → window
   restores AND re-takes focus → Preview modal shows captured steps.
3. **Save the recording.** Name it "demo", pick a hotkey via the dropdown
   OR the new Capture button (press Ctrl+Shift+F5), set mode to Once →
   Save. The new row appears in the list with the correct hotkey and step
   count.
4. **Discard a recording.** Repeat step 2; in the Preview modal click
   Discard. No new macro is saved.
5. **Edit metadata + steps.** Click ✎ on a row → full-screen editor opens.
   Change the name, toggle a modifier, change mode to Repeat(3). Edit a
   step's hold_ms. Reorder with ↑/↓. Delete a step with ✕. Add a new step
   via the "+ Add step" picker. Save. Reload the app — changes persist.
6. **Live hotkey capture.** In the editor or recording Preview, click
   "🎯 Capture" → banner shows "Press your hotkey combo" → press
   Ctrl+Shift+F5 → modifiers + key appear → release → combo committed.
   Esc cancels without committing. Hold-only-modifiers does NOT commit.
   After 5 seconds idle, listening auto-cancels.
7. **Delete.** Click ✕, confirm. Restart the app — still gone.
8. **Play.** With Interception installed and running, click ▶ on a macro.
   PlaybackBanner appears. When it finishes, success toast.
9. **Stop a Loop macro.** During Loop playback, click "■ Stop" — banner
   disappears within ~100ms.
10. **PlaybackActive guard.** While playing, click ▶ on another row → short
    yellow toast: "Already playing — stop it first."
11. **RecordingActive guard.** Click "+ Record new", confirm Start; before
    pressing F10, switch back to the list view (if possible — window is
    minimized; use Alt+Tab) and try to click ▶ → red toast: "A recording
    is already in progress." Press F10 to clean up.
12. **Concurrent: try recording during playback.** Start playing a Loop
    macro. Then trigger "+ Record new" (you'll need a hotkey-bound macro
    or two side-by-side runs). Click Start in the recording modal →
    expect: red toast "A playback is already in progress" and the
    recording does NOT start.
13. **Window close mid-recording.** Start a recording. Before F10, close
    the rust-macro window → Interception releases (the OS regains
    keyboard control immediately, no stuck keys). Re-open the app → no
    half-recorded macros in the list.

## Architecture

- 3a design: `docs/superpowers/specs/2026-05-26-rust-macro-plan-3a-tauri-gui-design.md`
- 3b design: `docs/superpowers/specs/2026-05-27-rust-macro-plan-3b-recording-editor-design.md`

## Known limitations (deferred to Plan 3c+)

- Global hotkey listener (`rm-hotkey`) — triggering macros from another app's focus.
- Driver status indicator + install button.
- Settings page (configurable stop key, default storage root, theme).
- System tray icon + window state persistence.
- Toast persistence across reloads.
- Multi-macro concurrent playback.
- Drag-and-drop step reordering (3b uses ↑↓ buttons only).
- Hotkey conflict detection.
- Live hotkey capture via Interception (3b uses browser keyboard events; Win key alone, Print Screen fall back to dropdown).
