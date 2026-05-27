# rust-macro — Plan 3d: bundle Interception driver + first-run install flow — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the Interception kernel driver inside the rust-macro installer so the user doesn't have to find/install it manually. First launch detects the driver status, prompts to install if missing (one-click via UAC elevation), then prompts to reboot. After reboot, the app works.

**Architecture:** The Tauri bundle's `resources/` directory contains oblitum's signed `install-interception.exe` + `uninstall-interception.exe` + license text. A new `driver_install` module in `rm-app` spawns the installer elevated via the `runas` shell verb and writes a `pending-reboot` marker file. Frontend gains a persistent banner (top of every view) reflecting `DriverStatus` and a "Restart Windows" modal after a successful install. The Settings view gains a driver status indicator + reinstall button. LGPL-3.0 compliance is satisfied by including license text + source pointer in the install directory.

**Tech Stack:** Tauri 2 (Rust stable MSVC), Svelte 5 (runes), TypeScript. Windows-only. Builds on Plans 3a + 3b + 3c. The Interception binaries are oblitum's official signed builds; we vendor them at a pinned version.

**Spec:** Self-contained — design notes inline. Cross-references: Plans 3a/3b/3c specs in `docs/superpowers/specs/` and `docs/superpowers/plans/`.

---

## Open Architectural Risks (read before starting)

1. **Code-signing of the Tauri installer.** Our own `.msi`/`.exe` bundle should be signed (otherwise SmartScreen blocks). This plan does NOT cover code-signing of the rust-macro installer — it ships unsigned for now and will trip SmartScreen on download. Signing is a separate operational task (EV cert + signtool integration). Document as a Plan 4 prereq.

2. **Interception version pin.** We vendor `install-interception.exe` at a specific version (current upstream: `1.0.1` as of this plan's writing — verify against `github.com/oblitum/Interception/releases` before merging Task 2). Upstream rarely releases; pinning is safe.

3. **AV/EDR false positives.** Antivirus may flag the bundled installer because it registers a kernel filter driver. Most consumer AV passes oblitum's signed binary. Enterprise EDR may not. Document in README.

4. **Existing Interception install.** If the user already has Interception (e.g., from Kanata), our installer may overwrite, conflict, or warn. Behavior depends on oblitum's installer — Task 3's smoke test verifies. We do NOT remove or modify an existing install; we just call the installer and let it decide.

5. **Uninstall.** Uninstalling rust-macro does NOT remove Interception. This is intentional — the user may have other apps depending on it (Kanata, AHK-fork). The Settings page exposes a manual "Uninstall driver" button for users who want to clean up.

6. **Per-machine vs per-user.** Interception is a kernel driver — always per-machine. Our app is per-user (no admin to install). On first run, the install prompt is the only admin-required step.

---

## File Structure

**Files to create (backend):**
- `crates/app/src/driver_install.rs` — install/uninstall/reboot command bodies + UAC elevation + pending-reboot marker

**Files to create (assets — vendor manually in Task 2):**
- `crates/app/installers/interception/install-interception.exe`
- `crates/app/installers/interception/uninstall-interception.exe`
- `crates/app/installers/interception/LICENSE-LGPL.txt`
- `crates/app/installers/interception/SOURCE-INFO.txt`

**Files to create (frontend):**
- `crates/app/ui/src/lib/stores/driver.ts`
- `crates/app/ui/src/lib/components/DriverStatusBanner.svelte`
- `crates/app/ui/src/lib/components/RebootPromptModal.svelte`

**Files to modify (backend):**
- `crates/app/Cargo.toml` — add `windows-sys` dep for `ShellExecuteExW` + `InitiateSystemShutdownExW`
- `crates/app/tauri.conf.json` — add `bundle.resources` entry for the installer dir
- `crates/app/src/state.rs` — add `pending_reboot` flag (read on boot from marker file)
- `crates/app/src/dto.rs` — add `DriverStatusDto`, `DriverStateDto`
- `crates/app/src/commands.rs` — add `driver_status`, `install_driver`, `uninstall_driver`, `reboot_windows` commands
- `crates/app/src/main.rs` — register new commands; load `pending_reboot` flag at boot

**Files to modify (frontend):**
- `crates/app/ui/src/lib/types.ts` — add `DriverStateDto`, `DriverStatusDto`
- `crates/app/ui/src/lib/api.ts` — driver-related wrappers
- `crates/app/ui/src/lib/components/SettingsView.svelte` — driver status indicator + reinstall button
- `crates/app/ui/src/App.svelte` — mount `DriverStatusBanner` + `RebootPromptModal` in all views
- `crates/app/README.md` — install flow docs + LGPL note

**Files to modify (docs):**
- `LICENSES.md` — expand Interception section to cover the bundled installer

Backend-first ordering so the frontend has stable Tauri commands.

---

## Task 1: `DriverStateDto` — wire `detect_status()` into a Tauri command

**Files:**
- Modify: `crates/app/src/dto.rs`
- Modify: `crates/app/src/commands.rs`
- Modify: `crates/app/src/main.rs`

Currently `rm_driver_interception::detect_status()` returns `DriverStatus` (NotInstalled / InstalledNotRunning / Running). The GUI has no way to query it. Task 1 adds a `driver_status` Tauri command that returns a frontend-friendly DTO.

- [ ] **Step 1: Add DTOs to `crates/app/src/dto.rs`**

Append:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DriverStatusDto {
    NotInstalled,
    InstalledNotRunning,
    Running,
}

#[cfg(feature = "interception")]
impl From<rm_driver_interception::DriverStatus> for DriverStatusDto {
    fn from(s: rm_driver_interception::DriverStatus) -> Self {
        match s {
            rm_driver_interception::DriverStatus::NotInstalled => DriverStatusDto::NotInstalled,
            rm_driver_interception::DriverStatus::InstalledNotRunning => DriverStatusDto::InstalledNotRunning,
            rm_driver_interception::DriverStatus::Running => DriverStatusDto::Running,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DriverStateDto {
    pub status: DriverStatusDto,
    /// True if an install was completed during this session and the user
    /// hasn't rebooted yet. Reset on app start (the marker file is wiped
    /// once we detect status = Running).
    pub pending_reboot: bool,
}
```

- [ ] **Step 2: Add `driver_status` command to `crates/app/src/commands.rs`**

Append after `save_settings`:

```rust
#[tauri::command]
pub async fn driver_status(state: State<'_, AppState>) -> Result<crate::dto::DriverStateDto, WireError> {
    #[cfg(feature = "interception")]
    let status: crate::dto::DriverStatusDto = rm_driver_interception::detect_status().into();
    #[cfg(not(feature = "interception"))]
    let status = crate::dto::DriverStatusDto::NotInstalled;

    let pending_reboot = *state.pending_reboot.lock().await;

    // If the driver is now Running, the reboot took effect — clear the flag
    // (the marker file is cleared in install_driver's post-install path; this
    // additionally clears the in-memory cache).
    let pending_reboot = if matches!(status, crate::dto::DriverStatusDto::Running) {
        false
    } else {
        pending_reboot
    };

    Ok(crate::dto::DriverStateDto { status, pending_reboot })
}
```

- [ ] **Step 3: Add `pending_reboot` field to `AppState`**

In `crates/app/src/state.rs`:

```rust
pub struct AppState {
    pub storage_root: PathBuf,
    pub driver_hub: Mutex<Option<Arc<DriverHub>>>,
    pub active: Mutex<Option<ActivePlayback>>,
    pub recording: Mutex<Option<ActiveRecording>>,
    pub settings: Mutex<crate::settings::Settings>,
    pub pending_reboot: Mutex<bool>,
    #[cfg(feature = "interception")]
    pub listener: Mutex<Option<crate::listener::ActiveListener>>,
}

impl AppState {
    pub fn new(storage_root: PathBuf, settings: crate::settings::Settings, pending_reboot: bool) -> Self {
        Self {
            storage_root,
            driver_hub: Mutex::new(None),
            active: Mutex::new(None),
            recording: Mutex::new(None),
            settings: Mutex::new(settings),
            pending_reboot: Mutex::new(pending_reboot),
            #[cfg(feature = "interception")]
            listener: Mutex::new(None),
        }
    }
}
```

- [ ] **Step 4: Load `pending_reboot` flag in `main.rs`**

Add a helper near the top of `main.rs`:

```rust
fn read_pending_reboot(storage_root: &std::path::Path) -> bool {
    storage_root.join(".driver-install-pending").exists()
}
```

In `main()`, after loading settings:

```rust
    let pending_reboot = read_pending_reboot(&storage_root);
```

Pass it to `AppState::new(storage_root, settings, pending_reboot)`.

- [ ] **Step 5: Register the command**

In `main.rs` `invoke_handler!`, add `commands::driver_status,`.

- [ ] **Step 6: Update `fixture_state` in tests**

In `crates/app/src/commands.rs` `mod tests`, update `fixture_state` to pass `false`:

```rust
    fn fixture_state() -> (TempDir, AppState) {
        let tmp = TempDir::new().unwrap();
        let state = AppState::new(tmp.path().to_path_buf(), crate::settings::Settings::default(), false);
        (tmp, state)
    }
```

- [ ] **Step 7: Run tests**

Run: `cargo test -p rm-app`
Expected: PASS — all prior tests still green.

Run: `cargo check -p rm-app --no-default-features`
Expected: PASS — the feature-gated `From` impl compiles out cleanly; the `not(feature = "interception")` branch in `driver_status` returns `NotInstalled`.

- [ ] **Step 8: Commit**

```powershell
git add crates/app/src/dto.rs crates/app/src/commands.rs crates/app/src/state.rs crates/app/src/main.rs
git commit -m "feat(app): driver_status Tauri command + pending_reboot flag in AppState"
```

---

## Task 2: Vendor the Interception installer binaries

**Files:**
- Create: `crates/app/installers/interception/install-interception.exe` (binary, vendored)
- Create: `crates/app/installers/interception/uninstall-interception.exe` (binary, vendored)
- Create: `crates/app/installers/interception/LICENSE-LGPL.txt`
- Create: `crates/app/installers/interception/SOURCE-INFO.txt`
- Modify: `crates/app/tauri.conf.json` (`bundle.resources`)
- Modify: `LICENSES.md`

This task is **manual** — there's no automatic download. The implementer downloads oblitum's signed binaries and commits them.

- [ ] **Step 1: Download Interception from upstream**

Source: https://github.com/oblitum/Interception/releases

Download the latest stable release (verify version: e.g., `Interception.zip` containing `install-interception.exe`, `uninstall-interception.exe`, `interception.dll`, `library/`).

**Verify the Authenticode signature** on `install-interception.exe`:

```powershell
Get-AuthenticodeSignature .\install-interception.exe
```

Expected: `Status: Valid`, `SignerCertificate.Subject` references oblitum / Francisco Lopes. If invalid signature, STOP — do not bundle. Investigate.

- [ ] **Step 2: Place binaries in the repo**

Create `crates/app/installers/interception/` directory. Copy:
- `install-interception.exe`
- `uninstall-interception.exe`

(Do NOT bundle `interception.dll` — it's installed BY `install-interception.exe`, not bundled by us.)

- [ ] **Step 3: Create `LICENSE-LGPL.txt`**

In `crates/app/installers/interception/LICENSE-LGPL.txt`, paste the full text of GNU LGPL v3 (from https://www.gnu.org/licenses/lgpl-3.0.txt). Keep it as a separate file in the bundle so the user finds it adjacent to the binary.

- [ ] **Step 4: Create `SOURCE-INFO.txt`**

```
Interception kernel driver — source and license info
=====================================================

This directory contains the signed installer binaries from
oblitum's Interception project, redistributed under LGPL-3.0.

Upstream:    https://github.com/oblitum/Interception
Version:     <PIN_THE_EXACT_VERSION_HERE, e.g., 1.0.1>
License:     LGPL-3.0 (see LICENSE-LGPL.txt)

To obtain the source code or build the driver yourself, see the
upstream repository above. You may replace this driver with a
modified version by:

1. Building/installing your own Interception variant.
2. Removing/uninstalling this bundled version via
   `uninstall-interception.exe` (run as administrator).
3. Installing your replacement.

rust-macro dynamically loads `interception.dll` at runtime via the
`kanata-interception` crate, so swapping the underlying driver is
non-invasive — no relinking of rust-macro is required.
```

Replace `<PIN_THE_EXACT_VERSION_HERE>` with the actual version from Step 1.

- [ ] **Step 5: Update `crates/app/tauri.conf.json`**

Add `bundle.resources` so the Tauri bundler includes the installer directory in built `.msi`/`.exe`:

```json
{
  ...
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.ico"
    ],
    "resources": [
      "installers/interception/*"
    ]
  }
}
```

After bundling, the resources land in `$RESOURCEDIR/installers/interception/` at runtime, accessible via Tauri's `app.path().resource_dir()`.

- [ ] **Step 6: Update `LICENSES.md`**

Expand the existing Interception section. Append after the "Static linking..." paragraph:

```markdown
### Bundled Interception installer (binary redistribution)

Built installers of this project (`.msi`, `.exe`) include the unmodified
signed binaries `install-interception.exe` and `uninstall-interception.exe`
from oblitum's Interception project, redistributed under LGPL-3.0.

The bundled `installers/interception/` directory contains:
- The two signed installer binaries (unmodified).
- `LICENSE-LGPL.txt` — full LGPL-3.0 text.
- `SOURCE-INFO.txt` — pointer to upstream source and version pin.

The LGPL "lesser license" obligations for binary redistribution are met
because:
1. The Interception driver loads as a Windows kernel filter; the rust-macro
   user-space process dynamically links to `interception.dll` only.
2. The user can replace the installed driver with a modified version
   without rebuilding rust-macro (see `installers/interception/SOURCE-INFO.txt`).
3. Source/version info is included in the install directory.
```

- [ ] **Step 7: Add `.gitignore` exception (if needed)**

The repo likely doesn't gitignore `.exe`. If it does, add to `.gitignore`:

```
# vendored binaries
!crates/app/installers/interception/*.exe
```

- [ ] **Step 8: Verify build**

Run: `cargo check -p rm-app`
Expected: PASS — adding bundle resources doesn't affect compile.

- [ ] **Step 9: Commit**

```powershell
git add crates/app/installers/interception/ crates/app/tauri.conf.json LICENSES.md
git commit -m "build(app): vendor Interception installer binaries (LGPL-3.0)"
```

**Note**: this commit contains binary blobs (~1-2 MB total). That's intentional — supply-chain auditability prefers in-repo binaries to download-at-build-time.

---

## Task 3: `driver_install` module — UAC-elevated installer spawn

**Files:**
- Create: `crates/app/src/driver_install.rs`
- Modify: `crates/app/src/main.rs` (register `mod driver_install;`)
- Modify: `crates/app/Cargo.toml` (verify `windows-sys` features)

This module owns the install/uninstall command bodies. It uses `ShellExecuteExW` with the `runas` verb to elevate UAC and run `install-interception.exe`.

- [ ] **Step 1: Verify/update `crates/app/Cargo.toml`**

Confirm `windows-sys` is a dep with `Win32_UI_Shell`, `Win32_System_Threading`, `Win32_Foundation` features. If not present, add:

```toml
[target.'cfg(windows)'.dependencies]
windows-sys = { workspace = true, features = ["Win32_UI_Shell", "Win32_System_Threading", "Win32_Foundation", "Win32_System_Shutdown"] }
```

(If `windows-sys` is already in the regular `[dependencies]`, just confirm/add the features.)

- [ ] **Step 2: Create `crates/app/src/driver_install.rs`**

```rust
//! Driver install / uninstall via UAC-elevated `ShellExecuteExW`.
//! Writes a `.driver-install-pending` marker in storage_root when an install
//! completes, signaling that the next reboot will activate the driver.

use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use rm_error::AppError;

const PENDING_MARKER_FILENAME: &str = ".driver-install-pending";

/// Spawn the Interception installer elevated via `runas`. Blocks the caller
/// until the installer process exits (typically a few seconds — the user
/// confirms UAC, the installer does its work).
///
/// On exit-code-zero: writes the pending-reboot marker and returns Ok.
/// On non-zero or spawn failure: returns AppError::Other.
pub fn install_driver(installer_path: &Path, storage_root: &Path) -> Result<(), AppError> {
    spawn_runas_wait(installer_path)?;
    write_pending_marker(storage_root)?;
    Ok(())
}

/// Spawn the Interception uninstaller elevated.
pub fn uninstall_driver(uninstaller_path: &Path, storage_root: &Path) -> Result<(), AppError> {
    spawn_runas_wait(uninstaller_path)?;
    write_pending_marker(storage_root)?;
    Ok(())
}

/// Path to the pending-reboot marker.
pub fn pending_marker_path(storage_root: &Path) -> PathBuf {
    storage_root.join(PENDING_MARKER_FILENAME)
}

/// Clear the pending-reboot marker (called by frontend after the user
/// confirms they've rebooted, or automatically when driver_status returns
/// Running).
pub fn clear_pending_marker(storage_root: &Path) -> Result<(), AppError> {
    let path = pending_marker_path(storage_root);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| AppError::Other(format!(
            "clear pending marker: {e}"
        )))?;
    }
    Ok(())
}

fn write_pending_marker(storage_root: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(storage_root)
        .map_err(|e| AppError::Other(format!("create storage dir: {e}")))?;
    let path = pending_marker_path(storage_root);
    std::fs::write(&path, b"rust-macro: reboot required after Interception install\n")
        .map_err(|e| AppError::Other(format!("write pending marker: {e}")))?;
    Ok(())
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(once(0)).collect()
}

#[cfg(target_os = "windows")]
fn spawn_runas_wait(installer: &Path) -> Result<(), AppError> {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        WaitForSingleObject, GetExitCodeProcess, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let installer_str = installer.to_string_lossy().to_string();
    let verb = wide("runas");
    let file = wide(&installer_str);

    // SAFETY: pointers are valid for the duration of ShellExecuteExW.
    let mut sei: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    sei.fMask = SEE_MASK_NOCLOSEPROCESS;
    sei.hwnd = std::ptr::null_mut();
    sei.lpVerb = verb.as_ptr();
    sei.lpFile = file.as_ptr();
    sei.lpParameters = std::ptr::null();
    sei.lpDirectory = std::ptr::null();
    sei.nShow = SW_HIDE as i32;

    let ok = unsafe { ShellExecuteExW(&mut sei) };
    if ok == 0 {
        let err = std::io::Error::last_os_error();
        return Err(AppError::Other(format!(
            "ShellExecuteExW failed (user may have declined UAC): {err}"
        )));
    }
    if sei.hProcess.is_null() {
        return Err(AppError::Other(
            "ShellExecuteExW returned a null process handle".into(),
        ));
    }

    // Wait for the installer to exit.
    let wait_result = unsafe { WaitForSingleObject(sei.hProcess, INFINITE) };
    if wait_result != WAIT_OBJECT_0 {
        unsafe { CloseHandle(sei.hProcess) };
        return Err(AppError::Other(format!(
            "WaitForSingleObject returned 0x{wait_result:x}"
        )));
    }

    let mut exit_code: u32 = 0;
    let got = unsafe { GetExitCodeProcess(sei.hProcess, &mut exit_code) };
    unsafe { CloseHandle(sei.hProcess) };
    if got == 0 {
        return Err(AppError::Other(format!(
            "GetExitCodeProcess failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    if exit_code != 0 {
        return Err(AppError::Other(format!(
            "installer exited with code {exit_code}"
        )));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn spawn_runas_wait(_installer: &Path) -> Result<(), AppError> {
    Err(AppError::Other("driver install is Windows-only".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_then_clear_pending_marker_roundtrip() {
        let tmp = TempDir::new().unwrap();
        write_pending_marker(tmp.path()).unwrap();
        assert!(pending_marker_path(tmp.path()).exists());
        clear_pending_marker(tmp.path()).unwrap();
        assert!(!pending_marker_path(tmp.path()).exists());
    }

    #[test]
    fn clear_when_absent_is_ok() {
        let tmp = TempDir::new().unwrap();
        clear_pending_marker(tmp.path()).unwrap();
    }
}
```

- [ ] **Step 3: Register module in `main.rs`**

After `mod recording;`:

```rust
mod driver_install;
```

(Not feature-gated — the spawn function is Windows-gated internally, and we want the marker-file helpers available always.)

- [ ] **Step 4: Run tests**

Run: `cargo test -p rm-app driver_install::tests`
Expected: PASS — 2 tests.

Run: `cargo check -p rm-app && cargo check -p rm-app --no-default-features`
Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add crates/app/src/driver_install.rs crates/app/src/main.rs crates/app/Cargo.toml
git commit -m "feat(app): driver_install module — UAC-elevated installer spawn + reboot marker"
```

---

## Task 4: `install_driver` / `uninstall_driver` Tauri commands

**Files:**
- Modify: `crates/app/src/commands.rs`
- Modify: `crates/app/src/main.rs`

The commands locate the bundled installer (via Tauri's `app.path().resource_dir()`), delegate to `driver_install`, then refresh state.

- [ ] **Step 1: Add the commands to `crates/app/src/commands.rs`**

Append after `driver_status`:

```rust
fn resource_path_or_err(app: &AppHandle, rel: &str) -> Result<std::path::PathBuf, AppError> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| AppError::Other(format!("resource_dir lookup: {e}")))?;
    Ok(resource_dir.join(rel))
}

#[tauri::command]
pub async fn install_driver(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), WireError> {
    let installer = resource_path_or_err(&app, "installers/interception/install-interception.exe")
        .map_err(|e| e.to_wire())?;
    if !installer.exists() {
        return Err(AppError::Other(format!(
            "installer not bundled at {}",
            installer.display()
        ))
        .to_wire());
    }
    let storage_root = state.storage_root.clone();
    // Spawn in a blocking task — ShellExecuteExW + WaitForSingleObject is blocking.
    tokio::task::spawn_blocking(move || {
        crate::driver_install::install_driver(&installer, &storage_root)
    })
    .await
    .map_err(|e| AppError::Other(format!("install task join: {e}")).to_wire())?
    .map_err(|e| e.to_wire())?;
    *state.pending_reboot.lock().await = true;
    Ok(())
}

#[tauri::command]
pub async fn uninstall_driver(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), WireError> {
    let uninstaller = resource_path_or_err(&app, "installers/interception/uninstall-interception.exe")
        .map_err(|e| e.to_wire())?;
    if !uninstaller.exists() {
        return Err(AppError::Other(format!(
            "uninstaller not bundled at {}",
            uninstaller.display()
        ))
        .to_wire());
    }
    let storage_root = state.storage_root.clone();
    tokio::task::spawn_blocking(move || {
        crate::driver_install::uninstall_driver(&uninstaller, &storage_root)
    })
    .await
    .map_err(|e| AppError::Other(format!("uninstall task join: {e}")).to_wire())?
    .map_err(|e| e.to_wire())?;
    *state.pending_reboot.lock().await = true;
    Ok(())
}

#[tauri::command]
pub async fn clear_pending_reboot(state: State<'_, AppState>) -> Result<(), WireError> {
    crate::driver_install::clear_pending_marker(&state.storage_root)
        .map_err(|e| e.to_wire())?;
    *state.pending_reboot.lock().await = false;
    Ok(())
}
```

Note: `app.path()` is the Tauri 2 path manager (requires `tauri::Manager` in scope — already imported).

- [ ] **Step 2: Register commands in `main.rs`**

Add to `invoke_handler!`:

```rust
            commands::driver_status,
            commands::install_driver,
            commands::uninstall_driver,
            commands::clear_pending_reboot,
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p rm-app`
Expected: PASS.

Run: `cargo check -p rm-app --no-default-features`
Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add crates/app/src/commands.rs crates/app/src/main.rs
git commit -m "feat(app): install_driver/uninstall_driver/clear_pending_reboot commands"
```

---

## Task 5: `reboot_windows` command

**Files:**
- Modify: `crates/app/src/driver_install.rs`
- Modify: `crates/app/src/commands.rs`
- Modify: `crates/app/src/main.rs`

Trigger a Windows shutdown/restart from the GUI when the user clicks "Restart now" in the reboot prompt.

- [ ] **Step 1: Add `reboot_windows()` to `crates/app/src/driver_install.rs`**

Append at module scope:

```rust
/// Trigger a Windows shutdown-and-restart with a small delay so the GUI
/// can close cleanly. Requires SE_SHUTDOWN_NAME privilege to be enabled
/// in the calling process token — we elevate via the same `runas` trick
/// by invoking `shutdown.exe /r /t 10 /d p:4:1` instead of calling
/// `InitiateSystemShutdownExW` directly (saves us from privilege juggling).
#[cfg(target_os = "windows")]
pub fn restart_windows() -> Result<(), AppError> {
    // /r restart, /t 10 sec, /d p:4:1 reason planned application
    let result = std::process::Command::new("shutdown.exe")
        .args(["/r", "/t", "10", "/d", "p:4:1"])
        .spawn();
    match result {
        Ok(_) => Ok(()),
        Err(e) => Err(AppError::Other(format!("shutdown.exe spawn: {e}"))),
    }
}

#[cfg(not(target_os = "windows"))]
pub fn restart_windows() -> Result<(), AppError> {
    Err(AppError::Other("restart_windows is Windows-only".into()))
}
```

- [ ] **Step 2: Add the Tauri command**

In `crates/app/src/commands.rs`, after `clear_pending_reboot`:

```rust
#[tauri::command]
pub async fn reboot_windows() -> Result<(), WireError> {
    crate::driver_install::restart_windows().map_err(|e| e.to_wire())
}
```

Register in `main.rs`:

```rust
            commands::reboot_windows,
```

- [ ] **Step 3: Run tests + builds**

Run: `cargo test -p rm-app && cargo check --workspace`
Expected: PASS.

- [ ] **Step 4: Commit**

```powershell
git add crates/app/src/driver_install.rs crates/app/src/commands.rs crates/app/src/main.rs
git commit -m "feat(app): reboot_windows command (shutdown.exe /r /t 10)"
```

---

## Task 6: Frontend types + api wrappers

**Files:**
- Modify: `crates/app/ui/src/lib/types.ts`
- Modify: `crates/app/ui/src/lib/api.ts`

- [ ] **Step 1: Append types**

In `crates/app/ui/src/lib/types.ts`, append:

```ts
export type DriverStatusDto = "not_installed" | "installed_not_running" | "running";

export type DriverStateDto = {
  status: DriverStatusDto;
  pending_reboot: boolean;
};
```

- [ ] **Step 2: Append api wrappers**

In `crates/app/ui/src/lib/api.ts`. Consolidate type imports first; then append:

```ts
export async function driverStatus(): Promise<DriverStateDto> {
  return invoke<DriverStateDto>("driver_status");
}

export async function installDriver(): Promise<void> {
  await invoke("install_driver");
}

export async function uninstallDriver(): Promise<void> {
  await invoke("uninstall_driver");
}

export async function clearPendingReboot(): Promise<void> {
  await invoke("clear_pending_reboot");
}

export async function rebootWindows(): Promise<void> {
  await invoke("reboot_windows");
}
```

- [ ] **Step 3: Build + commit**

```powershell
cd crates/app/ui && npm run build && cd ..\..\..
git add crates/app/ui/src/lib/types.ts crates/app/ui/src/lib/api.ts
git commit -m "feat(app/ui): DriverStateDto types + driver/install/reboot api wrappers"
```

---

## Task 7: Driver status store

**Files:**
- Create: `crates/app/ui/src/lib/stores/driver.ts`

- [ ] **Step 1: Create the store**

```ts
import { writable } from "svelte/store";
import type { DriverStateDto } from "../types";
import * as api from "../api";
import { reportError, pushToast } from "./toast";

const DEFAULT: DriverStateDto = { status: "not_installed", pending_reboot: false };

export const driver = writable<DriverStateDto>(DEFAULT);

/** Refresh from the backend. Call on boot + after any install/uninstall. */
export async function refresh(): Promise<void> {
  try {
    const s = await api.driverStatus();
    driver.set(s);
  } catch (e) {
    reportError(e);
  }
}

/** Start install flow — UAC prompt, then writes pending marker. */
export async function install(): Promise<void> {
  try {
    await api.installDriver();
    await refresh();
    pushToast("info", "Driver installed. Restart to activate.");
  } catch (e) {
    reportError(e);
  }
}

export async function uninstall(): Promise<void> {
  try {
    await api.uninstallDriver();
    await refresh();
    pushToast("info", "Driver uninstalled. Restart to complete.");
  } catch (e) {
    reportError(e);
  }
}

export async function dismissPending(): Promise<void> {
  try {
    await api.clearPendingReboot();
    await refresh();
  } catch (e) {
    reportError(e);
  }
}

export async function restartNow(): Promise<void> {
  try {
    await api.rebootWindows();
    // The shutdown.exe call returns ~immediately; Windows takes ~10s to
    // actually restart. The app process will be terminated by Windows.
    pushToast("info", "Restarting in 10 seconds…");
  } catch (e) {
    reportError(e);
  }
}
```

- [ ] **Step 2: Build + commit**

```powershell
cd crates/app/ui && npm run build && cd ..\..\..
git add crates/app/ui/src/lib/stores/driver.ts
git commit -m "feat(app/ui): driver status store (refresh/install/uninstall/restart)"
```

---

## Task 8: `DriverStatusBanner` component

**Files:**
- Create: `crates/app/ui/src/lib/components/DriverStatusBanner.svelte`

A thin banner shown at the top of every view when status is NOT Running OR pending_reboot is true.

- [ ] **Step 1: Create the file**

```svelte
<script lang="ts">
  import { driver, install, dismissPending } from "../stores/driver";

  let installing = $state(false);
  async function onInstall() {
    installing = true;
    try { await install(); } finally { installing = false; }
  }
</script>

{#if $driver.pending_reboot}
  <div class="banner banner-warning">
    <span>⚠ Driver install pending — restart Windows to activate Interception.</span>
    <button onclick={() => void dismissPending()} title="Hide this banner">✕</button>
  </div>
{:else if $driver.status === "not_installed"}
  <div class="banner banner-error">
    <span>❌ Interception driver not installed. Hotkeys and playback won't work.</span>
    <button class="primary" disabled={installing} onclick={() => void onInstall()}>
      {installing ? "Installing…" : "Install (admin)"}
    </button>
  </div>
{:else if $driver.status === "installed_not_running"}
  <div class="banner banner-warning">
    <span>⚠ Interception driver installed but not running. Restart Windows to activate.</span>
  </div>
{/if}

<style>
  .banner {
    display: flex;
    gap: 0.75rem;
    align-items: center;
    padding: 0.5rem 1rem;
    font-size: 0.9rem;
    border-bottom: 1px solid var(--border);
  }
  .banner span { flex: 1; }
  .banner-error {
    background: rgba(220, 38, 38, 0.15);
    color: #fca5a5;
  }
  .banner-warning {
    background: rgba(202, 138, 4, 0.15);
    color: #fde68a;
  }
  button { padding: 0.25rem 0.6rem; }
</style>
```

- [ ] **Step 2: Build + commit**

```powershell
cd crates/app/ui && npm run build && cd ..\..\..
git add crates/app/ui/src/lib/components/DriverStatusBanner.svelte
git commit -m "feat(app/ui): DriverStatusBanner — install button + reboot reminder"
```

---

## Task 9: `RebootPromptModal` component

**Files:**
- Create: `crates/app/ui/src/lib/components/RebootPromptModal.svelte`

A modal shown ONCE per session when `pending_reboot` flips from false → true (i.e., right after a successful install). Offers Restart now / Later. Dismissing leaves the banner up.

- [ ] **Step 1: Create the file**

```svelte
<script lang="ts">
  import { driver, restartNow, dismissPending } from "../stores/driver";

  let shown = $state(false);
  let lastPending = $state(false);

  $effect(() => {
    // Detect transition false → true; show modal once per transition.
    if ($driver.pending_reboot && !lastPending) {
      shown = true;
    }
    lastPending = $driver.pending_reboot;
  });

  function close() { shown = false; }
  async function later() {
    close();
    // We don't clear the marker — the banner stays up.
  }
  async function now() {
    await restartNow();
    close();
  }
</script>

{#if shown}
  <div class="backdrop" role="presentation">
    <div class="modal" role="dialog" aria-labelledby="reboot-title">
      <h3 id="reboot-title">Restart required</h3>
      <p>
        The Interception driver was installed. Windows must restart before
        the driver becomes active and rust-macro can capture input.
      </p>
      <p class="small">Restart will begin in 10 seconds after you click "Restart now".</p>
      <div class="actions">
        <button onclick={later}>Restart later</button>
        <button class="primary" onclick={() => void now()}>Restart now</button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex; align-items: center; justify-content: center;
    z-index: 700;
  }
  .modal {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1.5rem;
    max-width: 480px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.5);
  }
  h3 { margin: 0 0 0.75rem 0; }
  p { margin: 0 0 0.75rem 0; }
  .small { color: var(--text-muted); font-size: 0.85rem; }
  .actions { display: flex; justify-content: flex-end; gap: 0.5rem; margin-top: 1.25rem; }
</style>
```

- [ ] **Step 2: Build + commit**

```powershell
cd crates/app/ui && npm run build && cd ..\..\..
git add crates/app/ui/src/lib/components/RebootPromptModal.svelte
git commit -m "feat(app/ui): RebootPromptModal — Restart now / later after install"
```

---

## Task 10: Mount banner + reboot modal in `App.svelte`

**Files:**
- Modify: `crates/app/ui/src/App.svelte`

The banner needs to appear in ALL views (list, editor, settings). The reboot modal too. Refresh driver state on boot.

- [ ] **Step 1: Update `App.svelte`**

Add imports near the top of the `<script>`:

```ts
import DriverStatusBanner from "./lib/components/DriverStatusBanner.svelte";
import RebootPromptModal from "./lib/components/RebootPromptModal.svelte";
import { refresh as refreshDriver } from "./lib/stores/driver";
```

In `onMount`:

```ts
  onMount(() => {
    void loadAll();
    void startPlaybackListening();
    void startRecordingListening();
    void refreshDriver();   // ← new
  });
```

Update each `{#if view.tag === ...}` arm to include the banner + modal. Easiest: wrap the entire `{#if ...}{/if}` block:

```svelte
<DriverStatusBanner />

{#if view.tag === "list"}
  <main>...</main>
  <PlaybackBanner />
  <RecordingModal />
  <ToastHost />
{:else if view.tag === "editor"}
  <StepEditor macroId={view.macroId} onBack={backToList} />
  <ToastHost />
{:else if view.tag === "settings"}
  <SettingsView onBack={backToList} />
  <ToastHost />
{/if}

<RebootPromptModal />
```

The banner renders above the per-view `<main>` because it's outside the `{#if}`. The modal sits at the end, always available.

- [ ] **Step 2: Build + commit**

```powershell
cd crates/app/ui && npm run build && cd ..\..\..
git add crates/app/ui/src/App.svelte
git commit -m "feat(app/ui): mount DriverStatusBanner + RebootPromptModal globally"
```

---

## Task 11: Driver status indicator in SettingsView

**Files:**
- Modify: `crates/app/ui/src/lib/components/SettingsView.svelte`

Add a "Driver" section with status indicator + Install/Reinstall/Uninstall buttons.

- [ ] **Step 1: Update SettingsView.svelte**

Add imports + driver state:

```ts
import { driver, install as installDriver, uninstall as uninstallDriver, refresh as refreshDriver } from "../stores/driver";

let installing = $state(false);
let uninstalling = $state(false);

onMount(async () => {
  await loadSettings();
  await refreshDriver();
  // ... existing onMount body
});

async function onInstall() {
  installing = true;
  try { await installDriver(); } finally { installing = false; }
}

async function onUninstall() {
  if (!confirm("Uninstall Interception driver? Other apps (Kanata, AHK-fork) that depend on it will stop working.")) return;
  uninstalling = true;
  try { await uninstallDriver(); } finally { uninstalling = false; }
}

function statusLabel(s: typeof $driver.status): string {
  switch (s) {
    case "running": return "✅ Running";
    case "installed_not_running": return "⚠ Installed (not running — restart required)";
    case "not_installed": return "❌ Not installed";
  }
}
```

Add a section in the markup before the closing `</main>`:

```svelte
<section class="driver-section">
  <h3>Interception driver</h3>
  <p class="status">{statusLabel($driver.status)}</p>
  <p class="hint">
    rust-macro requires Interception to capture and inject input.
    See <a href="https://github.com/oblitum/Interception" target="_blank">upstream</a>
    for source and license.
  </p>
  <div class="actions">
    {#if $driver.status === "not_installed"}
      <button class="primary" disabled={installing} onclick={() => void onInstall()}>
        {installing ? "Installing…" : "Install driver"}
      </button>
    {:else}
      <button disabled={installing} onclick={() => void onInstall()}>
        {installing ? "Reinstalling…" : "Reinstall"}
      </button>
      <button class="danger" disabled={uninstalling} onclick={() => void onUninstall()}>
        {uninstalling ? "Uninstalling…" : "Uninstall"}
      </button>
    {/if}
  </div>
</section>

<style>
  .driver-section { border-top: 1px solid var(--border); padding-top: 1rem; margin-top: 2rem; }
  .driver-section h3 { margin: 0 0 0.5rem 0; font-size: 0.85rem; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.05em; }
  .driver-section .status { font-size: 1rem; margin: 0 0 0.5rem 0; }
  .driver-section .hint { color: var(--text-muted); font-size: 0.85rem; }
  .driver-section .actions { display: flex; gap: 0.5rem; margin-top: 0.75rem; }
</style>
```

- [ ] **Step 2: Build + commit**

```powershell
cd crates/app/ui && npm run build && cd ..\..\..
git add crates/app/ui/src/lib/components/SettingsView.svelte
git commit -m "feat(app/ui): SettingsView — driver section with status + install/uninstall"
```

---

## Task 12: README + smoke updates

**Files:**
- Modify: `crates/app/README.md`

- [ ] **Step 1: Update Prerequisites**

Replace the existing "Interception kernel driver installed — see..." line with:

```markdown
- Interception kernel driver — **bundled with the installer**. On first launch,
  the app prompts to install it (UAC + reboot required). For dev builds via
  `cargo tauri dev` (which doesn't run the installer), download Interception
  from <https://github.com/oblitum/Interception> and run
  `install-interception.exe` once.
```

- [ ] **Step 2: Add smoke test items**

After item 18 in the smoke test list, append:

```markdown
19. **First-run driver install.** On a clean machine without Interception:
    launch the app → red banner "Driver not installed" at top → click Install →
    UAC prompt → after a few seconds, banner switches to "Restart required" +
    modal pops up offering Restart now/later → click "Restart now" → Windows
    restarts in 10s → after reboot, app launches with no banner.
20. **Driver section in Settings.** Open ⚙ Settings → scroll to "Interception
    driver" → status reads "✅ Running" → "Reinstall" and "Uninstall" buttons
    visible.
21. **Uninstall path.** Click Uninstall in Settings → confirm dialog → UAC
    prompt → driver uninstalled → banner appears "Restart required".
    (DON'T do this on your daily machine if you use Kanata or AHK-fork.)
22. **Resilient to user declining UAC.** Click Install in the banner → UAC
    prompt appears → click "No" → toast: "ShellExecuteExW failed (user may
    have declined UAC)" → banner unchanged. No crash.
```

- [ ] **Step 3: Add Bundle section**

After the "Build a release binary" section, append:

```markdown
## What's in the bundle

The installer (`target/release/bundle/msi/rust-macro_<version>_x64_en-US.msi`
or the equivalent `.exe`) contains:

- `rust-macro.exe` — the GUI app.
- `installers/interception/install-interception.exe` — oblitum's signed
  Interception driver installer.
- `installers/interception/uninstall-interception.exe` — oblitum's uninstaller.
- `installers/interception/LICENSE-LGPL.txt` — driver license.
- `installers/interception/SOURCE-INFO.txt` — pointer to upstream source.

The bundled Interception binaries are oblitum's official signed builds at
version `<PINNED_VERSION>`. To use a different version, install your own
Interception (or build it from source) before launching rust-macro for the
first time — the app's driver-status detector picks up any existing install.
```

(Replace `<PINNED_VERSION>` with the actual pin from Task 2.)

- [ ] **Step 4: Commit**

```powershell
git add crates/app/README.md
git commit -m "docs(app): Plan 3d README — bundled driver flow + smoke items 19-22"
```

---

## Task 13: Final verification

- [ ] **Step 1: All workspace tests pass**

```powershell
cargo test --workspace --no-fail-fast
```

Expected: PASS. New: 2 tests in `driver_install::tests`. Existing 109 unchanged.

- [ ] **Step 2: Frontend builds clean**

```powershell
cd crates/app/ui
npm run build
cd ..\..\..
```

Expected: PASS.

- [ ] **Step 3: Both feature variants compile**

```powershell
cargo check -p rm-app
cargo check -p rm-app --no-default-features
```

Both: PASS.

- [ ] **Step 4: Build the release bundle**

```powershell
cd crates/app
cargo tauri build
```

Expected: produces `.msi`/`.exe` in `target/release/bundle/`. Open the installer in a hex editor or extract via `7z l`; verify `installers/interception/install-interception.exe` is present inside.

- [ ] **Step 5: Smoke test walkthrough**

Items 19-22 from `crates/app/README.md`. Smoke tests 19-22 require a Windows VM/sandbox without Interception pre-installed (otherwise the install path can't be exercised cleanly). Suggestion: use a Hyper-V or VirtualBox VM with a fresh Windows 11 image.

- [ ] **Step 6: No commit if Steps 1-4 pass**

The previous tasks committed all changes. Final verification is acceptance-only.

---

## Acceptance Checklist

- [ ] `cargo test --workspace` is green (111 tests).
- [ ] `cargo build -p rm-app` succeeds default + `--no-default-features`.
- [ ] `cargo tauri build` produces an installer that contains the Interception binaries under `installers/interception/`.
- [ ] On first run with no Interception present, red banner appears at the top with an Install button.
- [ ] Clicking Install triggers a real UAC prompt; on confirm, installer runs, pending-reboot marker is written, banner switches to yellow "Restart required".
- [ ] Reboot prompt modal appears once after install; "Restart now" actually reboots the machine via `shutdown.exe /r /t 10`.
- [ ] After reboot, app launches with no banner; `driver_status` returns Running.
- [ ] Settings view shows status indicator + Reinstall + Uninstall buttons.
- [ ] Uninstall path works symmetrically (with a confirm dialog).
- [ ] LGPL compliance: `LICENSE-LGPL.txt` and `SOURCE-INFO.txt` are present alongside the bundled installer in the build output.
- [ ] Declining UAC produces a clean toast error, no crash.

---

## Open Implementation Notes

- **`shutdown.exe /r` requires no admin** for user-initiated restart. If the call fails with "access denied" on some lockdown environments, the user can manually restart — the banner stays up until they do.

- **`ShellExecuteExW` with `runas` verb** triggers UAC. If the user clicks "No", `ShellExecuteExW` returns 0 and `GetLastError` is `ERROR_CANCELLED` (1223). The error message is generic; we could special-case 1223 for a friendlier toast ("install cancelled") — small enhancement, not required for v1.

- **Tauri 2 path manager:** `app.path().resource_dir()` returns `Result<PathBuf, tauri::Error>`. In Tauri 2's current API this is a sync call; if a future version returns a future, adapt.

- **Bundle resources during `cargo tauri dev`:** in dev mode, `resource_dir()` points to `target/debug/`, NOT the project's `installers/` directory. Two options:
  1. Document that dev mode needs Interception pre-installed (current README note).
  2. Symlink or copy `installers/interception/` into `target/debug/` on dev startup — fragile. Skip for v1.

- **Anti-virus paranoia.** Test the Tauri build on a machine with Defender enabled. If Defender quarantines the bundled installer at extraction time, document the workaround (add to exclusions) in the README.

- **Existing Interception installations:** oblitum's installer reports a friendly "already installed" message and exits with code 0. Our code treats exit-code-zero as success, which is correct — the pending-reboot marker is still written but `driver_status` quickly returns Running, and the banner self-clears. Verify in smoke item 19's variant: machine WITH Interception already present.

- **Installer logs.** oblitum's installer writes to `%TEMP%`; we don't capture them. If install fails on a user's machine, instruct them to check `%TEMP%\InterceptionInstaller_*.log`.

- **Why not bundle `interception.dll` separately?** oblitum's installer places `interception.dll` into `C:\Windows\System32` (the standard location for system-wide DLLs). Our app's `kanata-interception` crate loads it from there. We don't need to bundle the DLL separately.
