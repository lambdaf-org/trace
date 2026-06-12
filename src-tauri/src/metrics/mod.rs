//! Daily aggregation. Deterministic over a day's ordered segments — the receipt
//! must be able to show the arithmetic behind every number.

pub mod verdicts;

use std::collections::HashMap;

use anyhow::Result;
use chrono::{Local, TimeZone, Timelike};
use rusqlite::Connection;

use crate::db::repo;
use crate::model::{ActivityEvent, AppTotal, CategoryTotal, DaySummary, Receipt, SiteTotal};

fn build_categories(conn: &Connection) -> Vec<String> {
    repo::get_setting(conn, "build_categories")
        .ok()
        .flatten()
        .unwrap_or_else(|| "code,terminal".to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn focus_break_idle_ms(conn: &Connection) -> i64 {
    repo::get_setting(conn, "focus_break_idle_seconds")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(120)
        * 1000
}

fn host_of(url: &str) -> String {
    url.split('/').next().unwrap_or(url).to_string()
}

pub fn day_summary(conn: &Connection, day: &str) -> Result<DaySummary> {
    let events = repo::events_for_day(conn, day)?;

    let active_ms: i64 = events
        .iter()
        .filter(|e| !e.is_idle)
        .filter_map(|e| e.duration_ms)
        .sum();
    let idle_ms: i64 = events
        .iter()
        .filter(|e| e.is_idle)
        .filter_map(|e| e.duration_ms)
        .sum();

    let categories = group(&events, |e| Some(e.category.clone()))
        .into_iter()
        .map(|(category, ms)| CategoryTotal { category, ms })
        .collect::<Vec<_>>();

    let top_apps = {
        let mut v = group(&events, |e| Some(e.app_name.clone()))
            .into_iter()
            .map(|(app_name, ms)| AppTotal { app_name, ms })
            .collect::<Vec<_>>();
        v.truncate(10);
        v
    };

    let top_sites = {
        let mut v = group(&events, |e| e.url.as_deref().map(host_of))
            .into_iter()
            .map(|(host, ms)| SiteTotal { host, ms })
            .collect::<Vec<_>>();
        v.truncate(10);
        v
    };

    let builds = build_categories(conn);
    let build_ms: i64 = categories
        .iter()
        .filter(|c| builds.contains(&c.category))
        .map(|c| c.ms)
        .sum();
    let focus_ratio = if active_ms > 0 {
        build_ms as f64 / active_ms as f64
    } else {
        0.0
    };

    let context_switches = count_switches(&events);
    let longest_focus_ms = longest_focus(&events, &builds, focus_break_idle_ms(conn));
    let most_fragmented_hour = fragmented_hour(&events);
    let label = repo::day_label(conn, day)?;

    let mut summary = DaySummary {
        local_day: day.to_string(),
        active_ms,
        idle_ms,
        categories,
        top_apps,
        top_sites,
        focus_ratio,
        build_ms,
        context_switches,
        longest_focus_ms,
        most_fragmented_hour,
        label,
        build_categories: builds,
        verdicts: vec![],
    };
    summary.verdicts = verdicts::evaluate(&summary);
    Ok(summary)
}

/// Sum non-idle duration by a key projection, sorted desc. None keys skip.
fn group<F>(events: &[ActivityEvent], key: F) -> Vec<(String, i64)>
where
    F: Fn(&ActivityEvent) -> Option<String>,
{
    let mut map: HashMap<String, i64> = HashMap::new();
    for e in events.iter().filter(|e| !e.is_idle) {
        if let Some(k) = key(e) {
            *map.entry(k).or_default() += e.duration_ms.unwrap_or(0);
        }
    }
    let mut v: Vec<_> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1));
    v
}

fn count_switches(events: &[ActivityEvent]) -> i64 {
    let active: Vec<&ActivityEvent> = events.iter().filter(|e| !e.is_idle).collect();
    active
        .windows(2)
        .filter(|w| w[0].app_name != w[1].app_name)
        .count() as i64
}

fn longest_focus(events: &[ActivityEvent], builds: &[String], break_idle_ms: i64) -> i64 {
    const GLANCE_MS: i64 = 60_000;
    let mut best: i64 = 0;
    let mut run: i64 = 0;
    for e in events {
        let dur = e.duration_ms.unwrap_or(0);
        let is_build = !e.is_idle && builds.contains(&e.category);
        if is_build {
            run += dur;
        } else if e.is_idle && dur > break_idle_ms {
            best = best.max(run);
            run = 0;
        } else if !e.is_idle && dur > GLANCE_MS {
            best = best.max(run);
            run = 0;
        }
        best = best.max(run);
    }
    best
}

fn fragmented_hour(events: &[ActivityEvent]) -> Option<i32> {
    let active: Vec<&ActivityEvent> = events.iter().filter(|e| !e.is_idle).collect();
    let mut per_hour = [0i32; 24];
    for w in active.windows(2) {
        if w[0].app_name != w[1].app_name {
            if let Some(dt) = Local.timestamp_millis_opt(w[1].started_at).single() {
                per_hour[dt.hour() as usize] += 1;
            }
        }
    }
    let (hour, max) = per_hour
        .iter()
        .enumerate()
        .max_by_key(|(_, c)| **c)
        .map(|(h, c)| (h as i32, *c))?;
    (max > 0).then_some(hour)
}

fn fmt_hm(ms: i64) -> String {
    let m = ms / 60_000;
    format!("{}h {:02}m", m / 60, m % 60)
}

pub fn build_receipt(_conn: &Connection, s: &DaySummary) -> Result<Receipt> {
    let mut summary = String::new();
    summary.push_str(&format!("TRACE RECEIPT — {}\n\n", s.local_day));
    summary.push_str(&format!("Active time:  {}\n", fmt_hm(s.active_ms)));
    summary.push_str(&format!("Idle time:    {}\n\n", fmt_hm(s.idle_ms)));
    for c in &s.categories {
        summary.push_str(&format!("{:<16}{}\n", c.category, fmt_hm(c.ms)));
    }

    let mut formula = String::new();
    formula.push_str("Focus ratio = build-tool time / active time\n\n");
    let build_parts: Vec<String> = s
        .categories
        .iter()
        .filter(|c| s.build_categories.contains(&c.category) && c.ms > 0)
        .map(|c| format!("{} ({})", c.category, fmt_hm(c.ms)))
        .collect();
    if !build_parts.is_empty() {
        formula.push_str(&format!("  build = {}\n", build_parts.join(" + ")));
    }
    formula.push_str(&format!("  build time   = {}\n", fmt_hm(s.build_ms)));
    formula.push_str(&format!("  active time  = {}\n", fmt_hm(s.active_ms)));
    formula.push_str(&format!(
        "  focus ratio  = {} / {} = {:.1}%\n\n",
        fmt_hm(s.build_ms),
        fmt_hm(s.active_ms),
        s.focus_ratio * 100.0
    ));

    if !s.top_sites.is_empty() {
        formula.push_str("Top sites:\n");
        for site in s.top_sites.iter().take(5) {
            formula.push_str(&format!("  {:<28}{}\n", site.host, fmt_hm(site.ms)));
        }
        formula.push('\n');
    }

    formula.push_str(&format!("Context switches: {}\n", s.context_switches));
    formula.push_str(&format!(
        "Longest focus block: {}\n",
        fmt_hm(s.longest_focus_ms)
    ));
    if let Some(h) = s.most_fragmented_hour {
        formula.push_str(&format!(
            "Most fragmented hour: {:02}:00–{:02}:00\n",
            h,
            h + 1
        ));
    }

    if !s.verdicts.is_empty() {
        formula.push_str("\nVERDICT\n");
        for v in &s.verdicts {
            formula.push_str(&format!("  {}\n    └ {}\n", v.line, v.evidence));
        }
    }

    Ok(Receipt { summary, formula })
}
