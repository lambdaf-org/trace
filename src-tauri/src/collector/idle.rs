//! System-wide idle time via GetLastInputInfo. Stores nothing about the input.

#[cfg(windows)]
pub fn idle_ms() -> u64 {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    unsafe {
        let mut lii = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if GetLastInputInfo(&mut lii).as_bool() {
            // wrapping_sub handles the 32-bit tick-count rollover (~49.7 days).
            GetTickCount().wrapping_sub(lii.dwTime) as u64
        } else {
            0
        }
    }
}

#[cfg(not(windows))]
pub fn idle_ms() -> u64 {
    0
}
