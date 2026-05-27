//! Driver install / uninstall via UAC-elevated `ShellExecuteExW`.
//! Writes a `.driver-install-pending` marker in storage_root when an
//! install/uninstall completes, signaling that the next reboot will
//! activate the change.

use std::ffi::OsStr;
use std::iter::once;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use rm_error::AppError;

const PENDING_MARKER_FILENAME: &str = ".driver-install-pending";

/// Spawn the Interception installer with `/install` elevated. Blocks until
/// the installer process exits. On exit-code-zero: writes the
/// pending-reboot marker and returns Ok.
pub fn install_driver(installer_path: &Path, storage_root: &Path) -> Result<(), AppError> {
    spawn_runas_wait(installer_path, "/install")?;
    write_pending_marker(storage_root)?;
    Ok(())
}

/// Spawn the same installer with `/uninstall` elevated.
pub fn uninstall_driver(installer_path: &Path, storage_root: &Path) -> Result<(), AppError> {
    spawn_runas_wait(installer_path, "/uninstall")?;
    write_pending_marker(storage_root)?;
    Ok(())
}

/// Path to the pending-reboot marker.
pub fn pending_marker_path(storage_root: &Path) -> PathBuf {
    storage_root.join(PENDING_MARKER_FILENAME)
}

/// Clear the pending-reboot marker. Idempotent: missing file → Ok.
pub fn clear_pending_marker(storage_root: &Path) -> Result<(), AppError> {
    let path = pending_marker_path(storage_root);
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| AppError::Other(format!("clear pending marker: {e}")))?;
    }
    Ok(())
}

fn write_pending_marker(storage_root: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(storage_root)
        .map_err(|e| AppError::Other(format!("create storage dir: {e}")))?;
    let path = pending_marker_path(storage_root);
    std::fs::write(&path, b"rust-macro: reboot required after Interception install/uninstall\n")
        .map_err(|e| AppError::Other(format!("write pending marker: {e}")))?;
    Ok(())
}

fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(once(0)).collect()
}

#[cfg(target_os = "windows")]
fn spawn_runas_wait(installer: &Path, args: &str) -> Result<(), AppError> {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, WaitForSingleObject, INFINITE,
    };
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let installer_str = installer.to_string_lossy().to_string();
    let verb = wide("runas");
    let file = wide(&installer_str);
    let params = wide(args);

    // SAFETY: pointers are valid for the duration of ShellExecuteExW.
    let mut sei: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    sei.fMask = SEE_MASK_NOCLOSEPROCESS;
    sei.hwnd = std::ptr::null_mut();
    sei.lpVerb = verb.as_ptr();
    sei.lpFile = file.as_ptr();
    sei.lpParameters = params.as_ptr();
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
fn spawn_runas_wait(_installer: &Path, _args: &str) -> Result<(), AppError> {
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
