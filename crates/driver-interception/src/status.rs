//! `detect_status()` — probe whether the Interception kernel driver is present
//! and running. Detection works in two layers: (1) try to open an Interception
//! context (definitive when it succeeds); (2) on failure, query the Windows
//! Service Control Manager for the two driver services to distinguish
//! NotInstalled from InstalledNotRunning.
//!
//! NOTE: The exact service names ("keyboard" and "mouse") must be verified
//! against a live Interception install before merge. If they differ, adjust
//! `INTERCEPTION_SERVICE_NAMES` and the test fixtures.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriverStatus {
    /// Interception services are not present on the system.
    NotInstalled,
    /// Services exist but are not running (user-disabled or pending reboot).
    InstalledNotRunning,
    /// Services running and a context can be opened.
    Running,
}

/// The two driver services Oblitum's installer registers. **Verify before merge.**
const INTERCEPTION_SERVICE_NAMES: &[&str] = &["keyboard", "mouse"];

/// Outcome of `ServiceQuery::query_all`. Distilled to three cases that map
/// directly to `DriverStatus` when the context-open path fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    AllRunning,
    AllPresentSomeStopped,
    AnyMissing,
}

/// Abstracts the Windows SCM. Production impl talks to the real SCM via
/// `windows-sys`; tests inject a fake.
pub trait ServiceQuery {
    fn query_all(&self, names: &[&str]) -> ServiceState;
}

/// Probe Interception. `open_ctx` is a closure that attempts to open a context
/// and returns true on success — injected so this function is testable without
/// the real driver.
pub fn detect_status_with<F, S>(open_ctx: F, services: &S) -> DriverStatus
where
    F: FnOnce() -> bool,
    S: ServiceQuery,
{
    if open_ctx() {
        return DriverStatus::Running;
    }
    match services.query_all(INTERCEPTION_SERVICE_NAMES) {
        ServiceState::AllRunning => DriverStatus::Running,
        ServiceState::AllPresentSomeStopped => DriverStatus::InstalledNotRunning,
        ServiceState::AnyMissing => DriverStatus::NotInstalled,
    }
}

/// Public entry point: live SCM + live Interception context open.
pub fn detect_status() -> DriverStatus {
    detect_status_with(try_open_real_context, &Scm)
}

fn try_open_real_context() -> bool {
    // `Interception::new()` returns Option<Interception>. We additionally wrap
    // in catch_unwind defensively — the FFI shouldn't panic, but unknown C
    // boundary.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        kanata_interception::Interception::new().is_some()
    }))
    .unwrap_or(false)
}

/// Real Windows SCM-backed implementation.
struct Scm;

impl ServiceQuery for Scm {
    fn query_all(&self, names: &[&str]) -> ServiceState {
        scm::query(names)
    }
}

mod scm {
    use super::ServiceState;
    use std::ffi::OsStr;
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::ERROR_SERVICE_DOES_NOT_EXIST;
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus,
        SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_STATUS,
    };

    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(once(0)).collect()
    }

    pub fn query(names: &[&str]) -> ServiceState {
        // SAFETY: Win32 API calls — pointers are valid for the lifetime of
        // each call, handles are explicitly closed.
        unsafe {
            let scm = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
            if scm.is_null() {
                tracing::debug!(
                    error = std::io::Error::last_os_error().to_string(),
                    "OpenSCManagerW failed; assuming services absent"
                );
                return ServiceState::AnyMissing;
            }
            let mut all_running = true;
            for name in names {
                let w = wide(name);
                let svc = OpenServiceW(scm, w.as_ptr(), SERVICE_QUERY_STATUS);
                if svc.is_null() {
                    let err = std::io::Error::last_os_error();
                    let code = err.raw_os_error().unwrap_or(0) as u32;
                    if code == ERROR_SERVICE_DOES_NOT_EXIST {
                        CloseServiceHandle(scm);
                        return ServiceState::AnyMissing;
                    }
                    tracing::debug!(service = name, ?err, "OpenServiceW failed; treating as missing");
                    CloseServiceHandle(scm);
                    return ServiceState::AnyMissing;
                }
                let mut st: SERVICE_STATUS = std::mem::zeroed();
                let ok = QueryServiceStatus(svc, &mut st);
                CloseServiceHandle(svc);
                if ok == 0 || st.dwCurrentState != SERVICE_RUNNING {
                    all_running = false;
                }
            }
            CloseServiceHandle(scm);
            if all_running {
                ServiceState::AllRunning
            } else {
                ServiceState::AllPresentSomeStopped
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeServices(ServiceState);
    impl ServiceQuery for FakeServices {
        fn query_all(&self, _names: &[&str]) -> ServiceState {
            self.0
        }
    }

    #[test]
    fn open_succeeds_returns_running_regardless_of_services() {
        let s = detect_status_with(|| true, &FakeServices(ServiceState::AnyMissing));
        assert_eq!(s, DriverStatus::Running);
    }

    #[test]
    fn open_fails_services_missing_returns_not_installed() {
        let s = detect_status_with(|| false, &FakeServices(ServiceState::AnyMissing));
        assert_eq!(s, DriverStatus::NotInstalled);
    }

    #[test]
    fn open_fails_services_present_but_stopped_returns_installed_not_running() {
        let s = detect_status_with(|| false, &FakeServices(ServiceState::AllPresentSomeStopped));
        assert_eq!(s, DriverStatus::InstalledNotRunning);
    }

    #[test]
    fn open_fails_but_services_all_running_returns_running() {
        // Race window: SCM reports running but context-open lost a race.
        // We trust SCM in that case.
        let s = detect_status_with(|| false, &FakeServices(ServiceState::AllRunning));
        assert_eq!(s, DriverStatus::Running);
    }
}
