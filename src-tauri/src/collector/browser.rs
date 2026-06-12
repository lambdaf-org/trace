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

const DEFAULT_BROWSER_PROCESSES: &[&str] = &[
    // Chromium family
    "arc.exe",
    "brave.exe",
    "chrome.exe",
    "chromium.exe",
    "duckduckgo.exe",
    "maxthon.exe",
    "msedge.exe",
    "opera.exe",
    "opera_gx.exe",
    "qutebrowser.exe",
    "sidekick.exe",
    "thorium.exe",
    "vivaldi.exe",
    "whale.exe",
    "yandex.exe",
    // Firefox / Gecko family
    "basilisk.exe",
    "firefox.exe",
    "floorp.exe",
    "librewolf.exe",
    "mullvadbrowser.exe",
    "palemoon.exe",
    "seamonkey.exe",
    "torbrowser.exe",
    "waterfox.exe",
    "zen.exe",
];

fn normalize_process_name(process: &str) -> Option<String> {
    let p = process.trim().to_lowercase();
    if p.is_empty() || p.contains('\\') || p.contains('/') {
        return None;
    }
    Some(if p.ends_with(".exe") {
        p
    } else {
        format!("{p}.exe")
    })
}

pub fn browser_processes(extra: &str) -> Vec<String> {
    let mut out = DEFAULT_BROWSER_PROCESSES
        .iter()
        .filter_map(|p| normalize_process_name(p))
        .collect::<Vec<_>>();
    for p in extra.split(',').filter_map(normalize_process_name) {
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

pub fn is_browser(process: &str, browsers: &[String]) -> bool {
    let Some(process) = normalize_process_name(process) else {
        return false;
    };
    browsers.iter().any(|p| p == &process)
}

fn looks_like_address_control(name: &str, automation_id: &str) -> bool {
    let haystack = format!("{name} {automation_id}").to_lowercase();
    [
        "address",
        "awesomebar",
        "location",
        "omnibox",
        "search or enter",
        "search with",
        "url",
    ]
    .iter()
    .any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::{browser_processes, is_browser, looks_like_address_control, strip};

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

    #[test]
    fn recognizes_default_and_configured_browser_processes() {
        let browsers = browser_processes("custombrowser, Weird.exe, chrome.exe");
        assert!(is_browser("vivaldi.exe", &browsers));
        assert!(is_browser("LibreWolf.exe", &browsers));
        assert!(is_browser("custombrowser.exe", &browsers));
        assert!(is_browser("weird.exe", &browsers));
        assert!(!is_browser("code.exe", &browsers));
    }

    #[test]
    fn recognizes_address_control_metadata() {
        assert!(looks_like_address_control(
            "Search with Google or enter address",
            ""
        ));
        assert!(looks_like_address_control("", "urlbar-input"));
        assert!(looks_like_address_control("Address and search bar", ""));
        assert!(!looks_like_address_control("Email", "login"));
    }
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
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationValuePattern,
        TreeScope_Descendants, UIA_ControlTypePropertyId, UIA_ValuePatternId,
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

        let read_value = |element: &IUIAutomationElement| -> Option<String> {
            let value: IUIAutomationValuePattern =
                match element.GetCurrentPatternAs(UIA_ValuePatternId) {
                    Ok(value) => value,
                    Err(_) => return None,
                };
            let bstr = match value.CurrentValue() {
                Ok(value) => value,
                Err(_) => return None,
            };
            Some(bstr.to_string())
        };

        let edits = root.FindAll(TreeScope_Descendants, &cond).ok()?;
        let len = edits.Length().ok()?;
        for i in 0..len {
            let edit = edits.GetElement(i).ok()?;
            let Some(raw) = read_value(&edit) else {
                continue;
            };
            if let Some(url) = strip(&raw, detail) {
                return Some(url);
            }
        }

        let all = root
            .FindAll(
                TreeScope_Descendants,
                &automation.CreateTrueCondition().ok()?,
            )
            .ok()?;
        let len = all.Length().ok()?;
        for i in 0..len.min(500) {
            let element = all.GetElement(i).ok()?;
            let Some(raw) = read_value(&element) else {
                continue;
            };
            let name = element
                .CurrentName()
                .map(|value| value.to_string())
                .unwrap_or_default();
            let automation_id = element
                .CurrentAutomationId()
                .map(|value| value.to_string())
                .unwrap_or_default();
            if looks_like_address_control(&name, &automation_id) {
                if let Some(url) = strip(&raw, detail) {
                    return Some(url);
                }
            }
        }

        None
    }
}
