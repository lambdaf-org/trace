//! Which apps hold connections to which domains — local, unprivileged, no
//! packet inspection. We poll the TCP connection table (remote IP + owning
//! process) and translate IPs to names through the Windows DNS client cache,
//! i.e. the lookups apps themselves already made. Names are collapsed to the
//! registrable domain ("api.github.com" -> "github.com") before storage, and
//! an IP with no cached name is dropped entirely: raw IPs are NEVER stored.
//!
//! Shared hosting makes IP->name ambiguous: one CDN address can serve
//! thousands of sites. Once two different domains have resolved to the same
//! IP, that IP is marked conflicted and never recorded again — a missing
//! receipt is acceptable, a false one is not.
//!
//! WARNING: like the UIA block in browser.rs, the Win32 calls here are the
//! version-sensitive part. `DnsGetCacheDataTable` is an undocumented dnsapi
//! export, so it is resolved at runtime via GetProcAddress; if it is missing,
//! network capture silently records nothing rather than falling back to IPs.

use std::collections::HashMap;
#[cfg(windows)]
use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

/// Collapse a DNS name to something a person recognizes: the registrable
/// domain. Heuristic eTLD+1 — keeps three labels for common two-part suffixes
/// ("bbc.co.uk"), two otherwise ("api.github.com" -> "github.com").
pub fn registrable(name: &str) -> Option<String> {
    let h = name.trim().trim_end_matches('.').to_lowercase();
    if h.is_empty() || h.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    let labels: Vec<&str> = h.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|l| l.is_empty()) {
        return None; // single-label names (hostnames, "localhost") are not domains
    }
    let n = labels.len();
    match labels[n - 1] {
        "arpa" | "local" | "internal" | "lan" | "home" | "localdomain" => return None,
        _ => {}
    }
    const TWO_PART: &[&str] = &["co", "com", "net", "org", "gov", "edu", "ac"];
    let take = if n >= 3 && labels[n - 1].len() == 2 && TWO_PART.contains(&labels[n - 2]) {
        3
    } else {
        2
    };
    Some(labels[n - take..].join("."))
}

/// Record one name resolution into the IP map. `None` marks a conflicted IP:
/// two different domains claimed it (shared CDN address), so attributing a
/// connection to either would be a guess. Conflicts are permanent until the
/// map is rebuilt.
fn merge_resolution(map: &mut HashMap<IpAddr, Option<String>>, ip: IpAddr, domain: &str) {
    match map.get(&ip) {
        Some(Some(existing)) if existing != domain => {
            map.insert(ip, None);
        }
        Some(_) => {} // same domain again, or already conflicted
        None => {
            map.insert(ip, Some(domain.to_string()));
        }
    }
}

#[cfg(not(windows))]
pub fn spawn(_db: Arc<Mutex<Connection>>, _paused: Arc<AtomicBool>) {}

#[cfg(windows)]
pub fn spawn(db: Arc<Mutex<Connection>>, paused: Arc<AtomicBool>) {
    use std::sync::atomic::Ordering;
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::db::repo;

    let (track, poll_ms) = {
        let conn = db.lock().unwrap();
        let g = |k: &str| repo::get_setting(&conn, k).ok().flatten();
        (
            g("track_network").map(|v| v == "true").unwrap_or(true),
            g("net_poll_ms")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(5000),
        )
    };
    if !track {
        return;
    }

    thread::spawn(move || {
        // Open segment per (process, domain).
        let mut open: HashMap<(String, String), NetSeg> = HashMap::new();
        // None = conflicted IP, see merge_resolution.
        let mut dns: HashMap<IpAddr, Option<String>> = HashMap::new();
        let mut procs: HashMap<u32, String> = HashMap::new();
        let mut last_dns_refresh: Option<Instant> = None;
        // A connection missing for one poll is usually table-read jitter, not a
        // disconnect; only close after this long unseen.
        let grace_ms = (poll_ms * 3) as i64;

        loop {
            let now = super::now_ms();

            if paused.load(Ordering::Relaxed) {
                close_all(&db, &mut open);
                thread::sleep(Duration::from_millis(poll_ms));
                continue;
            }

            // The DNS cache churns slowly; refreshing it is the expensive step.
            if last_dns_refresh
                .map(|t| t.elapsed().as_secs() >= 30)
                .unwrap_or(true)
            {
                refresh_dns_map(&mut dns);
                last_dns_refresh = Some(Instant::now());
                if dns.len() > 20_000 {
                    dns.clear(); // safety valve; rebuilt on the next refresh
                }
            }

            let mut seen_pids: HashSet<u32> = HashSet::new();
            let mut observed: HashSet<(String, String)> = HashSet::new();
            for (pid, ip) in tcp_connections() {
                if pid <= 4 || !is_public(&ip) {
                    continue; // System/idle pseudo-processes, loopback, LAN
                }
                let Some(Some(domain)) = dns.get(&ip) else {
                    continue; // unnamed or ambiguous IP -> never stored
                };
                seen_pids.insert(pid);
                let proc = match procs.get(&pid) {
                    Some(p) => p.clone(),
                    None => {
                        let p = process_name(pid).unwrap_or_else(|| "<unknown>".into());
                        procs.insert(pid, p.clone());
                        p
                    }
                };
                observed.insert((proc, domain.clone()));
            }
            // PIDs recycle; drop cached names for processes with no connections.
            procs.retain(|pid, _| seen_pids.contains(pid));

            for key in &observed {
                let seg = open.entry(key.clone()).or_insert(NetSeg {
                    started: now,
                    last: now,
                    row: None,
                });
                seg.last = now;
                // Written while still open so the ledger fills in live; a
                // connection held all day would otherwise never show up.
                persist_seg(&db, &key.0, &key.1, seg);
            }

            let expired: Vec<_> = open
                .iter()
                .filter(|(_, seg)| now - seg.last > grace_ms)
                .map(|(k, _)| k.clone())
                .collect();
            for key in expired {
                if let Some(mut seg) = open.remove(&key) {
                    persist_seg(&db, &key.0, &key.1, &mut seg);
                }
            }

            thread::sleep(Duration::from_millis(poll_ms));
        }
    });
}

#[cfg(windows)]
struct NetSeg {
    started: i64,
    last: i64,
    row: Option<i64>,
}

#[cfg(windows)]
fn close_all(db: &Arc<Mutex<Connection>>, open: &mut HashMap<(String, String), NetSeg>) {
    for ((proc, domain), mut seg) in open.drain() {
        persist_seg(db, &proc, &domain, &mut seg);
    }
}

/// Insert the segment's row on first persist, extend it afterwards. If the
/// row vanished (day deleted mid-segment), it stays gone: respect the delete.
#[cfg(windows)]
fn persist_seg(db: &Arc<Mutex<Connection>>, process: &str, domain: &str, seg: &mut NetSeg) {
    use crate::db::repo;
    if seg.last <= seg.started {
        return; // a single-poll blip carries no duration worth a row
    }
    if let Ok(conn) = db.lock() {
        if let Some(id) = seg.row {
            let _ = repo::update_net_event(&conn, id, seg.last);
            return;
        }
        seg.row = repo::insert_net_event(
            &conn,
            seg.started,
            seg.last,
            &super::local_day(seg.started),
            &super::friendly(process),
            process,
            domain,
        )
        .ok();
    }
}

/// Loopback, RFC1918, link-local etc. are not "network traffic" worth a row.
#[cfg(windows)]
fn is_public(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_multicast()
                || o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64/10 CGNAT
        }
        IpAddr::V6(v6) => {
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 ULA
                || (v6.segments()[0] & 0xffc0) == 0xfe80) // fe80::/10 link-local
        }
    }
}

/// Established TCP connections as (owning pid, remote address), IPv4 + IPv6.
#[cfg(windows)]
fn tcp_connections() -> Vec<(u32, IpAddr)> {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCP6TABLE_OWNER_PID, MIB_TCPROW_OWNER_PID,
        MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_CONNECTIONS,
    };
    use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

    const MIB_TCP_STATE_ESTAB: u32 = 5;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

    let mut out = Vec::new();

    unsafe {
        // The table-fetch dance is identical for v4/v6; only row layout differs.
        let fetch = |af: u32| -> Option<Vec<u8>> {
            let mut size = 0u32;
            let r = GetExtendedTcpTable(
                None,
                &mut size,
                false,
                af,
                TCP_TABLE_OWNER_PID_CONNECTIONS,
                0,
            );
            if r != ERROR_INSUFFICIENT_BUFFER && r != 0 {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            let r = GetExtendedTcpTable(
                Some(buf.as_mut_ptr() as *mut _),
                &mut size,
                false,
                af,
                TCP_TABLE_OWNER_PID_CONNECTIONS,
                0,
            );
            (r == 0).then_some(buf)
        };

        if let Some(buf) = fetch(AF_INET.0 as u32) {
            let table = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);
            let rows = std::slice::from_raw_parts(
                table.table.as_ptr() as *const MIB_TCPROW_OWNER_PID,
                table.dwNumEntries as usize,
            );
            for row in rows {
                if row.dwState == MIB_TCP_STATE_ESTAB {
                    // dwRemoteAddr is an in_addr: bytes already in address order.
                    out.push((
                        row.dwOwningPid,
                        IpAddr::V4(Ipv4Addr::from(row.dwRemoteAddr.to_ne_bytes())),
                    ));
                }
            }
        }

        if let Some(buf) = fetch(AF_INET6.0 as u32) {
            let table = &*(buf.as_ptr() as *const MIB_TCP6TABLE_OWNER_PID);
            let rows = std::slice::from_raw_parts(
                table.table.as_ptr() as *const MIB_TCP6ROW_OWNER_PID,
                table.dwNumEntries as usize,
            );
            for row in rows {
                if row.dwState == MIB_TCP_STATE_ESTAB {
                    out.push((
                        row.dwOwningPid,
                        IpAddr::V6(Ipv6Addr::from(row.ucRemoteAddr)),
                    ));
                }
            }
        }
    }

    out
}

/// pid -> image file name, same unprivileged route as foreground.rs.
#[cfg(windows)]
fn process_name(pid: u32) -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::MAX_PATH;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut path = [0u16; MAX_PATH as usize];
        let mut len = path.len() as u32;
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            PWSTR(path.as_mut_ptr()),
            &mut len,
        )
        .ok()?;
        let full = String::from_utf16_lossy(&path[..len as usize]);
        Some(full.rsplit('\\').next().unwrap_or(&full).to_string())
    }
}

/// Merge IP -> registrable domain pairs from the Windows DNS client cache into
/// `map`. Old entries are kept: a long-lived connection should stay named even
/// after its cache entry expires. Conflicts poison the IP, see merge_resolution.
#[cfg(windows)]
fn refresh_dns_map(map: &mut HashMap<IpAddr, Option<String>>) {
    for name in dns_cache_names() {
        let Some(domain) = registrable(&name) else {
            continue;
        };
        for ip in cached_ips(&name) {
            merge_resolution(map, ip, &domain);
        }
    }
}

/// Names currently in the DNS client cache, via the undocumented
/// `DnsGetCacheDataTable`. Resolved at runtime; returns empty if unavailable.
#[cfg(windows)]
fn dns_cache_names() -> Vec<String> {
    use std::sync::OnceLock;

    use windows::core::{s, w, PCWSTR};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

    #[repr(C)]
    struct DnsCacheEntry {
        next: *mut DnsCacheEntry,
        name: PCWSTR,
        wtype: u16,
        data_length: u16,
        flags: u32,
    }
    type GetTableFn = unsafe extern "system" fn(*mut *mut DnsCacheEntry) -> i32;

    static GET_TABLE: OnceLock<Option<usize>> = OnceLock::new();
    let addr = *GET_TABLE.get_or_init(|| unsafe {
        let lib = LoadLibraryW(w!("dnsapi.dll")).ok()?;
        GetProcAddress(lib, s!("DnsGetCacheDataTable")).map(|f| f as usize)
    });
    let Some(addr) = addr else { return Vec::new() };
    let get_table: GetTableFn = unsafe { std::mem::transmute(addr) };

    let mut names = Vec::new();
    unsafe {
        let mut head: *mut DnsCacheEntry = std::ptr::null_mut();
        if get_table(&mut head) == 0 || head.is_null() {
            return names;
        }
        let mut e = head;
        while !e.is_null() {
            let entry = &*e;
            if !entry.name.is_null() {
                if let Ok(n) = entry.name.to_string() {
                    names.push(n);
                }
            }
            e = entry.next;
        }
        // The table is allocated by dnsapi; freeing the flat entries keeps a
        // long-running poller from leaking.
        free_cache_table(head as *mut std::ffi::c_void);
    }
    names.sort();
    names.dedup();
    names
}

#[cfg(windows)]
unsafe fn free_cache_table(head: *mut std::ffi::c_void) {
    use std::sync::OnceLock;

    use windows::core::s;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

    // DnsFree(p, DnsFreeFlat = 0). Declared by hand to avoid pulling the typed
    // record machinery in for one call.
    type DnsFreeFn = unsafe extern "system" fn(*mut std::ffi::c_void, i32);
    static DNS_FREE: OnceLock<Option<usize>> = OnceLock::new();
    let addr = *DNS_FREE.get_or_init(|| {
        let lib = LoadLibraryA(s!("dnsapi.dll")).ok()?;
        GetProcAddress(lib, s!("DnsFree")).map(|f| f as usize)
    });
    if let Some(addr) = addr {
        let dns_free: DnsFreeFn = std::mem::transmute(addr);
        // Entries form a linked list of flat allocations.
        #[repr(C)]
        struct Entry {
            next: *mut Entry,
            name: *mut u16,
            wtype: u16,
            data_length: u16,
            flags: u32,
        }
        let mut e = head as *mut Entry;
        while !e.is_null() {
            let next = (*e).next;
            if !(*e).name.is_null() {
                dns_free((*e).name as *mut _, 0);
            }
            dns_free(e as *mut _, 0);
            e = next;
        }
    }
}

/// Cache-only A/AAAA lookup for one name: no packet leaves the machine.
#[cfg(windows)]
fn cached_ips(name: &str) -> Vec<IpAddr> {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::NetworkManagement::Dns::{
        DnsFree, DnsFreeRecordList, DnsQuery_W, DNS_QUERY_NO_WIRE_QUERY, DNS_RECORDA, DNS_TYPE_A,
        DNS_TYPE_AAAA,
    };

    let mut out = Vec::new();
    let wide = HSTRING::from(name);

    unsafe {
        for qtype in [DNS_TYPE_A, DNS_TYPE_AAAA] {
            let mut records: *mut DNS_RECORDA = std::ptr::null_mut();
            let status = DnsQuery_W(
                PCWSTR(wide.as_ptr()),
                qtype,
                DNS_QUERY_NO_WIRE_QUERY,
                None,
                &mut records,
                None,
            );
            if status.is_err() || records.is_null() {
                continue;
            }
            let mut r = records;
            while !r.is_null() {
                let rec = &*r;
                if rec.wType == DNS_TYPE_A.0 {
                    // IpAddress is an in_addr: bytes already in address order,
                    // exactly like dwRemoteAddr in tcp_connections. The two
                    // conversions MUST agree or no IPv4 ever gets a name.
                    out.push(IpAddr::V4(Ipv4Addr::from(
                        rec.Data.A.IpAddress.to_ne_bytes(),
                    )));
                } else if rec.wType == DNS_TYPE_AAAA.0 {
                    let s = rec.Data.AAAA.Ip6Address.IP6Word;
                    out.push(IpAddr::V6(Ipv6Addr::new(
                        u16::from_be(s[0]),
                        u16::from_be(s[1]),
                        u16::from_be(s[2]),
                        u16::from_be(s[3]),
                        u16::from_be(s[4]),
                        u16::from_be(s[5]),
                        u16::from_be(s[6]),
                        u16::from_be(s[7]),
                    )));
                }
                r = rec.pNext;
            }
            DnsFree(Some(records as *const _), DnsFreeRecordList);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::net::IpAddr;

    use super::{merge_resolution, registrable};

    #[test]
    fn collapses_to_registrable_domain() {
        assert_eq!(registrable("api.github.com"), Some("github.com".into()));
        assert_eq!(registrable("github.com"), Some("github.com".into()));
        assert_eq!(registrable("news.bbc.co.uk"), Some("bbc.co.uk".into()));
        assert_eq!(registrable("cdn.example.net."), Some("example.net".into()));
    }

    #[test]
    fn conflicted_ips_are_poisoned() {
        let ip: IpAddr = "104.16.0.1".parse().unwrap();
        let mut map = HashMap::new();

        merge_resolution(&mut map, ip, "github.com");
        assert_eq!(map[&ip].as_deref(), Some("github.com"));

        // Same domain again is fine.
        merge_resolution(&mut map, ip, "github.com");
        assert_eq!(map[&ip].as_deref(), Some("github.com"));

        // A second domain on the same IP makes attribution a guess: poison it.
        merge_resolution(&mut map, ip, "example.com");
        assert_eq!(map[&ip], None);

        // Once poisoned, no later resolution can un-poison it.
        merge_resolution(&mut map, ip, "github.com");
        assert_eq!(map[&ip], None);
    }

    #[test]
    fn rejects_non_domains() {
        assert_eq!(registrable("localhost"), None);
        assert_eq!(registrable("192.168.1.1"), None);
        assert_eq!(registrable("router.local"), None);
        assert_eq!(registrable("4.4.8.8.in-addr.arpa"), None);
        assert_eq!(registrable(""), None);
    }
}
