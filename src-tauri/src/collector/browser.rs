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
    // host is everything before the first slash, minus a plain port.
    let host_port = no_query.split('/').next().unwrap_or(no_query);
    let host = if let Some(rest) = host_port.strip_prefix('[') {
        rest.split(']').next().unwrap_or(host_port)
    } else {
        host_port.split(':').next().unwrap_or(host_port)
    };
    // Reject things that aren't host-like (new tab page, search text, etc.).
    if host.is_empty()
        || host.contains(' ')
        || !(host.eq_ignore_ascii_case("localhost")
            || host.parse::<std::net::IpAddr>().is_ok()
            || host.contains('.'))
    {
        return None;
    }
    let host = host.to_lowercase();
    if detail == "host_path" {
        Some(no_query.to_lowercase())
    } else {
        Some(host)
    }
}

#[cfg(test)]
mod tests {
    use super::strip;

    #[test]
    fn strips_to_host_without_query_or_port() {
        assert_eq!(
            strip("https://github.com/org/repo?x=1", "host"),
            Some("github.com".into())
        );
        assert_eq!(
            strip("localhost:3000/dashboard?x=1", "host"),
            Some("localhost".into())
        );
        assert_eq!(
            strip("http://127.0.0.1:3000/", "host"),
            Some("127.0.0.1".into())
        );
    }

    #[test]
    fn rejects_non_urlish_address_text() {
        assert_eq!(strip("new tab", "host"), None);
        assert_eq!(strip("search terms with spaces", "host"), None);
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
    read_address_bar(detail)
}

#[cfg(windows)]
fn read_address_bar(detail: &str) -> Option<String> {
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

        let edits = root.FindAll(TreeScope_Descendants, &cond).ok()?;
        let len = edits.Length().ok()?;
        for i in 0..len {
            let edit = edits.GetElement(i).ok()?;
            let value: IUIAutomationValuePattern =
                match edit.GetCurrentPatternAs(UIA_ValuePatternId) {
                    Ok(value) => value,
                    Err(_) => continue,
                };
            let bstr = match value.CurrentValue() {
                Ok(value) => value,
                Err(_) => continue,
            };
            if let Some(url) = strip(&bstr.to_string(), detail) {
                return Some(url);
            }
        }
        None
    }
}
