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

// System-wide idle time on macOS: seconds since the last HID event, from the
// combined session event source. Needs no special permission.
#[cfg(target_os = "macos")]
pub fn idle_ms() -> u64 {
    // CGEventSourceStateID::kCGEventSourceStateCombinedSessionState = 0.
    const COMBINED_SESSION_STATE: i32 = 0;
    // CGEventType::kCGAnyInputEventType = ~0 (matches any input event).
    const ANY_INPUT_EVENT: u32 = !0u32;

    unsafe {
        let secs = CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION_STATE, ANY_INPUT_EVENT);
        (secs * 1000.0) as u64
    }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn idle_ms() -> u64 {
    0
}
