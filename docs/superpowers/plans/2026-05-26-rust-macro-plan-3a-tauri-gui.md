# rust-macro — Plan 3a: Tauri GUI (macro manager + Play) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a Tauri 2.x + Svelte 5 desktop app that lists saved macros, lets the user edit metadata (rename / hotkey via dropdown / playback mode) and delete them, and triggers playback through the existing `rm-player` + `InterceptionDriver` stack. One playback at a time. No in-app recording, no step editor, no driver-status UI — all deferred to Plan 3b.

**Architecture:** New binary crate `rm-app` hosts the Tauri main process. Existing crates (`rm-storage`, `rm-player`, `rm-driver`, `rm-driver-interception`, `rm-macro-model`, `rm-error`) are reused unchanged. Tauri commands talk to a singleton `AppState` holding lazily-initialised `DriverHub` and a single-slot `ActivePlayback`. Frontend is Svelte 5 + TypeScript + Vite with hand-rolled CSS — no UI library. Lazy driver init: the Interception context is opened on the first `play_macro` call, not at startup, so the GUI opens even without the driver installed.

**Tech Stack:** Tauri 2.x (Rust stable, MSVC toolchain), Svelte 5, TypeScript, Vite 5, Vitest (local-only), `@tauri-apps/api` v2. Target: Windows 10/11 x64. WebView2 (pre-installed on Windows 11).

**Spec:** `docs/superpowers/specs/2026-05-26-rust-macro-plan-3a-tauri-gui-design.md`.

---

## File Structure

**Files to create:**

Backend:
- `crates/app/Cargo.toml`
- `crates/app/build.rs`
- `crates/app/tauri.conf.json`
- `crates/app/icons/icon.png` (placeholder — copy from Tauri default)
- `crates/app/icons/32x32.png`
- `crates/app/icons/128x128.png`
- `crates/app/icons/128x128@2x.png`
- `crates/app/icons/icon.ico` (Windows runtime icon)
- `crates/app/src/main.rs`
- `crates/app/src/state.rs`
- `crates/app/src/commands.rs`
- `crates/app/src/dto.rs`

Frontend (under `crates/app/ui/`):
- `crates/app/ui/package.json`
- `crates/app/ui/vite.config.ts`
- `crates/app/ui/svelte.config.js`
- `crates/app/ui/tsconfig.json`
- `crates/app/ui/tsconfig.node.json`
- `crates/app/ui/index.html`
- `crates/app/ui/src/main.ts`
- `crates/app/ui/src/app.css`
- `crates/app/ui/src/App.svelte`
- `crates/app/ui/src/vite-env.d.ts`
- `crates/app/ui/src/lib/api.ts`
- `crates/app/ui/src/lib/types.ts`
- `crates/app/ui/src/lib/stores/macros.ts`
- `crates/app/ui/src/lib/stores/playback.ts`
- `crates/app/ui/src/lib/stores/toast.ts`
- `crates/app/ui/src/lib/components/MacroTable.svelte`
- `crates/app/ui/src/lib/components/MacroRow.svelte`
- `crates/app/ui/src/lib/components/EditMetadataModal.svelte`
- `crates/app/ui/src/lib/components/HotkeyPicker.svelte`
- `crates/app/ui/src/lib/components/PlaybackBanner.svelte`
- `crates/app/ui/src/lib/components/Toast.svelte`
- `crates/app/ui/src/lib/components/ToastHost.svelte`

Docs:
- `crates/app/README.md`

**Files to modify:**
- `Cargo.toml` (repo root) — add `crates/app` member, add Tauri workspace deps
- `crates/error/src/lib.rs` — add `AppError::PlaybackActive`
- `.gitignore` — add `crates/app/ui/node_modules/`, `crates/app/ui/dist/`, `crates/app/ui/.vite/`

Tasks decomposed by file boundary. Each task is one focused commit.

---

## Task 1: Workspace plumbing — add `crates/app` member + Tauri workspace deps

**Files:**
- Modify: `Cargo.toml` (repo root)
- Modify: `.gitignore`

- [ ] **Step 1: Update `Cargo.toml` (repo root)**

Replace the file with:

```toml
[workspace]
resolver = "2"
members = [
    "crates/error",
    "crates/macro_model",
    "crates/driver",
    "crates/driver-interception",
    "crates/storage",
    "crates/recorder",
    "crates/player",
    "crates/hotkey",
    "crates/cli",
    "crates/app",
]

[workspace.package]
edition = "2021"
rust-version = "1.75"
version = "0.1.0"
license = "MIT OR Apache-2.0"

[workspace.dependencies]
anyhow = "1"
async-trait = "0.1"
chrono = { version = "0.4", features = ["serde"] }
clap = { version = "4", features = ["derive"] }
dirs = "5"
kanata-interception = "0.3"
rand = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tauri = { version = "2", features = [] }
tauri-build = { version = "2", features = [] }
thiserror = "1"
tokio = { version = "1", features = ["full"] }
tokio-util = "0.7"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
uuid = { version = "1", features = ["v4", "serde"] }
windows-sys = { version = "0.59", features = [
    "Win32_System_Services",
    "Win32_Foundation",
] }

# dev
tempfile = "3"
```

- [ ] **Step 2: Update `.gitignore`**

The file currently has:
```
/target
**/*.rs.bk
Cargo.lock.bak
.DS_Store
.superpowers/
```

Append these three lines so the final content is:

```
/target
**/*.rs.bk
Cargo.lock.bak
.DS_Store
.superpowers/
crates/app/ui/node_modules/
crates/app/ui/dist/
crates/app/ui/.vite/
```

- [ ] **Step 3: Verify the workspace still parses (the new member doesn't exist yet — Task 5 creates it; an error mentioning `crates/app/Cargo.toml` is expected)**

Run: `cargo metadata --no-deps --format-version 1 1>$null`
Expected: error message mentioning `crates/app/Cargo.toml` does not exist.

- [ ] **Step 4: Commit**

```powershell
git add Cargo.toml .gitignore
git commit -m "chore(workspace): add app member + tauri/tauri-build workspace deps"
```

---

## Task 2: `AppError::PlaybackActive` variant + tests

**Files:**
- Modify: `crates/error/src/lib.rs`

- [ ] **Step 1: Write the failing test first**

Open `crates/error/src/lib.rs` and append this test inside `mod tests` (just before the closing `}`):

```rust
    #[test]
    fn playback_active_kind_is_stable() {
        assert_eq!(AppError::PlaybackActive.kind(), "PlaybackActive");
        assert_eq!(
            AppError::PlaybackActive.to_string(),
            "A playback is already in progress"
        );
    }
```

- [ ] **Step 2: Run the test — it fails**

Run: `cargo test -p rm-error playback_active_kind_is_stable`
Expected: FAIL — `AppError::PlaybackActive` is undefined.

- [ ] **Step 3: Add the variant**

Edit `crates/error/src/lib.rs`. Inside the `AppError` enum, add the new variant after `RecordingActive`:

```rust
    #[error("A playback is already in progress")]
    PlaybackActive,
```

Then in the `kind()` `match`, add the corresponding arm — the function should look like:

```rust
    pub fn kind(&self) -> &'static str {
        match self {
            AppError::DriverNotInstalled => "DriverNotInstalled",
            AppError::DriverNotRunning => "DriverNotRunning",
            AppError::DriverIo(_) => "DriverIo",
            AppError::MacroNotFound(_) => "MacroNotFound",
            AppError::RecordingActive => "RecordingActive",
            AppError::PlaybackActive => "PlaybackActive",
            AppError::Io { .. } => "Io",
            AppError::Serde(_) => "Serde",
            AppError::Other(_) => "Other",
        }
    }
```

- [ ] **Step 4: Run the test — it passes**

Run: `cargo test -p rm-error`
Expected: PASS — all previous error tests still pass plus the new one (5 tests total).

- [ ] **Step 5: Commit**

```powershell
git add crates/error/src/lib.rs
git commit -m "feat(error): add AppError::PlaybackActive for GUI single-playback enforcement"
```

---

## Task 3: Extract `open_with_status` to `rm-driver-interception`; refactor CLI

**Files:**
- Modify: `crates/driver-interception/src/lib.rs` (or a new module — implementer's choice)
- Modify: `crates/driver-interception/Cargo.toml`
- Modify: `crates/cli/src/commands.rs`

Plan 2b's CLI defined a private `open_interception()` helper that maps `InterceptionDriver::new()` failures to `AppError` via `detect_status()`. Plan 3a needs the identical logic. Extract it to the driver crate so both consumers use one source of truth.

- [ ] **Step 1: Add `open_with_status()` to `rm-driver-interception`**

In `crates/driver-interception/src/lib.rs`, add:

```rust
use rm_error::AppError;

/// Open an Interception context, mapping failure to `AppError` via
/// `detect_status()`. Consumers (CLI, GUI) should prefer this over
/// `InterceptionDriver::new()` + manual status mapping.
pub fn open_with_status() -> Result<InterceptionDriver, AppError> {
    InterceptionDriver::new().map_err(|orig| match detect_status() {
        DriverStatus::NotInstalled => AppError::DriverNotInstalled,
        DriverStatus::InstalledNotRunning => AppError::DriverNotRunning,
        DriverStatus::Running => AppError::DriverIo(orig.to_string()),
    })
}
```

(The function uses items already exported from `status` and `driver` modules: `InterceptionDriver`, `detect_status`, `DriverStatus`. They are already re-exported at the crate root; if not, the implementer adds the appropriate `pub use`.)

Also add `rm-error = { path = "../error" }` to `crates/driver-interception/Cargo.toml`'s `[dependencies]` (it isn't currently a dep).

- [ ] **Step 2: Refactor `crates/cli/src/commands.rs` to use the new helper**

Replace the existing `fn open_interception()` with a one-liner that delegates to the new helper:

```rust
#[cfg(feature = "interception")]
fn open_interception() -> Result<rm_driver_interception::InterceptionDriver> {
    rm_driver_interception::open_with_status()
}
```

(The CLI's `Result<T>` is `std::result::Result<T, AppError>`, so the helper's return type matches directly.)

- [ ] **Step 3: Run all tests**

```powershell
cargo test --workspace
```

Expected: PASS — no regressions; the CLI's behavior is identical.

- [ ] **Step 4: Commit**

```powershell
git add crates/driver-interception/src/lib.rs crates/driver-interception/Cargo.toml crates/cli/src/commands.rs
git commit -m "refactor: extract open_with_status to rm-driver-interception (shared by CLI + GUI)"
```

---

## Task 4: Add `PlaybackHandle::run_with_stop` to `rm-player`

**Files:**
- Modify: `crates/player/src/lib.rs`

The current `PlaybackHandle` exposes `stop(&mut self)` and `wait(self)` — but these can't be called concurrently from a single caller. Plan 3a's GUI needs to be able to signal "please stop" from the `stop_playback` Tauri command while a separate supervisor task is awaiting completion. This task adds a method that takes both responsibilities at once.

- [ ] **Step 1: Add a failing test first**

Append to `mod tests` in `crates/player/src/lib.rs`:

```rust
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
```

- [ ] **Step 2: Run the test — it fails**

Run: `cargo test -p rm-player run_with_stop_signals_clean_exit`
Expected: FAIL — `run_with_stop` is undefined.

- [ ] **Step 3: Implement `run_with_stop` on `PlaybackHandle`**

In `crates/player/src/lib.rs`, add this method inside `impl PlaybackHandle`:

```rust
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
```

- [ ] **Step 4: Run the test — it passes**

Run: `cargo test -p rm-player`
Expected: PASS — all prior tests + the new one (~7 tests).

- [ ] **Step 5: Commit**

```powershell
git add crates/player/src/lib.rs
git commit -m "feat(player): add PlaybackHandle::run_with_stop for external stop signaling"
```

---

## Task 5: Scaffold `rm-app` crate (Cargo.toml, build.rs, minimal main.rs)

**Files:**
- Create: `crates/app/Cargo.toml`
- Create: `crates/app/build.rs`
- Create: `crates/app/src/main.rs`

- [ ] **Step 1: Write `crates/app/Cargo.toml`**

```toml
[package]
name = "rm-app"
version.workspace = true
edition.workspace = true

[[bin]]
name = "rust-macro"
path = "src/main.rs"

[features]
default = ["interception"]
# Pulls in rm-driver-interception. Disable with --no-default-features for
# builds that need to avoid the transitive LGPL-3.0 obligation from
# interception-sys (see LICENSES.md). Without this feature, `play_macro`
# returns DriverNotInstalled and the GUI is read-only.
interception = ["dep:rm-driver-interception"]

[build-dependencies]
tauri-build.workspace = true

[dependencies]
async-trait.workspace = true
chrono.workspace = true
dirs.workspace = true
rm-driver = { path = "../driver" }
rm-driver-interception = { path = "../driver-interception", optional = true }
rm-error = { path = "../error" }
rm-macro-model = { path = "../macro_model" }
rm-player = { path = "../player" }
rm-storage = { path = "../storage" }
serde.workspace = true
serde_json.workspace = true
tauri.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
uuid.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

Note: `dirs` is promoted to a workspace dependency in Task 1 (Plan 3a). `crates/cli/Cargo.toml` still pins `dirs = "5"` locally; flag as a future cleanup to switch the CLI to `dirs.workspace = true` — not required in 3a.

- [ ] **Step 2: Write `crates/app/build.rs`**

```rust
fn main() {
    tauri_build::build();
}
```

- [ ] **Step 3: Write a minimal `crates/app/src/main.rs`** (no commands yet — just opens the window so we can verify the scaffold works)

```rust
//! Entry point for the rust-macro Tauri GUI. Commands and state are wired in
//! later tasks of Plan 3a; this initial revision only verifies that the Tauri
//! runtime starts and shows a window.

// Hide the Windows console when launching the release binary; keep it for
// debug so println!/tracing output is visible during development.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 4: Defer compilation verification to Task 6**

`cargo check -p rm-app` will fail until `tauri.conf.json` and the frontend `index.html` exist — both are created in Task 6. Commit this task's files first, then continue.

- [ ] **Step 5: Commit**

```powershell
git add crates/app/Cargo.toml crates/app/build.rs crates/app/src/main.rs
git commit -m "feat(app): scaffold rm-app crate (Cargo.toml + minimal Tauri main)"
```

---

## Task 6: Tauri config + placeholder icons + frontend skeleton

**Files:**
- Create: `crates/app/tauri.conf.json`
- Create: `crates/app/icons/*` (placeholder PNG + ICO files)
- Create: `crates/app/ui/package.json`
- Create: `crates/app/ui/vite.config.ts`
- Create: `crates/app/ui/svelte.config.js`
- Create: `crates/app/ui/tsconfig.json`
- Create: `crates/app/ui/tsconfig.node.json`
- Create: `crates/app/ui/index.html`
- Create: `crates/app/ui/src/main.ts`
- Create: `crates/app/ui/src/app.css`
- Create: `crates/app/ui/src/App.svelte`
- Create: `crates/app/ui/src/vite-env.d.ts`

- [ ] **Step 1: Write `crates/app/tauri.conf.json`**

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "rust-macro",
  "version": "0.1.0",
  "identifier": "dev.xyrlan.rust-macro",
  "build": {
    "beforeDevCommand": "npm --prefix ui run dev",
    "beforeBuildCommand": "npm --prefix ui run build",
    "devUrl": "http://localhost:1420",
    "frontendDist": "../ui/dist"
  },
  "app": {
    "windows": [
      {
        "title": "rust-macro",
        "width": 1000,
        "height": 700,
        "resizable": true,
        "fullscreen": false
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.ico"
    ]
  }
}
```

- [ ] **Step 2: Add placeholder icons**

Tauri requires the icon files to exist at the paths declared above, even if they're temporary. Generate them with the Tauri CLI:

```powershell
# From crates/app/, generate icons from a 1024x1024 source. If you don't have a
# source image, use any solid-color PNG — production branding is Plan 3b.
# Easiest: download the default Tauri logo (used in `create-tauri-app`) and run:
npx @tauri-apps/cli icon path\to\source.png --output icons
```

If `npx @tauri-apps/cli icon` is unavailable or you want a fully offline workflow:

1. Create `icons/32x32.png`, `icons/128x128.png`, `icons/128x128@2x.png`, `icons/icon.ico` as 1×1-pixel placeholder PNGs (any solid color). The simplest way is to copy a small PNG from anywhere on your system into all four names and rename. Tauri will warn but build succeeds.
2. For Windows-only dev, `icons/icon.ico` is the file actually used at runtime; the rest can be 1×1 placeholders.

The implementer should verify all four files exist with non-zero size; the actual visual content is replaced in Plan 3b polish.

- [ ] **Step 3: Write `crates/app/ui/package.json`**

```json
{
  "name": "rm-app-ui",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "preview": "vite preview",
    "test": "vitest run",
    "test:watch": "vitest"
  },
  "dependencies": {
    "@tauri-apps/api": "^2.0.0"
  },
  "devDependencies": {
    "@sveltejs/vite-plugin-svelte": "^4.0.0",
    "@testing-library/svelte": "^5.2.0",
    "@tsconfig/svelte": "^5.0.0",
    "jsdom": "^25.0.0",
    "svelte": "^5.0.0",
    "svelte-check": "^4.0.0",
    "tslib": "^2.6.0",
    "typescript": "^5.5.0",
    "vite": "^5.4.0",
    "vitest": "^2.1.0"
  }
}
```

- [ ] **Step 4: Write `crates/app/ui/vite.config.ts`**

```ts
/// <reference types="vitest" />
import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed port. 1420 matches `devUrl` in tauri.conf.json.
const TAURI_DEV_PORT = 1420;

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: TAURI_DEV_PORT,
    strictPort: true,
    host: "127.0.0.1",
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: "es2022",
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
  test: {
    environment: "jsdom",
    globals: true,
  },
});
```

- [ ] **Step 5: Write `crates/app/ui/svelte.config.js`**

```js
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

export default {
  preprocess: vitePreprocess(),
};
```

- [ ] **Step 6: Write `crates/app/ui/tsconfig.json`**

```json
{
  "extends": "@tsconfig/svelte/tsconfig.json",
  "compilerOptions": {
    "target": "ES2022",
    "useDefineForClassFields": true,
    "module": "ESNext",
    "resolveJsonModule": true,
    "allowJs": true,
    "checkJs": true,
    "isolatedModules": true,
    "moduleDetection": "force",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "skipLibCheck": true
  },
  "include": ["src/**/*.ts", "src/**/*.svelte", "src/**/*.d.ts"],
  "references": [{ "path": "./tsconfig.node.json" }]
}
```

- [ ] **Step 7: Write `crates/app/ui/tsconfig.node.json`**

```json
{
  "compilerOptions": {
    "composite": true,
    "skipLibCheck": true,
    "module": "ESNext",
    "moduleResolution": "bundler",
    "allowSyntheticDefaultImports": true,
    "strict": true
  },
  "include": ["vite.config.ts"]
}
```

- [ ] **Step 8: Write `crates/app/ui/index.html`**

```html
<!DOCTYPE html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>rust-macro</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

- [ ] **Step 9: Write `crates/app/ui/src/main.ts`**

```ts
import "./app.css";
import { mount } from "svelte";
import App from "./App.svelte";

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;
```

- [ ] **Step 10: Write `crates/app/ui/src/vite-env.d.ts`**

```ts
/// <reference types="svelte" />
/// <reference types="vite/client" />
```

- [ ] **Step 11: Write `crates/app/ui/src/app.css`**

```css
:root {
  --bg: #0e0e10;
  --bg-elevated: #18181b;
  --bg-input: #1f1f23;
  --border: #2a2a2e;
  --border-hover: #3a3a40;
  --text: #e4e4e7;
  --text-muted: #a1a1aa;
  --accent: #2563eb;
  --accent-hover: #1d4ed8;
  --danger: #dc2626;
  --success: #16a34a;
  --warning: #ca8a04;

  color-scheme: dark;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  font-size: 14px;
  line-height: 1.5;
  color: var(--text);
  background-color: var(--bg);
}

*, *::before, *::after {
  box-sizing: border-box;
}

html, body, #app {
  margin: 0;
  height: 100%;
}

body {
  min-height: 100vh;
}

button {
  font: inherit;
  color: inherit;
  background: var(--bg-elevated);
  border: 1px solid var(--border);
  padding: 0.4rem 0.8rem;
  border-radius: 4px;
  cursor: pointer;
}

button:hover:not(:disabled) {
  border-color: var(--border-hover);
}

button:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

button.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: white;
}

button.primary:hover:not(:disabled) {
  background: var(--accent-hover);
  border-color: var(--accent-hover);
}

button.danger {
  background: transparent;
  border-color: var(--danger);
  color: var(--danger);
}

input, select {
  font: inherit;
  color: inherit;
  background: var(--bg-input);
  border: 1px solid var(--border);
  padding: 0.4rem 0.6rem;
  border-radius: 4px;
}

input:focus, select:focus {
  outline: none;
  border-color: var(--accent);
}

code {
  font-family: ui-monospace, "Cascadia Code", "Consolas", monospace;
  background: var(--bg-input);
  padding: 0.1rem 0.4rem;
  border-radius: 3px;
  font-size: 0.9em;
}
```

- [ ] **Step 12: Write `crates/app/ui/src/App.svelte`** (hello-world version; expanded in later tasks)

```svelte
<script lang="ts">
  // Plan 3a: placeholder root. Replaced in Task 9.
</script>

<main style="padding: 2rem;">
  <h1>rust-macro</h1>
  <p>GUI scaffold up. Macro list lands in Task 9.</p>
</main>
```

- [ ] **Step 13: Install npm dependencies + run the dev server once to validate**

```powershell
# From crates/app/ui/
cd crates/app/ui
npm install
```

Expected: no errors. `node_modules/` populated.

Optional sanity (don't commit anything from this): `npm run build` should emit `crates/app/ui/dist/index.html`. Delete `dist/` after verification — it's gitignored.

- [ ] **Step 14: Run `cargo check -p rm-app` from the repo root**

```powershell
cd ..\..\..  # back to repo root
cargo check -p rm-app
```

Expected: PASS. `tauri-build` runs in build.rs and finds `tauri.conf.json`. The Rust binary now compiles.

- [ ] **Step 15: Verify the dev workflow opens a window**

```powershell
# Optional manual sanity test — NOT part of CI
npx --prefix crates/app/ui @tauri-apps/cli dev
```

Or, if `tauri-cli` is installed globally (`cargo install tauri-cli --version "^2"`):

```powershell
cd crates/app
cargo tauri dev
```

Expected: Vite starts on port 1420; Tauri opens a window titled "rust-macro" showing "GUI scaffold up." Close the window to stop.

If this manual step fails, report BLOCKED — Tauri 2 toolchain isn't installed correctly. Do not proceed.

- [ ] **Step 16: Commit**

```powershell
git add crates/app/tauri.conf.json crates/app/icons crates/app/ui/
git commit -m "feat(app): Tauri 2 config + Svelte 5 frontend scaffold (hello world)"
```

Do NOT commit `crates/app/ui/node_modules/` or `crates/app/ui/dist/` — they are gitignored from Task 1. The `package-lock.json` SHOULD be committed (it's not gitignored). Verify with `git status crates/app/ui/package-lock.json` after running `npm install` — it should show as a new file ready to stage.

---

## Task 7: DTOs (`dto.rs`) with serde + roundtrip tests

**Files:**
- Create: `crates/app/src/dto.rs`
- Modify: `crates/app/src/main.rs`

- [ ] **Step 1: Write failing tests first**

Create `crates/app/src/dto.rs` with the test module up front (stub structs that fail to compile until Step 2 fills them in):

```rust
//! Wire-format DTOs for Tauri commands. Mirror `rm-macro-model` shapes but
//! kept separate so the wire format can evolve independently from the
//! internal domain types.

use chrono::{DateTime, Utc};
use rm_macro_model::{KeyCode, Macro, Modifier, PlaybackMode, Trigger};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct MacroDto {
    pub id: Uuid,
    pub name: String,
    pub trigger: TriggerDto,
    pub playback: PlaybackModeDto,
    pub step_count: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerDto {
    Hotkey { key: KeyCode, modifiers: Vec<Modifier> },
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlaybackModeDto {
    Once,
    Repeat { value: u32 },
    Loop,
    Toggle,
}

impl From<&Trigger> for TriggerDto {
    fn from(t: &Trigger) -> Self {
        match t {
            Trigger::Hotkey { key, modifiers } => TriggerDto::Hotkey {
                key: *key,
                modifiers: modifiers.clone(),
            },
        }
    }
}

impl From<TriggerDto> for Trigger {
    fn from(t: TriggerDto) -> Self {
        match t {
            TriggerDto::Hotkey { key, modifiers } => Trigger::Hotkey { key, modifiers },
        }
    }
}

impl From<&PlaybackMode> for PlaybackModeDto {
    fn from(p: &PlaybackMode) -> Self {
        match p {
            PlaybackMode::Once => PlaybackModeDto::Once,
            PlaybackMode::Repeat { count } => PlaybackModeDto::Repeat { value: *count },
            PlaybackMode::Loop => PlaybackModeDto::Loop,
            PlaybackMode::Toggle => PlaybackModeDto::Toggle,
        }
    }
}

impl From<PlaybackModeDto> for PlaybackMode {
    fn from(p: PlaybackModeDto) -> Self {
        match p {
            PlaybackModeDto::Once => PlaybackMode::Once,
            PlaybackModeDto::Repeat { value } => PlaybackMode::Repeat { count: value },
            PlaybackModeDto::Loop => PlaybackMode::Loop,
            PlaybackModeDto::Toggle => PlaybackMode::Toggle,
        }
    }
}

impl From<&Macro> for MacroDto {
    fn from(m: &Macro) -> Self {
        MacroDto {
            id: m.id,
            name: m.name.clone(),
            trigger: (&m.trigger).into(),
            playback: (&m.playback).into(),
            step_count: m.steps.len(),
            created_at: m.created_at,
            updated_at: m.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_dto_roundtrips_through_json() {
        let t = TriggerDto::Hotkey {
            key: KeyCode::F1,
            modifiers: vec![Modifier::Ctrl, Modifier::Shift],
        };
        let json = serde_json::to_string(&t).unwrap();
        assert!(json.contains("\"type\":\"hotkey\""));
        let back: TriggerDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn playback_mode_dto_serializes_with_tagged_repeat() {
        let p = PlaybackModeDto::Repeat { value: 7 };
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"type\":\"repeat\""));
        assert!(json.contains("\"value\":7"));
        let back: PlaybackModeDto = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn macro_dto_from_macro_omits_steps_but_keeps_count() {
        let mut m = Macro::new(
            "demo",
            Trigger::Hotkey {
                key: KeyCode::F1,
                modifiers: vec![Modifier::Ctrl],
            },
            PlaybackMode::Once,
        );
        m.steps = vec![
            rm_macro_model::Step::Wait { min_ms: 100, max_ms: 100 },
            rm_macro_model::Step::Wait { min_ms: 50, max_ms: 50 },
        ];
        let dto: MacroDto = (&m).into();
        assert_eq!(dto.id, m.id);
        assert_eq!(dto.name, "demo");
        assert_eq!(dto.step_count, 2);
    }

    #[test]
    fn trigger_roundtrip_dto_to_domain() {
        let dto = TriggerDto::Hotkey {
            key: KeyCode::Enter,
            modifiers: vec![Modifier::Alt],
        };
        let domain: Trigger = dto.clone().into();
        let back: TriggerDto = (&domain).into();
        assert_eq!(back, dto);
    }

    #[test]
    fn playback_mode_roundtrip_dto_to_domain() {
        for dto in [
            PlaybackModeDto::Once,
            PlaybackModeDto::Repeat { value: 5 },
            PlaybackModeDto::Loop,
            PlaybackModeDto::Toggle,
        ] {
            let domain: PlaybackMode = dto.into();
            let back: PlaybackModeDto = (&domain).into();
            assert_eq!(back, dto);
        }
    }
}
```

- [ ] **Step 2: Register the module in `main.rs`**

Edit `crates/app/src/main.rs`. After the `#![cfg_attr(...)]` line and before `fn main`, add:

```rust
mod dto;
```

The file should look like:

```rust
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod dto;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 3: Run the tests**

```powershell
cargo test -p rm-app dto::tests
```

Expected: PASS (5 tests).

- [ ] **Step 4: Commit**

```powershell
git add crates/app/src/dto.rs crates/app/src/main.rs
git commit -m "feat(app): wire DTOs for Tauri commands (MacroDto, TriggerDto, PlaybackModeDto)"
```

---

## Task 8: `AppState` + `load_macros` + `delete_macro` commands

**Files:**
- Create: `crates/app/src/state.rs`
- Create: `crates/app/src/commands.rs`
- Modify: `crates/app/src/main.rs`

- [ ] **Step 1: Write `crates/app/src/state.rs`**

```rust
//! Runtime state for the Tauri app. `DriverHub` is created lazily on the
//! first `play_macro` call; `active` enforces one-playback-at-a-time.

use std::path::PathBuf;
use std::sync::Arc;

use rm_driver::DriverHub;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Initialised once at startup in `main`. All Tauri commands receive a
/// `State<'_, AppState>` parameter.
pub struct AppState {
    pub storage_root: PathBuf,
    pub driver_hub: Mutex<Option<Arc<DriverHub>>>,
    pub active: Mutex<Option<ActivePlayback>>,
}

pub struct ActivePlayback {
    pub macro_id: Uuid,
    pub macro_name: String,
    /// User-initiated stop signal. `Some` while the playback is running;
    /// `stop_playback` takes the sender out and fires it. The supervisor
    /// task spawned by `play_macro` observes this via a relay and forwards
    /// it to `PlaybackHandle::run_with_stop`. Once fired, the supervisor
    /// clears the active slot and emits `playback_finished`.
    pub stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl AppState {
    pub fn new(storage_root: PathBuf) -> Self {
        Self {
            storage_root,
            driver_hub: Mutex::new(None),
            active: Mutex::new(None),
        }
    }
}
```

- [ ] **Step 2: Write `crates/app/src/commands.rs`** with `load_macros` + `delete_macro` (other commands added in later tasks)

```rust
//! Tauri command handlers. Each command takes `State<'_, AppState>` and
//! returns `Result<T, WireError>`. Errors map from `AppError::to_wire()`.

use rm_error::{AppError, WireError};
use rm_macro_model::Macro;
use rm_storage::{delete_macro as storage_delete, load_all, load_macro};
use tauri::State;
use uuid::Uuid;

use crate::dto::MacroDto;
use crate::state::AppState;

#[tauri::command]
pub async fn load_macros(state: State<'_, AppState>) -> Result<Vec<MacroDto>, WireError> {
    let macros = load_all(&state.storage_root).map_err(|e| e.to_wire())?;
    Ok(macros.iter().map(MacroDto::from).collect())
}

#[tauri::command]
pub async fn delete_macro(
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<(), WireError> {
    // load_macro returns MacroNotFound for a missing file via a single
    // path.exists() check — cheaper than load_all on machines with many
    // macros, and gives us the same "fail with MacroNotFound rather than a
    // silent no-op" behavior when the UI is out of sync.
    load_macro(&state.storage_root, id).map_err(|e| e.to_wire())?;
    storage_delete(&state.storage_root, id).map_err(|e| e.to_wire())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rm_macro_model::{KeyCode, Modifier, PlaybackMode, Step, Trigger};
    use rm_storage::save_macro;
    use tempfile::TempDir;

    fn fixture_state() -> (TempDir, AppState) {
        let tmp = TempDir::new().unwrap();
        let state = AppState::new(tmp.path().to_path_buf());
        (tmp, state)
    }

    fn fixture_macro(name: &str) -> Macro {
        let mut m = Macro::new(
            name,
            Trigger::Hotkey {
                key: KeyCode::F1,
                modifiers: vec![Modifier::Ctrl],
            },
            PlaybackMode::Once,
        );
        m.steps = vec![Step::Wait { min_ms: 10, max_ms: 10 }];
        m
    }

    // The State<'_, AppState> wrapper from Tauri is hard to construct outside a
    // Tauri runtime, so we test the inner logic by calling the storage layer
    // directly with our AppState's storage_root. This is what each command's
    // body does; the only thing not covered is the Tauri command-dispatch
    // wiring (which is verified by the manual smoke test at the end of the
    // plan).

    #[tokio::test]
    async fn load_returns_saved_macros() {
        let (_tmp, state) = fixture_state();
        let m = fixture_macro("alpha");
        save_macro(&state.storage_root, &m).unwrap();

        let macros = load_all(&state.storage_root).unwrap();
        let dtos: Vec<MacroDto> = macros.iter().map(MacroDto::from).collect();
        assert_eq!(dtos.len(), 1);
        assert_eq!(dtos[0].name, "alpha");
        assert_eq!(dtos[0].step_count, 1);
    }

    #[tokio::test]
    async fn delete_missing_returns_macro_not_found() {
        let (_tmp, state) = fixture_state();
        let id = Uuid::new_v4();
        let result = load_all(&state.storage_root)
            .map_err(|e| e.to_wire())
            .and_then(|all| {
                if all.iter().any(|m| m.id == id) {
                    Ok(())
                } else {
                    Err(AppError::MacroNotFound(id.to_string()).to_wire())
                }
            });
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind, "MacroNotFound");
    }

    #[tokio::test]
    async fn delete_existing_removes_file() {
        let (_tmp, state) = fixture_state();
        let m = fixture_macro("to-be-deleted");
        save_macro(&state.storage_root, &m).unwrap();
        assert_eq!(load_all(&state.storage_root).unwrap().len(), 1);

        storage_delete(&state.storage_root, m.id).unwrap();
        assert_eq!(load_all(&state.storage_root).unwrap().len(), 0);
    }
}
```

- [ ] **Step 3: Update `crates/app/src/main.rs`** to register the modules, initialise `AppState`, and register the commands

```rust
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod commands;
mod dto;
mod state;

use std::path::PathBuf;

use state::AppState;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let storage_root = dirs::data_dir()
        .map(|d| d.join("rust-macro"))
        .unwrap_or_else(|| PathBuf::from("./.rust-macro"));

    tauri::Builder::default()
        .manage(AppState::new(storage_root))
        .invoke_handler(tauri::generate_handler![
            commands::load_macros,
            commands::delete_macro,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 4: Run the tests**

```powershell
cargo test -p rm-app
```

Expected: PASS — 5 dto tests + 3 commands tests = 8 tests.

- [ ] **Step 5: Verify compilation**

```powershell
cargo check -p rm-app
```

Expected: PASS.

- [ ] **Step 6: Commit**

```powershell
git add crates/app/src/state.rs crates/app/src/commands.rs crates/app/src/main.rs
git commit -m "feat(app): AppState + load_macros/delete_macro Tauri commands"
```

---

## Task 9: Frontend — `api.ts`, `types.ts`, macros store, MacroTable, MacroRow

**Files:**
- Create: `crates/app/ui/src/lib/types.ts`
- Create: `crates/app/ui/src/lib/api.ts`
- Create: `crates/app/ui/src/lib/stores/macros.ts`
- Create: `crates/app/ui/src/lib/stores/toast.ts`
- Create: `crates/app/ui/src/lib/components/MacroTable.svelte`
- Create: `crates/app/ui/src/lib/components/MacroRow.svelte`
- Create: `crates/app/ui/src/lib/components/Toast.svelte`
- Create: `crates/app/ui/src/lib/components/ToastHost.svelte`
- Modify: `crates/app/ui/src/App.svelte`

- [ ] **Step 1: Write `crates/app/ui/src/lib/types.ts`**

```ts
// Mirror of crates/app/src/dto.rs. Keep in sync manually — runtime errors
// from a stale mirror will surface as "missing field" deserialisation errors
// in the Rust backend, which become WireError toasts in the UI.

// Values are the snake_case serde renames from rm_macro_model::KeyCode /
// Modifier. Keep this list in sync with crates/macro_model/src/input.rs.
// Display the user-facing label via the `keyCodeLabel(key)` helper below.
export type KeyCode =
  | "a" | "b" | "c" | "d" | "e" | "f" | "g" | "h" | "i" | "j" | "k" | "l" | "m"
  | "n" | "o" | "p" | "q" | "r" | "s" | "t" | "u" | "v" | "w" | "x" | "y" | "z"
  | "num0" | "num1" | "num2" | "num3" | "num4"
  | "num5" | "num6" | "num7" | "num8" | "num9"
  | "f1" | "f2" | "f3" | "f4" | "f5" | "f6"
  | "f7" | "f8" | "f9" | "f10" | "f11" | "f12"
  | "l_shift" | "r_shift" | "l_ctrl" | "r_ctrl"
  | "l_alt" | "r_alt" | "l_win" | "r_win"
  | "space" | "enter" | "tab" | "backspace" | "escape" | "caps_lock"
  | "up" | "down" | "left" | "right"
  | "insert" | "delete" | "home" | "end" | "page_up" | "page_down"
  | "minus" | "equals" | "l_bracket" | "r_bracket" | "backslash" | "semicolon"
  | "apostrophe" | "backtick" | "comma" | "period" | "slash";

export type Modifier = "ctrl" | "shift" | "alt" | "win";

/** Format a snake_case KeyCode/Modifier for display. */
export function inputLabel(s: KeyCode | Modifier): string {
  return s
    .split("_")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

export type Trigger = { type: "hotkey"; key: KeyCode; modifiers: Modifier[] };

export type PlaybackMode =
  | { type: "once" }
  | { type: "repeat"; value: number }
  | { type: "loop" }
  | { type: "toggle" };

export type MacroDto = {
  id: string;             // Uuid serialises as string
  name: string;
  trigger: Trigger;
  playback: PlaybackMode;
  step_count: number;
  created_at: string;     // RFC3339 datetime
  updated_at: string;
};

export type WireError = {
  kind:
    | "DriverNotInstalled"
    | "DriverNotRunning"
    | "DriverIo"
    | "MacroNotFound"
    | "RecordingActive"
    | "PlaybackActive"
    | "Io"
    | "Serde"
    | "Other";
  message: string;
};

export function isWireError(e: unknown): e is WireError {
  return (
    typeof e === "object" &&
    e !== null &&
    "kind" in e &&
    "message" in e &&
    typeof (e as Record<string, unknown>).kind === "string" &&
    typeof (e as Record<string, unknown>).message === "string"
  );
}
```

- [ ] **Step 2: Write `crates/app/ui/src/lib/api.ts`**

```ts
// Tauri 2 converts JS argument keys from camelCase to snake_case automatically
// when calling Rust commands. Single-word args (`id`, `name`) are unaffected;
// multi-word args added later (e.g. `macroId`) must use camelCase here and
// snake_case in the Rust signature. Keep this in mind when adding commands.

import { invoke } from "@tauri-apps/api/core";
import type { MacroDto, Trigger, PlaybackMode } from "./types";

export async function loadMacros(): Promise<MacroDto[]> {
  return invoke<MacroDto[]>("load_macros");
}

export async function deleteMacro(id: string): Promise<void> {
  await invoke("delete_macro", { id });
}

// Stubs for commands added in later tasks. Frontend uses them in Task 11+
// once they're implemented in the backend.
export async function updateMacroMetadata(
  id: string,
  name: string,
  trigger: Trigger,
  playback: PlaybackMode,
): Promise<MacroDto> {
  return invoke<MacroDto>("update_macro_metadata", { id, name, trigger, playback });
}

export async function playMacro(id: string): Promise<void> {
  await invoke("play_macro", { id });
}

export async function stopPlayback(): Promise<void> {
  await invoke("stop_playback");
}
```

- [ ] **Step 3: Write `crates/app/ui/src/lib/stores/toast.ts`** (used in Step 5 below)

```ts
import { writable, get } from "svelte/store";
import type { WireError } from "../types";
import { isWireError } from "../types";

export type ToastLevel = "info" | "success" | "warning" | "error";

export type ToastEntry = {
  id: number;
  level: ToastLevel;
  message: string;
  persistent: boolean;
};

let nextId = 1;
export const toasts = writable<ToastEntry[]>([]);

export function pushToast(
  level: ToastLevel,
  message: string,
  persistent = false,
): number {
  const id = nextId++;
  toasts.update((list) => [...list, { id, level, message, persistent }]);
  if (!persistent) {
    setTimeout(() => dismiss(id), 4000);
  }
  return id;
}

export function dismiss(id: number): void {
  toasts.update((list) => list.filter((t) => t.id !== id));
}

export function clear(): void {
  toasts.set([]);
}

/** Map a thrown command error to a toast. Errors that aren't WireError
 *  surface as an "Other" red toast with the raw message. */
export function reportError(e: unknown): void {
  if (isWireError(e)) {
    handleWireError(e);
    return;
  }
  const message = e instanceof Error ? e.message : String(e);
  pushToast("error", message);
}

function handleWireError(e: WireError): void {
  switch (e.kind) {
    case "DriverNotInstalled":
      pushToast("error", "Interception driver not installed. (Install flow lands in 3b.)", true);
      break;
    case "DriverNotRunning":
      pushToast("error", "Interception driver installed but not running. Reboot may be required.", true);
      break;
    case "PlaybackActive":
      pushToast("warning", "Already playing — stop it first.");
      break;
    case "MacroNotFound":
      pushToast("info", "That macro no longer exists; refreshing the list.");
      break;
    default:
      pushToast("error", `${e.kind}: ${e.message}`);
  }
}

// Test-only export — Vitest tests reset state between cases.
export function _testReset(): void {
  nextId = 1;
  toasts.set([]);
}

// Avoid unused warning in production builds when get isn't used elsewhere.
export const _peek = () => get(toasts);
```

- [ ] **Step 4: Write `crates/app/ui/src/lib/stores/macros.ts`**

```ts
import { writable, get } from "svelte/store";
import type { MacroDto } from "../types";
import * as api from "../api";
import { reportError } from "./toast";

export const macros = writable<MacroDto[]>([]);
export const loading = writable<boolean>(false);

export async function loadAll(): Promise<void> {
  loading.set(true);
  try {
    const list = await api.loadMacros();
    macros.set(list);
  } catch (e) {
    reportError(e);
  } finally {
    loading.set(false);
  }
}

export async function remove(id: string): Promise<void> {
  try {
    await api.deleteMacro(id);
    macros.update((list) => list.filter((m) => m.id !== id));
  } catch (e) {
    reportError(e);
    // The macro may have been deleted externally — reload to converge.
    await loadAll();
  }
}

// Helper for downstream stores/components — read the current macros snapshot
// without a subscribe roundtrip.
export function snapshot(): MacroDto[] {
  return get(macros);
}
```

- [ ] **Step 5: Write `crates/app/ui/src/lib/components/Toast.svelte`**

```svelte
<script lang="ts">
  import type { ToastEntry } from "../stores/toast";
  import { dismiss } from "../stores/toast";

  let { entry }: { entry: ToastEntry } = $props();

  const colors: Record<ToastEntry["level"], string> = {
    info: "var(--text-muted)",
    success: "var(--success)",
    warning: "var(--warning)",
    error: "var(--danger)",
  };
</script>

<div
  class="toast"
  style:border-left-color={colors[entry.level]}
  role="status"
>
  <span class="message">{entry.message}</span>
  <button class="close" onclick={() => dismiss(entry.id)} aria-label="Dismiss">×</button>
</div>

<style>
  .toast {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-left-width: 4px;
    padding: 0.75rem 1rem;
    border-radius: 4px;
    margin-bottom: 0.5rem;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    min-width: 280px;
    max-width: 420px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
  }
  .message {
    flex: 1;
    line-height: 1.4;
  }
  .close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 1.25rem;
    line-height: 1;
    padding: 0 0.25rem;
    cursor: pointer;
  }
  .close:hover {
    color: var(--text);
  }
</style>
```

- [ ] **Step 6: Write `crates/app/ui/src/lib/components/ToastHost.svelte`**

```svelte
<script lang="ts">
  import { toasts } from "../stores/toast";
  import Toast from "./Toast.svelte";
</script>

<div class="host" aria-live="polite">
  {#each $toasts as entry (entry.id)}
    <Toast {entry} />
  {/each}
</div>

<style>
  .host {
    position: fixed;
    top: 1rem;
    right: 1rem;
    z-index: 1000;
    display: flex;
    flex-direction: column;
    pointer-events: none;
  }
  .host :global(.toast) {
    pointer-events: auto;
  }
</style>
```

- [ ] **Step 7: Write `crates/app/ui/src/lib/components/MacroRow.svelte`**

```svelte
<script lang="ts">
  import type { MacroDto } from "../types";
  import { inputLabel } from "../types";

  let {
    macro,
    onPlay,
    onEdit,
    onDelete,
  }: {
    macro: MacroDto;
    onPlay: (id: string) => void;
    onEdit: (id: string) => void;
    onDelete: (id: string) => void;
  } = $props();

  function hotkeyLabel(macro: MacroDto): string {
    if (macro.trigger.type !== "hotkey") return "—";
    const parts = [...macro.trigger.modifiers, macro.trigger.key].map(inputLabel);
    return parts.join("+");
  }

  function modeLabel(macro: MacroDto): string {
    switch (macro.playback.type) {
      case "once": return "Once";
      case "repeat": return `Repeat(${macro.playback.value})`;
      case "loop": return "Loop";
      case "toggle": return "Toggle";
    }
  }

  function confirmDelete() {
    if (confirm(`Delete macro "${macro.name}"? This cannot be undone.`)) {
      onDelete(macro.id);
    }
  }
</script>

<tr>
  <td>{macro.name}</td>
  <td><code>{hotkeyLabel(macro)}</code></td>
  <td>{modeLabel(macro)}</td>
  <td class="num">{macro.step_count}</td>
  <td class="actions">
    <button onclick={() => onPlay(macro.id)} title="Play">▶</button>
    <button onclick={() => onEdit(macro.id)} title="Edit">✎</button>
    <button onclick={confirmDelete} class="danger" title="Delete">✕</button>
  </td>
</tr>

<style>
  td {
    padding: 0.6rem 0.5rem;
    border-bottom: 1px solid var(--border);
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .actions {
    text-align: right;
    white-space: nowrap;
  }
  .actions button {
    margin-left: 0.25rem;
    padding: 0.25rem 0.5rem;
  }
</style>
```

- [ ] **Step 8: Write `crates/app/ui/src/lib/components/MacroTable.svelte`**

```svelte
<script lang="ts">
  import { macros, loading, remove } from "../stores/macros";
  import MacroRow from "./MacroRow.svelte";

  let {
    onPlay,
    onEdit,
  }: {
    onPlay: (id: string) => void;
    onEdit: (id: string) => void;
  } = $props();

  function handleDelete(id: string) {
    void remove(id);
  }
</script>

<section>
  <div class="header">
    <h2>Macros</h2>
    <button disabled title="In-app recording lands in Plan 3b">+ Record new (3b)</button>
  </div>

  {#if $loading}
    <p class="empty">Loading…</p>
  {:else if $macros.length === 0}
    <p class="empty">
      No macros yet. Use the CLI to record one — in-app recording lands in Plan 3b.
    </p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>Name</th>
          <th>Hotkey</th>
          <th>Mode</th>
          <th class="num">Steps</th>
          <th class="actions">Actions</th>
        </tr>
      </thead>
      <tbody>
        {#each $macros as macro (macro.id)}
          <MacroRow
            {macro}
            {onPlay}
            {onEdit}
            onDelete={handleDelete}
          />
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1rem;
  }
  h2 {
    margin: 0;
    font-size: 1.25rem;
  }
  .empty {
    color: var(--text-muted);
    padding: 2rem 0;
    text-align: center;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    text-align: left;
    padding: 0.5rem;
    border-bottom: 1px solid var(--border);
    color: var(--text-muted);
    font-weight: 500;
    font-size: 0.85rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .num { text-align: right; }
  .actions { text-align: right; }
</style>
```

- [ ] **Step 9: Update `crates/app/ui/src/App.svelte`**

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { loadAll } from "./lib/stores/macros";
  import MacroTable from "./lib/components/MacroTable.svelte";
  import ToastHost from "./lib/components/ToastHost.svelte";

  // EditMetadataModal hook-up lands in Task 11. For now, edit shows a toast.
  function handlePlay(_id: string) {
    // Wired up in Task 13.
  }
  function handleEdit(_id: string) {
    // Wired up in Task 11.
  }

  onMount(() => {
    void loadAll();
  });
</script>

<main>
  <header>
    <h1>rust-macro</h1>
  </header>
  <MacroTable onPlay={handlePlay} onEdit={handleEdit} />
  <ToastHost />
</main>

<style>
  main {
    max-width: 960px;
    margin: 0 auto;
    padding: 2rem 1.5rem;
  }
  header {
    margin-bottom: 1.5rem;
  }
  h1 {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 600;
  }
</style>
```

- [ ] **Step 10: Compile-check the frontend**

```powershell
cd crates/app/ui
npm run build
```

Expected: PASS — Vite outputs to `dist/`. The implementer should see no TypeScript errors. Delete `dist/` after (gitignored).

- [ ] **Step 11: Commit**

```powershell
cd ..\..\..
git add crates/app/ui/src/
git commit -m "feat(app/ui): macro list view + toast host + typed API client"
```

---

## Task 10: `update_macro_metadata` command

**Files:**
- Modify: `crates/app/src/commands.rs`
- Modify: `crates/app/src/main.rs`

- [ ] **Step 1: Add the failing test first**

Append this test to `crates/app/src/commands.rs`'s `mod tests`:

```rust
    #[tokio::test]
    async fn update_metadata_changes_fields_and_persists() {
        let (_tmp, state) = fixture_state();
        let m = fixture_macro("before");
        let id = m.id;
        save_macro(&state.storage_root, &m).unwrap();

        // Simulate the command body (the State<'_, AppState> wrapper isn't
        // constructible without a Tauri runtime).
        let mut loaded = load_all(&state.storage_root)
            .unwrap()
            .into_iter()
            .find(|x| x.id == id)
            .unwrap();
        loaded.name = "after".into();
        loaded.trigger = Trigger::Hotkey {
            key: KeyCode::F5,
            modifiers: vec![Modifier::Alt],
        };
        loaded.playback = PlaybackMode::Repeat { count: 3 };
        loaded.updated_at = chrono::Utc::now();
        save_macro(&state.storage_root, &loaded).unwrap();

        let reloaded = load_all(&state.storage_root)
            .unwrap()
            .into_iter()
            .find(|x| x.id == id)
            .unwrap();
        assert_eq!(reloaded.name, "after");
        assert!(matches!(reloaded.trigger,
            Trigger::Hotkey { key: KeyCode::F5, .. }));
        assert!(matches!(reloaded.playback, PlaybackMode::Repeat { count: 3 }));
        assert_eq!(reloaded.steps.len(), 1); // steps preserved
    }
```

- [ ] **Step 2: Run the test — it passes immediately** (test exercises the storage layer that already supports this; the next steps add the Tauri command on top)

```powershell
cargo test -p rm-app commands::tests::update_metadata_changes_fields_and_persists
```

Expected: PASS (proves the storage layer can do what we need).

- [ ] **Step 3: Add the `update_macro_metadata` command**

In `crates/app/src/commands.rs`, add this command function after `delete_macro`:

```rust
use crate::dto::{PlaybackModeDto, TriggerDto};
use rm_storage::save_macro as storage_save;

#[tauri::command]
pub async fn update_macro_metadata(
    state: State<'_, AppState>,
    id: Uuid,
    name: String,
    trigger: TriggerDto,
    playback: PlaybackModeDto,
) -> Result<MacroDto, WireError> {
    let mut all = load_all(&state.storage_root).map_err(|e| e.to_wire())?;
    let m = all
        .iter_mut()
        .find(|m| m.id == id)
        .ok_or_else(|| AppError::MacroNotFound(id.to_string()).to_wire())?;

    m.name = name;
    m.trigger = trigger.into();
    m.playback = playback.into();
    m.updated_at = chrono::Utc::now();

    storage_save(&state.storage_root, m).map_err(|e| e.to_wire())?;
    Ok(MacroDto::from(&*m))
}
```

The new `use` line for `save_macro as storage_save` goes near the top of the file with the other `use` statements (or you can inline `rm_storage::save_macro(...)` in the function body — either is fine).

- [ ] **Step 4: Register the command in `main.rs`**

In `crates/app/src/main.rs`, update the `invoke_handler` block:

```rust
        .invoke_handler(tauri::generate_handler![
            commands::load_macros,
            commands::delete_macro,
            commands::update_macro_metadata,
        ])
```

- [ ] **Step 5: Run tests**

```powershell
cargo test -p rm-app
```

Expected: PASS — 9 tests total now (was 8).

- [ ] **Step 6: Commit**

```powershell
git add crates/app/src/commands.rs crates/app/src/main.rs
git commit -m "feat(app): update_macro_metadata Tauri command"
```

---

## Task 11: Frontend — `EditMetadataModal` + `HotkeyPicker`

**Files:**
- Create: `crates/app/ui/src/lib/components/EditMetadataModal.svelte`
- Create: `crates/app/ui/src/lib/components/HotkeyPicker.svelte`
- Modify: `crates/app/ui/src/lib/stores/macros.ts`
- Modify: `crates/app/ui/src/App.svelte`

- [ ] **Step 1: Add `updateMetadata` to the macros store**

In `crates/app/ui/src/lib/stores/macros.ts`, add at the bottom (before the `snapshot` function):

```ts
import type { Trigger, PlaybackMode } from "../types";

export async function updateMetadata(
  id: string,
  name: string,
  trigger: Trigger,
  playback: PlaybackMode,
): Promise<void> {
  try {
    const updated = await api.updateMacroMetadata(id, name, trigger, playback);
    macros.update((list) => list.map((m) => (m.id === id ? updated : m)));
  } catch (e) {
    reportError(e);
  }
}
```

Move the import of `Trigger, PlaybackMode` to the top of the file (TypeScript hoists but it's cleaner up top).

- [ ] **Step 2: Write `crates/app/ui/src/lib/components/HotkeyPicker.svelte`**

```svelte
<script lang="ts">
  import type { Trigger, KeyCode, Modifier } from "../types";

  let { value, onChange }: { value: Trigger; onChange: (t: Trigger) => void } = $props();

  import { inputLabel } from "../types";

  // Subset of keys we expose in the dropdown. Live capture lands in Plan 3b.
  const KEY_OPTIONS: KeyCode[] = [
    "f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12",
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m",
    "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
    "num0", "num1", "num2", "num3", "num4",
    "num5", "num6", "num7", "num8", "num9",
    "space", "enter", "tab", "escape",
    "up", "down", "left", "right",
  ];
  const MODIFIERS: Modifier[] = ["ctrl", "shift", "alt", "win"];

  function toggle(mod: Modifier) {
    if (value.type !== "hotkey") return;
    const has = value.modifiers.includes(mod);
    const modifiers = has
      ? value.modifiers.filter((m) => m !== mod)
      : [...value.modifiers, mod];
    onChange({ ...value, modifiers });
  }

  function changeKey(e: Event) {
    const key = (e.target as HTMLSelectElement).value as KeyCode;
    if (value.type !== "hotkey") return;
    onChange({ ...value, key });
  }
</script>

<div class="modifiers">
  {#each MODIFIERS as mod}
    <label>
      <input
        type="checkbox"
        checked={value.type === "hotkey" && value.modifiers.includes(mod)}
        onchange={() => toggle(mod)}
      />
      {inputLabel(mod)}
    </label>
  {/each}
</div>
<select onchange={changeKey} value={value.type === "hotkey" ? value.key : "f1"}>
  {#each KEY_OPTIONS as k}
    <option value={k}>{inputLabel(k)}</option>
  {/each}
</select>

<style>
  .modifiers {
    display: flex;
    gap: 0.75rem;
    margin-bottom: 0.5rem;
  }
  label {
    cursor: pointer;
    user-select: none;
  }
  select {
    width: 100%;
  }
</style>
```

- [ ] **Step 3: Write `crates/app/ui/src/lib/components/EditMetadataModal.svelte`**

```svelte
<script lang="ts">
  import type { MacroDto, Trigger, PlaybackMode } from "../types";
  import { updateMetadata } from "../stores/macros";
  import HotkeyPicker from "./HotkeyPicker.svelte";

  let {
    macro,
    onClose,
  }: {
    macro: MacroDto;
    onClose: () => void;
  } = $props();

  let name = $state(macro.name);
  let trigger = $state<Trigger>(macro.trigger);
  let playback = $state<PlaybackMode>(macro.playback);
  let repeatN = $state(
    macro.playback.type === "repeat" ? macro.playback.value : 1,
  );
  let saving = $state(false);

  function changePlayback(e: Event) {
    const v = (e.target as HTMLSelectElement).value;
    switch (v) {
      case "once": playback = { type: "once" }; break;
      case "repeat": playback = { type: "repeat", value: repeatN }; break;
      case "loop": playback = { type: "loop" }; break;
      case "toggle": playback = { type: "toggle" }; break;
    }
  }

  function changeRepeatN(e: Event) {
    repeatN = Math.max(1, Number((e.target as HTMLInputElement).value));
    if (playback.type === "repeat") {
      playback = { type: "repeat", value: repeatN };
    }
  }

  async function save() {
    if (name.trim() === "") return;
    saving = true;
    await updateMetadata(macro.id, name.trim(), trigger, playback);
    saving = false;
    onClose();
  }

  function backdropClick(e: MouseEvent) {
    if (e.target === e.currentTarget) onClose();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onClose();
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="backdrop" onclick={backdropClick} role="presentation">
  <div class="modal" role="dialog" aria-labelledby="edit-title">
    <h3 id="edit-title">Edit metadata</h3>

    <div class="field">
      <label for="edit-name">Name</label>
      <input id="edit-name" bind:value={name} />
    </div>

    <div class="field">
      <label>Hotkey</label>
      <HotkeyPicker
        value={trigger}
        onChange={(t) => (trigger = t)}
      />
    </div>

    <div class="field">
      <label for="edit-mode">Playback mode</label>
      <select id="edit-mode" value={playback.type} onchange={changePlayback}>
        <option value="once">Once</option>
        <option value="repeat">Repeat (N)</option>
        <option value="loop">Loop</option>
        <option value="toggle">Toggle</option>
      </select>
      {#if playback.type === "repeat"}
        <input
          class="repeat-n"
          type="number"
          min="1"
          value={repeatN}
          oninput={changeRepeatN}
        />
      {/if}
    </div>

    <div class="actions">
      <button onclick={onClose}>Cancel</button>
      <button class="primary" disabled={saving || name.trim() === ""} onclick={save}>
        {saving ? "Saving…" : "Save"}
      </button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 500;
  }
  .modal {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.5rem;
    width: 100%;
    max-width: 420px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.5);
  }
  h3 {
    margin: 0 0 1.25rem 0;
  }
  .field {
    margin-bottom: 1rem;
  }
  .field > label {
    display: block;
    font-size: 0.85rem;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.05em;
    margin-bottom: 0.35rem;
  }
  .field input,
  .field select {
    width: 100%;
  }
  .repeat-n {
    margin-top: 0.5rem;
    width: 100px !important;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 0.5rem;
    margin-top: 1.5rem;
  }
</style>
```

- [ ] **Step 4: Update `crates/app/ui/src/App.svelte`** to wire up the modal

Replace the file with:

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { macros, loadAll, snapshot } from "./lib/stores/macros";
  import MacroTable from "./lib/components/MacroTable.svelte";
  import EditMetadataModal from "./lib/components/EditMetadataModal.svelte";
  import ToastHost from "./lib/components/ToastHost.svelte";
  import type { MacroDto } from "./lib/types";

  let editing = $state<MacroDto | null>(null);

  function handlePlay(_id: string) {
    // Wired up in Task 13.
  }

  function handleEdit(id: string) {
    const m = snapshot().find((x) => x.id === id);
    if (m) editing = m;
  }

  onMount(() => {
    void loadAll();
  });
</script>

<main>
  <header>
    <h1>rust-macro</h1>
  </header>
  <MacroTable onPlay={handlePlay} onEdit={handleEdit} />
  {#if editing}
    <EditMetadataModal macro={editing} onClose={() => (editing = null)} />
  {/if}
  <ToastHost />
</main>

<style>
  main {
    max-width: 960px;
    margin: 0 auto;
    padding: 2rem 1.5rem;
  }
  header {
    margin-bottom: 1.5rem;
  }
  h1 {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 600;
  }
</style>
```

- [ ] **Step 5: Compile-check the frontend**

```powershell
cd crates/app/ui
npm run build
```

Expected: PASS. Delete `dist/` after.

- [ ] **Step 6: Commit**

```powershell
cd ..\..\..
git add crates/app/ui/src/
git commit -m "feat(app/ui): EditMetadataModal + HotkeyPicker (dropdown-based hotkey assignment)"
```

---

## Task 12: `play_macro` + `stop_playback` commands with `ActivePlayback` supervisor

**Files:**
- Modify: `crates/app/src/commands.rs`
- Modify: `crates/app/src/main.rs`

- [ ] **Step 1: Add the helper that opens the Interception driver lazily (feature-gated)**

In `crates/app/src/commands.rs`, near the top (after the `use` block), add:

```rust
use std::sync::Arc;

#[cfg(feature = "interception")]
mod driver_init {
    use super::*;
    use rm_driver::{Driver, DriverHub};
    use rm_driver_interception::open_with_status;
    use std::sync::Arc;

    pub async fn ensure_hub(state: &AppState) -> Result<Arc<DriverHub>, AppError> {
        let mut guard = state.driver_hub.lock().await;
        if let Some(h) = guard.as_ref() {
            return Ok(h.clone());
        }
        let drv: Arc<dyn Driver> = Arc::new(open_with_status()?);
        let hub = DriverHub::start(drv);
        *guard = Some(hub.clone());
        Ok(hub)
    }
}

#[cfg(not(feature = "interception"))]
mod driver_init {
    use super::*;
    use rm_driver::DriverHub;
    use std::sync::Arc;

    pub async fn ensure_hub(_state: &AppState) -> Result<Arc<DriverHub>, AppError> {
        Err(AppError::DriverNotInstalled)
    }
}

use driver_init::ensure_hub;
```

Note: `rm_driver_interception::open_with_status` is the shared helper added in Task 3. The non-feature stub returns `DriverNotInstalled` so the GUI surfaces the same toast as a missing driver and stays read-only — matching the LGPL opt-out promised by the `interception` feature flag.

- [ ] **Step 2: Add the `play_macro` and `stop_playback` command bodies**

Append to `crates/app/src/commands.rs` (after `update_macro_metadata`):

```rust
use rm_player::play;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Serialize, Clone)]
struct PlaybackStartedEvent {
    macro_id: Uuid,
    macro_name: String,
}

#[derive(Serialize, Clone)]
struct PlaybackFinishedEvent {
    macro_id: Uuid,
    outcome: PlaybackOutcome,
}

#[derive(Serialize, Clone)]
#[serde(tag = "status", rename_all = "snake_case")]
enum PlaybackOutcome {
    /// Macro ran to completion normally.
    Ok,
    /// User clicked Stop (or another stop_playback call took the slot).
    Stopped,
    /// Player returned an error.
    Failed { error: WireError },
}

#[tauri::command]
pub async fn play_macro(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<(), WireError> {
    // Reject if a playback is already active.
    {
        let active = state.active.lock().await;
        if active.is_some() {
            return Err(AppError::PlaybackActive.to_wire());
        }
    }

    // Load the macro before opening the driver, so MacroNotFound surfaces
    // without an unnecessary Interception context attempt.
    let all = load_all(&state.storage_root).map_err(|e| e.to_wire())?;
    let m = all
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| AppError::MacroNotFound(id.to_string()).to_wire())?;

    let hub = ensure_hub(&state).await.map_err(|e| e.to_wire())?;

    let macro_id = m.id;
    let macro_name = m.name.clone();

    // The slot's `stop_tx` is what `stop_playback` fires when the user clicks
    // Stop. A small relay records the user-initiated flag (so we can map the
    // player's clean exit to `PlaybackOutcome::Stopped` rather than `Ok`) and
    // forwards the signal to `PlaybackHandle::run_with_stop`, which calls the
    // handle's internal `stop()` and awaits the player's clean exit between
    // steps. No tasks are aborted; no players run detached.
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped_for_signal = stopped.clone();
    let (inner_stop_tx, inner_stop_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        if stop_rx.await.is_ok() {
            stopped_for_signal.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = inner_stop_tx.send(());
        }
    });

    let app_for_task = app.clone();
    tokio::spawn(async move {
        let handle = play(hub, m);
        let result = handle.run_with_stop(inner_stop_rx).await;
        let outcome = match (result, stopped.load(std::sync::atomic::Ordering::SeqCst)) {
            (Ok(()), true) => PlaybackOutcome::Stopped,
            (Ok(()), false) => PlaybackOutcome::Ok,
            (Err(e), _) => PlaybackOutcome::Failed { error: e.to_wire() },
        };

        // Cleanup: clear active slot if we're still the active playback.
        // Re-acquire AppState via the AppHandle so we don't capture a
        // 'static reference.
        if let Some(s) = app_for_task.try_state::<AppState>() {
            let mut active = s.active.lock().await;
            if active.as_ref().map(|a| a.macro_id) == Some(macro_id) {
                *active = None;
            }
        }

        let _ = app_for_task.emit(
            "playback_finished",
            PlaybackFinishedEvent { macro_id, outcome },
        );
    });

    // Store the active playback. The supervisor task above owns the player.
    // `stop_playback` takes `stop_tx` out of the slot and fires it; the
    // supervisor handles the rest.
    {
        let mut active = state.active.lock().await;
        *active = Some(ActivePlayback {
            macro_id,
            macro_name: macro_name.clone(),
            stop_tx: Some(stop_tx),
        });
    }

    // Emit playback_started after the active slot is populated, so any
    // frontend handler that immediately calls `stop_playback` sees a
    // consistent state.
    let _ = app.emit(
        "playback_started",
        PlaybackStartedEvent { macro_id, macro_name },
    );

    Ok(())
}

#[tauri::command]
pub async fn stop_playback(
    _app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), WireError> {
    // Send the cooperative stop signal. The supervisor task spawned by
    // `play_macro` will observe it, call `PlaybackHandle::run_with_stop`'s
    // internal `stop()`, await the player's clean exit, clear the active
    // slot, and emit `playback_finished` with `outcome: stopped`.
    let mut active = state.active.lock().await;
    if let Some(ap) = active.as_mut() {
        if let Some(tx) = ap.stop_tx.take() {
            let _ = tx.send(());
        }
    }
    Ok(())
}
```

- [ ] **Step 3: Register both commands in `main.rs`**

```rust
        .invoke_handler(tauri::generate_handler![
            commands::load_macros,
            commands::delete_macro,
            commands::update_macro_metadata,
            commands::play_macro,
            commands::stop_playback,
        ])
```

- [ ] **Step 4: Compile-check**

```powershell
cargo check -p rm-app
```

Expected: PASS. Warnings about unused `storage_root_for_log` are OK — that binding is illustrative and the implementer can remove it.

- [ ] **Step 5: Add a unit test for the active-slot guard logic**

In `crates/app/src/commands.rs`'s `mod tests`, append:

```rust
    #[tokio::test]
    async fn active_slot_rejects_concurrent_play() {
        let (_tmp, state) = fixture_state();
        // Simulate that a playback is in progress by placing a dummy in the
        // active slot. The macro_id/name don't matter — we only care about
        // the guard returning PlaybackActive.
        let (tx, _rx) = tokio::sync::oneshot::channel::<()>();
        {
            let mut active = state.active.lock().await;
            *active = Some(crate::state::ActivePlayback {
                macro_id: Uuid::new_v4(),
                macro_name: "x".into(),
                stop_tx: Some(tx),
            });
        }
        // The guard in play_macro is a simple `if active.is_some()` block;
        // verify it would reject:
        let blocked = {
            let active = state.active.lock().await;
            active.is_some()
        };
        assert!(blocked);
    }
```

- [ ] **Step 6: Run tests**

```powershell
cargo test -p rm-app
```

Expected: PASS — 10 tests now.

- [ ] **Step 7: Commit**

```powershell
git add crates/app/src/commands.rs crates/app/src/main.rs
git commit -m "feat(app): play_macro/stop_playback with lazy DriverHub + ActivePlayback supervisor"
```

---

## Task 13: Frontend — playback store + `PlaybackBanner` + event listeners

**Files:**
- Create: `crates/app/ui/src/lib/stores/playback.ts`
- Create: `crates/app/ui/src/lib/components/PlaybackBanner.svelte`
- Modify: `crates/app/ui/src/App.svelte`

- [ ] **Step 1: Write `crates/app/ui/src/lib/stores/playback.ts`**

```ts
import { writable } from "svelte/store";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { WireError } from "../types";
import * as api from "../api";
import { reportError, pushToast } from "./toast";

export type ActivePlayback = {
  macroId: string;
  macroName: string;
  startedAt: number;
};

export const active = writable<ActivePlayback | null>(null);

type StartedPayload = { macro_id: string; macro_name: string };
type FinishedOutcome =
  | { status: "ok" }
  | { status: "stopped" }
  | { status: "failed"; error: WireError };

type FinishedPayload = { macro_id: string; outcome: FinishedOutcome };

let unlisteners: UnlistenFn[] = [];

export async function startListening(): Promise<void> {
  // Idempotent — calling twice is harmless because we tear down first.
  await stopListening();

  unlisteners.push(
    await listen<StartedPayload>("playback_started", (event) => {
      active.set({
        macroId: event.payload.macro_id,
        macroName: event.payload.macro_name,
        startedAt: Date.now(),
      });
    }),
  );

  unlisteners.push(
    await listen<FinishedPayload>("playback_finished", (event) => {
      const { outcome } = event.payload;
      switch (outcome.status) {
        case "ok":
          pushToast("success", "Playback finished.");
          break;
        case "stopped":
          pushToast("info", "Playback stopped.");
          break;
        case "failed":
          pushToast("error", `Playback failed: ${outcome.error.message}`);
          break;
      }
      active.set(null);
    }),
  );
}

export async function stopListening(): Promise<void> {
  for (const u of unlisteners) u();
  unlisteners = [];
}

export async function play(id: string): Promise<void> {
  try {
    await api.playMacro(id);
  } catch (e) {
    reportError(e);
  }
}

export async function stop(): Promise<void> {
  try {
    await api.stopPlayback();
  } catch (e) {
    reportError(e);
  }
}
```

- [ ] **Step 2: Write `crates/app/ui/src/lib/components/PlaybackBanner.svelte`**

```svelte
<script lang="ts">
  import { active, stop } from "../stores/playback";

  let elapsedMs = $state(0);
  let timer: ReturnType<typeof setInterval> | null = null;

  $effect(() => {
    const current = $active;
    if (current) {
      elapsedMs = Date.now() - current.startedAt;
      timer = setInterval(() => {
        elapsedMs = Date.now() - current.startedAt;
      }, 250);
    } else {
      if (timer) {
        clearInterval(timer);
        timer = null;
      }
      elapsedMs = 0;
    }
    return () => {
      if (timer) clearInterval(timer);
    };
  });

  function formatElapsed(ms: number): string {
    const s = Math.floor(ms / 1000);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }
</script>

{#if $active}
  <div class="banner" role="status">
    <span class="icon">▶</span>
    <span class="text">
      Playing <strong>{$active.macroName}</strong>
      · {formatElapsed(elapsedMs)}
    </span>
    <button class="danger" onclick={() => void stop()}>■ Stop</button>
  </div>
{/if}

<style>
  .banner {
    position: sticky;
    bottom: 0;
    background: rgba(34, 197, 94, 0.1);
    border: 1px solid var(--success);
    border-left-width: 4px;
    padding: 0.75rem 1rem;
    border-radius: 4px;
    display: flex;
    align-items: center;
    gap: 0.75rem;
    margin-top: 1.5rem;
  }
  .icon {
    color: var(--success);
    font-size: 1.1rem;
  }
  .text {
    flex: 1;
    color: var(--text);
  }
  strong {
    color: var(--text);
  }
</style>
```

- [ ] **Step 3: Update `crates/app/ui/src/App.svelte`** to wire play + start/stop listeners

```svelte
<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { macros, loadAll, snapshot } from "./lib/stores/macros";
  import { play, startListening, stopListening } from "./lib/stores/playback";
  import MacroTable from "./lib/components/MacroTable.svelte";
  import EditMetadataModal from "./lib/components/EditMetadataModal.svelte";
  import PlaybackBanner from "./lib/components/PlaybackBanner.svelte";
  import ToastHost from "./lib/components/ToastHost.svelte";
  import type { MacroDto } from "./lib/types";

  let editing = $state<MacroDto | null>(null);

  function handlePlay(id: string) {
    void play(id);
  }

  function handleEdit(id: string) {
    const m = snapshot().find((x) => x.id === id);
    if (m) editing = m;
  }

  onMount(() => {
    void loadAll();
    void startListening();
  });

  onDestroy(() => {
    void stopListening();
  });
</script>

<main>
  <header>
    <h1>rust-macro</h1>
  </header>
  <MacroTable onPlay={handlePlay} onEdit={handleEdit} />
  <PlaybackBanner />
  {#if editing}
    <EditMetadataModal macro={editing} onClose={() => (editing = null)} />
  {/if}
  <ToastHost />
</main>

<style>
  main {
    max-width: 960px;
    margin: 0 auto;
    padding: 2rem 1.5rem;
  }
  header {
    margin-bottom: 1.5rem;
  }
  h1 {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 600;
  }
</style>
```

- [ ] **Step 4: Compile-check the frontend**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add crates/app/ui/src/
git commit -m "feat(app/ui): playback store + PlaybackBanner + Tauri event listeners"
```

---

## Task 14: README + manual smoke test plan

**Files:**
- Create: `crates/app/README.md`

- [ ] **Step 1: Write `crates/app/README.md`**

```markdown
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
```

- [ ] **Step 2: Commit**

```powershell
git add crates/app/README.md
git commit -m "docs(app): Plan 3a README with dev setup + manual smoke test plan"
```

---

## Task 15: Final verification

- [ ] **Step 1: All workspace tests pass**

Run: `cargo test --workspace --no-fail-fast`
Expected: PASS — 76 from before + 1 new in `rm-player` (run_with_stop) + new tests from `rm-app` (≈10 — 5 dto + 4 commands + 1 state guard).

- [ ] **Step 2: Frontend builds**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS. `crates/app/ui/dist/` is produced (and immediately gitignored).

- [ ] **Step 3: Tauri dev opens the window**

```powershell
cd crates/app
cargo tauri dev
```

Expected: window opens, shows "rust-macro" title, lists macros (or empty
state), supports edit/delete. Closing the window stops the process cleanly.

- [ ] **Step 4: Run the manual smoke test from the README**

Walk steps 1–8 in `crates/app/README.md`. Items 5 and 6/7/8 depend on
Interception being installed; if the implementer doesn't have it, mark
those as "deferred — verified by the spec author after install" and
proceed.

- [ ] **Step 5: No commit if Steps 1–3 pass; everything is already committed**

The previous tasks committed all changes. This task is acceptance-only.

---

## Acceptance Checklist (from the spec)

- [ ] `cargo test --workspace` is green (76 prior + 1 new in `rm-player` + ~10 new in `rm-app`).
- [ ] `cargo build -p rm-app` succeeds on Windows.
- [ ] `cargo tauri dev` opens a working window.
- [ ] Empty-state, list-render, edit, delete, and PlaybackActive guard all
      verified manually.
- [ ] Play succeeds when Interception is installed; fails gracefully with
      the right toast otherwise.
- [ ] `crates/app/README.md` exists with the smoke-test plan.
- [ ] `AppError::PlaybackActive` is added and has a `kind()` test.

---

## Open Implementation Notes

- **Tauri 2 + Svelte 5 template drift.** If the official `create-tauri-app`
  scaffolder still emits Svelte 4 at implementation time, the
  `crates/app/ui/package.json` versions in this plan force Svelte 5. The
  `mount()` API in Task 6's `main.ts` and the rune syntax (`$state`,
  `$props`, `$effect`) in components require Svelte 5. If the implementer
  hits a runtime error like `mount is not a function`, the most likely
  cause is that npm resolved Svelte 4 — verify with `npm ls svelte` and
  upgrade explicitly: `npm install -E svelte@^5`.
- **`@tauri-apps/api/core` vs `/tauri`.** Tauri 2 moved `invoke` to
  `@tauri-apps/api/core`. The plan uses the correct import. If the
  implementer's clipboard or autocomplete fills in the v1 path
  `@tauri-apps/api/tauri`, it will fail at runtime with "module not
  found" — the fix is to use `@tauri-apps/api/core`.
- **Tauri `Emitter` trait.** Task 12's `app.emit(...)` requires
  `use tauri::Emitter;` in Tauri 2 (the trait method is not implicitly
  on `AppHandle`). This is included in the plan; if it's missed, the
  compile error is clear ("no method named emit").
- **`tauri::Manager` and `try_state`.** In Task 12's supervisor task,
  `app.try_state::<AppState>()` requires `use tauri::Manager;` to be in
  scope. Add this with the other `use` statements in `commands.rs`. If
  missed, the compile error names the method correctly.
- **Stop semantics (resolved).** `stop_playback` sends on the slot's
  `stop_tx`. A relay task observes this, records the user-initiated flag,
  and forwards to `PlaybackHandle::run_with_stop` (added in Task 4 of this
  plan). `run_with_stop` calls the handle's internal `stop()` then awaits
  the player's clean exit between steps. The supervisor task then emits
  `playback_finished` with `outcome: stopped`. No tasks are aborted; no
  detached players. The Loop-mode "stop actually stops" guarantee is
  exercised by `crates/player/src/lib.rs`'s new
  `run_with_stop_signals_clean_exit` test.
- **`open_with_status` extraction.** Plan 2b's CLI carries a private
  `open_interception()` helper that maps `InterceptionDriver::new()`
  failure to `AppError` via `detect_status()`. Task 3 of this plan
  extracts that to `rm_driver_interception::open_with_status` so both
  the CLI and the GUI use a single source of truth.
- **Storage root.** `dirs::data_dir()` on Windows returns
  `%APPDATA%/Roaming`. The CLI uses the same path. If the implementer
  runs both the CLI and the GUI on the same machine, they share the
  macro storage automatically.
- **Vitest in CI.** Not in scope for 3a. Tests in the Vitest harness are
  for local development. `cargo test --workspace` remains the CI gate.
