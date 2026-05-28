//! Diagnostic tool: print every WM_INPUT mouse event the OS delivers to a
//! message-only window. Used to diagnose whether Interception-injected mouse
//! events propagate to Raw Input subscribers (games like Resident Evil's RE
//! Engine use Raw Input for in-game mouse-look).
//!
//! Usage:
//!   cargo run -p rm-rawinput-probe
//!
//! At startup the tool enumerates registered raw input mouse devices and
//! prints their hDevice handle + device name so events can be correlated back
//! to a physical device. Then it logs each WM_INPUT mouse event until Ctrl+C.

#[cfg(not(windows))]
fn main() {
    eprintln!("rawinput-probe is Windows-only.");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    probe::run();
}

#[cfg(windows)]
mod probe {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::sync::OnceLock;
    use std::time::Instant;

    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::{
        GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList,
        RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
        RAWINPUTDEVICELIST, RAWINPUTHEADER, RIDEV_INPUTSINK, RIDI_DEVICENAME,
        RID_INPUT, RIM_TYPEMOUSE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW,
        RegisterClassExW, TranslateMessage, HWND_MESSAGE, MSG, WM_INPUT,
        WNDCLASSEXW,
    };

    static START: OnceLock<Instant> = OnceLock::new();

    fn elapsed_ms() -> u128 {
        START.get_or_init(Instant::now).elapsed().as_millis()
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_INPUT {
            handle_raw_input(lparam as HRAWINPUT);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    unsafe fn handle_raw_input(hri: HRAWINPUT) {
        let header_size = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        let mut size: u32 = 0;
        if GetRawInputData(hri, RID_INPUT, std::ptr::null_mut(), &mut size, header_size)
            == u32::MAX
        {
            return;
        }
        let mut buf: Vec<u8> = vec![0u8; size as usize];
        let written = GetRawInputData(
            hri,
            RID_INPUT,
            buf.as_mut_ptr() as *mut _,
            &mut size,
            header_size,
        );
        if written == u32::MAX {
            return;
        }
        let raw = &*(buf.as_ptr() as *const RAWINPUT);
        if raw.header.dwType != RIM_TYPEMOUSE {
            return;
        }
        let mouse = raw.data.mouse;
        let btn_flags = mouse.Anonymous.Anonymous.usButtonFlags;
        println!(
            "[{:>7}ms] dev=0x{:016x} dx={:>5} dy={:>5} flags=0x{:04x} btn=0x{:04x} extra=0x{:08x}",
            elapsed_ms(),
            raw.header.hDevice as usize,
            mouse.lLastX,
            mouse.lLastY,
            mouse.usFlags,
            btn_flags,
            mouse.ulExtraInformation,
        );
    }

    unsafe fn enumerate_devices() {
        println!("=== Raw Input mouse devices ===");
        let entry_size = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
        let mut count: u32 = 0;
        // First call with null buffer populates count, returns 0 on success.
        GetRawInputDeviceList(std::ptr::null_mut(), &mut count, entry_size);
        if count == 0 {
            println!("(no devices reported)");
            println!();
            return;
        }
        let mut devices: Vec<RAWINPUTDEVICELIST> =
            vec![std::mem::zeroed(); count as usize];
        let got = GetRawInputDeviceList(devices.as_mut_ptr(), &mut count, entry_size);
        if got == u32::MAX {
            println!("(GetRawInputDeviceList failed)");
            println!();
            return;
        }
        for d in &devices[..got as usize] {
            if d.dwType != RIM_TYPEMOUSE {
                continue;
            }
            let mut name_len: u32 = 0;
            GetRawInputDeviceInfoW(
                d.hDevice,
                RIDI_DEVICENAME,
                std::ptr::null_mut(),
                &mut name_len,
            );
            let mut name_buf: Vec<u16> = vec![0u16; name_len as usize];
            let n = GetRawInputDeviceInfoW(
                d.hDevice,
                RIDI_DEVICENAME,
                name_buf.as_mut_ptr() as *mut _,
                &mut name_len,
            );
            let name = if n != u32::MAX {
                let len = name_buf
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(name_buf.len());
                OsString::from_wide(&name_buf[..len])
                    .to_string_lossy()
                    .into_owned()
            } else {
                "(name lookup failed)".to_string()
            };
            println!("  dev=0x{:016x}  {}", d.hDevice as usize, name);
        }
        println!();
    }

    pub fn run() {
        unsafe {
            START.get_or_init(Instant::now);
            enumerate_devices();

            let h_instance = GetModuleHandleW(std::ptr::null());
            let class_name = wide("RmRawInputProbeClass");

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: h_instance as _,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class_name.as_ptr(),
                hIconSm: std::ptr::null_mut(),
            };
            if RegisterClassExW(&wc) == 0 {
                eprintln!("RegisterClassExW failed");
                std::process::exit(1);
            }

            let window_name = wide("RmRawInputProbe");
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                window_name.as_ptr(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                std::ptr::null_mut(),
                h_instance as _,
                std::ptr::null_mut(),
            );
            if hwnd.is_null() {
                eprintln!("CreateWindowExW failed");
                std::process::exit(1);
            }

            let rid = RAWINPUTDEVICE {
                usUsagePage: 0x01, // Generic Desktop
                usUsage: 0x02,     // Mouse
                dwFlags: RIDEV_INPUTSINK,
                hwndTarget: hwnd,
            };
            if RegisterRawInputDevices(
                &rid,
                1,
                std::mem::size_of::<RAWINPUTDEVICE>() as u32,
            ) == 0
            {
                eprintln!("RegisterRawInputDevices failed");
                std::process::exit(1);
            }

            println!("=== Events (Ctrl+C to quit) ===");
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}
