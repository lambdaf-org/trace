//! Foreground window -> (title, process name). Unprivileged Win32 only.

#[cfg(windows)]
pub fn foreground_snapshot() -> Option<(String, String)> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{HWND, MAX_PATH};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }

        // Title (may be empty; protected/elevated windows can refuse).
        let mut buf = [0u16; 512];
        let n = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..n as usize]);

        // Owning process id -> image path -> file name.
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }

        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut path = [0u16; MAX_PATH as usize];
        let mut len = path.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            PWSTR(path.as_mut_ptr()),
            &mut len,
        )
        .is_ok();
        // handle is closed automatically when dropped in recent windows-rs; if your
        // version returns a raw HANDLE, CloseHandle(handle) here.
        if !ok {
            // Higher-integrity process: we got a pid but not the name. Mark protected.
            return Some((title, "<protected>".to_string()));
        }

        let full = String::from_utf16_lossy(&path[..len as usize]);
        let process = full.rsplit('\\').next().unwrap_or(&full).to_string();
        Some((title, process))
    }
}

#[cfg(not(windows))]
pub fn foreground_snapshot() -> Option<(String, String)> {
    None
}
