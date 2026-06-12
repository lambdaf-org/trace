//! The OS collector. One polling loop turns four signals — foreground window,
//! browser URL, idle time, input counts — into activity segments. The current
//! segment is updated while it is still open, so the UI can refresh live. A URL
//! change opens a new segment, so sites split apart cleanly.

pub mod browser;
pub mod foreground;
pub mod idle;
pub mod input;
pub mod network;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use chrono::{Local, TimeZone, Utc};
use rusqlite::Connection;

use crate::db::repo;
use crate::model::OpenSegment;

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn local_day(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

pub(crate) fn friendly(process: &str) -> String {
    match process.to_lowercase().as_str() {
        "code.exe" => "VS Code",
        "devenv.exe" => "Visual Studio",
        "idea64.exe" => "IntelliJ IDEA",
        "windowsterminal.exe" => "Terminal",
        "powershell.exe" | "pwsh.exe" => "PowerShell",
        "cmd.exe" => "Command Prompt",
        "arc.exe" => "Arc",
        "brave.exe" => "Brave",
        "chrome.exe" => "Chrome",
        "chromium.exe" => "Chromium",
        "duckduckgo.exe" => "DuckDuckGo",
        "msedge.exe" => "Edge",
        "opera.exe" => "Opera",
        "opera_gx.exe" => "Opera GX",
        "vivaldi.exe" => "Vivaldi",
        "firefox.exe" => "Firefox",
        "zen.exe" => "Zen",
        "floorp.exe" => "Floorp",
        "librewolf.exe" => "LibreWolf",
        "mullvadbrowser.exe" => "Mullvad Browser",
        "palemoon.exe" => "Pale Moon",
        "waterfox.exe" => "Waterfox",
        "discord.exe" => "Discord",
        "slack.exe" => "Slack",
        "teams.exe" => "Teams",
        "figma.exe" => "Figma",
        "vlc.exe" => "VLC",
        _ => {
            return process
                .strip_suffix(".exe")
                .or_else(|| process.strip_suffix(".EXE"))
                .unwrap_or(process)
                .to_string()
        }
    }
    .to_string()
}

struct Cfg {
    poll_ms: u64,
    idle_threshold_ms: i64,
    capture_titles: bool,
    track_urls: bool,
    url_detail: String,
    browser_processes: Vec<String>,
}

fn read_cfg(conn: &Connection) -> Cfg {
    let g = |k: &str| repo::get_setting(conn, k).ok().flatten();
    Cfg {
        poll_ms: g("poll_interval_ms")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000),
        idle_threshold_ms: g("idle_threshold_seconds")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(60)
            * 1000,
        capture_titles: g("capture_titles").map(|v| v == "true").unwrap_or(true),
        track_urls: g("track_urls").map(|v| v == "true").unwrap_or(true),
        url_detail: g("url_detail").unwrap_or_else(|| "host".to_string()),
        browser_processes: browser::browser_processes(&g("browser_processes").unwrap_or_default()),
    }
}

fn persist_segment(db: &Arc<Mutex<Connection>>, seg: &mut OpenSegment, ended_at: i64) {
    let (k, c, d) = input::snapshot();
    let keys = k.saturating_sub(seg.base_keys) as i64;
    let clicks = c.saturating_sub(seg.base_clicks) as i64;
    let dist = d.saturating_sub(seg.base_dist) as f64;

    if let Ok(conn) = db.lock() {
        if let Some(id) = seg.event_id {
            match repo::update_event_progress(&conn, id, ended_at, keys, clicks, dist) {
                Ok(true) => return,
                Ok(false) | Err(_) => seg.event_id = None,
            }
        }

        let category = if seg.is_idle {
            "idle".to_string()
        } else {
            repo::categorize(
                &conn,
                &seg.process_name,
                seg.window_title.as_deref(),
                seg.url.as_deref(),
            )
        };
        let day = local_day(seg.started_at);
        let id = repo::insert_event(
            &conn,
            seg.started_at,
            ended_at,
            &day,
            &seg.app_name,
            &seg.process_name,
            seg.window_title.as_deref(),
            seg.url.as_deref(),
            &category,
            keys,
            clicks,
            dist,
            seg.is_idle,
        )
        .ok();
        seg.event_id = id;
    }
}

pub fn spawn(db: Arc<Mutex<Connection>>, paused: Arc<AtomicBool>) {
    #[cfg(not(windows))]
    {
        let _ = (&db, &paused);
        eprintln!("[trace] collector is Windows-only in V0; running without capture.");
        return;
    }

    #[cfg(windows)]
    {
        input::start_listener();
        network::spawn(db.clone(), paused.clone());

        thread::spawn(move || {
            let cfg = {
                let conn = db.lock().unwrap();
                read_cfg(&conn)
            };

            let mut current: Option<OpenSegment> = None;

            loop {
                let now = now_ms();

                if paused.load(Ordering::Relaxed) {
                    if let Some(mut seg) = current.take() {
                        persist_segment(&db, &mut seg, now);
                    }
                    thread::sleep(Duration::from_millis(cfg.poll_ms));
                    continue;
                }

                let idle = idle::idle_ms();
                let is_idle = idle >= cfg.idle_threshold_ms as u64;

                let (app_name, process_name, title, url) = if is_idle {
                    ("Idle".to_string(), "idle".to_string(), None, None)
                } else {
                    match foreground::foreground_snapshot() {
                        Some((t, p)) => {
                            let title = if cfg.capture_titles && !t.is_empty() {
                                Some(t)
                            } else {
                                None
                            };
                            let url = if cfg.track_urls
                                && browser::is_browser(&p, &cfg.browser_processes)
                            {
                                browser::active_url(&cfg.url_detail)
                            } else {
                                None
                            };
                            (friendly(&p), p, title, url)
                        }
                        None => {
                            thread::sleep(Duration::from_millis(cfg.poll_ms));
                            continue;
                        }
                    }
                };

                let key = (app_name.clone(), title.clone(), url.clone(), is_idle);
                let changed = current.as_ref().map(|s| s.key() != key).unwrap_or(true);

                if changed {
                    if let Some(mut seg) = current.take() {
                        persist_segment(&db, &mut seg, now);
                    }
                    let (bk, bc, bd) = input::snapshot();
                    let mut next = OpenSegment {
                        event_id: None,
                        started_at: now,
                        app_name,
                        process_name,
                        window_title: title,
                        url,
                        is_idle,
                        base_keys: bk,
                        base_clicks: bc,
                        base_dist: bd,
                    };
                    persist_segment(&db, &mut next, now);
                    current = Some(next);
                } else if let Some(seg) = current.as_mut() {
                    persist_segment(&db, seg, now);
                }

                thread::sleep(Duration::from_millis(cfg.poll_ms));
            }
        });
    }
}
