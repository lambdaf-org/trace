//! Foreground window -> (title, process name). Unprivileged Win32 only.

#[cfg(windows)]
pub fn foreground_snapshot() -> Option<(String, String)> {
    use windows::Win32::Foundation::{HWND, MAX_PATH};
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };
    use windows::core::PWSTR;

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

// macOS foreground snapshot. The app's localized name comes from NSWorkspace
// (no permission required) and serves as both the display name and the process
// identity used for browser detection. The window title comes from the
// Accessibility API and is only available once the user grants Accessibility
// permission — until then we return an empty title and still record the app.
#[cfg(target_os = "macos")]
pub fn foreground_snapshot() -> Option<(String, String)> {
    let (pid, app_name) = frontmost_app()?;
    let title = window_title(pid).unwrap_or_default();
    Some((title, app_name))
}

#[cfg(target_os = "macos")]
fn frontmost_app() -> Option<(i32, String)> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::ffi::CStr;
    use std::os::raw::c_char;

    unsafe {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return None;
        }
        // frontmostApplication returns an autoreleased NSRunningApplication.
        let app: Option<Retained<AnyObject>> = msg_send![workspace, frontmostApplication];
        let app = app?;

        let pid: i32 = msg_send![&*app, processIdentifier];

        let ns_name: *mut AnyObject = msg_send![&*app, localizedName];
        if ns_name.is_null() {
            return None;
        }
        let utf8: *const c_char = msg_send![ns_name, UTF8String];
        if utf8.is_null() {
            return None;
        }
        let name = CStr::from_ptr(utf8).to_string_lossy().into_owned();
        if name.is_empty() {
            return None;
        }
        Some((pid, name))
    }
}

// Accessibility API element handle. Treated as an opaque CFType pointer.
#[cfg(target_os = "macos")]
type AXUIElementRef = *const std::ffi::c_void;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: core_foundation::string::CFStringRef,
        value: *mut core_foundation::base::CFTypeRef,
    ) -> i32;
}

#[cfg(target_os = "macos")]
fn window_title(pid: i32) -> Option<String> {
    use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
    use core_foundation::string::{CFString, CFStringRef};

    // kAXErrorSuccess
    const AX_SUCCESS: i32 = 0;

    unsafe {
        // Without Accessibility permission every Copy call fails; skip the work.
        if !AXIsProcessTrusted() {
            return None;
        }

        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return None;
        }

        // Focused window of the application.
        let focused_attr = CFString::new("AXFocusedWindow");
        let mut window: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(
            app,
            focused_attr.as_concrete_TypeRef(),
            &mut window,
        );
        CFRelease(app as CFTypeRef);
        if err != AX_SUCCESS || window.is_null() {
            return None;
        }

        // Title of that window.
        let title_attr = CFString::new("AXTitle");
        let mut title_ref: CFTypeRef = std::ptr::null();
        let err = AXUIElementCopyAttributeValue(
            window as AXUIElementRef,
            title_attr.as_concrete_TypeRef(),
            &mut title_ref,
        );
        CFRelease(window);
        if err != AX_SUCCESS || title_ref.is_null() {
            return None;
        }

        // Copy attribute returns +1; wrap under the create rule so it is released.
        let title = CFString::wrap_under_create_rule(title_ref as CFStringRef).to_string();
        if title.is_empty() {
            None
        } else {
            Some(title)
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn foreground_snapshot() -> Option<(String, String)> {
    None
}
