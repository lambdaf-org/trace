//! Active browser URL via UI Automation — local, no extension, no packet
//! inspection. We read the address-bar text, then immediately strip it: the
//! query string and fragment are dropped before anything is stored, and by
//! default only the host (domain) is kept.
//!
//! WARNING: the UI Automation block is the most version-sensitive code in the
//! project and cannot be verified without a Windows build. If it fails to
//! compile against your `windows` crate, replace the body of `read_address_bar`
//! with `None` to ship without URL capture, then re-enable once the symbols are
//! reconciled. `strip` below is pure Rust and always correct.

/// Reduce a raw address-bar string to what we are willing to store.
/// detail = "host" keeps only the domain; "host_path" keeps host + path.
/// The query string and fragment are ALWAYS removed.
pub fn strip(raw: &str, detail: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    // Drop scheme.
    let no_scheme = raw
        .splitn(2, "://")
        .last()
        .unwrap_or(raw)
        .trim_start_matches("www.");
    // Cut at the first query/fragment marker.
    let no_query = no_scheme.split(['?', '#']).next().unwrap_or(no_scheme);
    // host is everything before the first slash.
    let host = no_query.split('/').next().unwrap_or(no_query);
    // Reject things that aren't host-like (new tab page, search text, etc.).
    if host.is_empty() || !host.contains('.') || host.contains(' ') {
        return None;
    }
    let host = host.to_lowercase();
    if detail == "host_path" {
        Some(no_query.to_lowercase())
    } else {
        Some(host)
    }
}

pub fn is_browser(process: &str) -> bool {
    matches!(
        process.to_lowercase().as_str(),
        "chrome.exe" | "msedge.exe" | "firefox.exe" | "zen.exe" | "brave.exe" | "opera.exe"
    )
}

#[cfg(not(windows))]
pub fn active_url(_detail: &str) -> Option<String> {
    None
}

#[cfg(windows)]
pub fn active_url(detail: &str) -> Option<String> {
    read_address_bar().and_then(|raw| strip(&raw, detail))
}

#[cfg(windows)]
fn read_address_bar() -> Option<String> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationValuePattern, TreeScope_Descendants,
        UIA_ControlTypePropertyId, UIA_ValuePatternId,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

    // UIA control-type id for an Edit (the address bar). Stable Win32 constant.
    const UIA_EDIT_CONTROL_TYPE_ID: i32 = 50004;

    unsafe {
        // Safe to call repeatedly on the collector thread.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;

        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }

        let root = automation.ElementFromHandle(hwnd).ok()?;
        let cond = automation
            .CreatePropertyCondition(UIA_ControlTypePropertyId, &UIA_EDIT_CONTROL_TYPE_ID.into())
            .ok()?;

        let edit = root.FindFirst(TreeScope_Descendants, &cond).ok()?;
        let value: IUIAutomationValuePattern = edit.GetCurrentPatternAs(UIA_ValuePatternId).ok()?;
        let bstr = value.CurrentValue().ok()?;
        let s = bstr.to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}
