//! All SQL lives here. Functions take &Connection so the same code serves both
//! the collector (writes) and commands (reads/writes).

use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{params, Connection};

use crate::model::{ActivityEvent, CategoryRule, NetDomainTotal};

fn row_to_event(r: &rusqlite::Row) -> rusqlite::Result<ActivityEvent> {
    Ok(ActivityEvent {
        id: r.get("id")?,
        started_at: r.get("started_at")?,
        ended_at: r.get("ended_at")?,
        duration_ms: r.get("duration_ms")?,
        local_day: r.get("local_day")?,
        app_name: r.get("app_name")?,
        process_name: r.get("process_name")?,
        window_title: r.get("window_title")?,
        url: r.get("url")?,
        category: r.get("category")?,
        keyboard_count: r.get("keyboard_count")?,
        mouse_click_count: r.get("mouse_click_count")?,
        mouse_move_distance: r.get("mouse_move_distance")?,
        is_idle: r.get::<_, i64>("is_idle")? != 0,
        unclean: r.get::<_, i64>("unclean")? != 0,
    })
}

/// Insert a finalized segment. Returns the new row id.
#[allow(clippy::too_many_arguments)]
pub fn insert_event(
    conn: &Connection,
    started_at: i64,
    ended_at: i64,
    local_day: &str,
    app_name: &str,
    process_name: &str,
    window_title: Option<&str>,
    url: Option<&str>,
    category: &str,
    keyboard_count: i64,
    mouse_click_count: i64,
    mouse_move_distance: f64,
    is_idle: bool,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO activity_event
           (started_at, ended_at, duration_ms, local_day, app_name, process_name,
            window_title, url, category, keyboard_count, mouse_click_count,
            mouse_move_distance, is_idle, unclean)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 0)",
        params![
            started_at,
            ended_at,
            ended_at - started_at,
            local_day,
            app_name,
            process_name,
            window_title,
            url,
            category,
            keyboard_count,
            mouse_click_count,
            mouse_move_distance,
            is_idle as i64,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Update the still-active row for a segment. Returns false if the row was
/// removed while the collector was running.
pub fn update_event_progress(
    conn: &Connection,
    id: i64,
    ended_at: i64,
    keyboard_count: i64,
    mouse_click_count: i64,
    mouse_move_distance: f64,
) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE activity_event
            SET ended_at = ?2,
                duration_ms = CASE WHEN ?2 > started_at THEN ?2 - started_at ELSE 0 END,
                keyboard_count = ?3,
                mouse_click_count = ?4,
                mouse_move_distance = ?5
          WHERE id = ?1",
        params![
            id,
            ended_at,
            keyboard_count,
            mouse_click_count,
            mouse_move_distance,
        ],
    )?;
    Ok(changed > 0)
}

/// On startup, close any segment left open by an unclean exit at zero duration.
pub fn recover_unclean(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE activity_event
            SET ended_at = started_at, duration_ms = 0, unclean = 1
          WHERE ended_at IS NULL",
        [],
    )?;
    Ok(())
}

/// Insert a finalized network segment: one app holding connections to one domain.
pub fn insert_net_event(
    conn: &Connection,
    started_at: i64,
    ended_at: i64,
    local_day: &str,
    app_name: &str,
    process_name: &str,
    domain: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO net_event
           (started_at, ended_at, duration_ms, local_day, app_name, process_name, domain)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            started_at,
            ended_at,
            ended_at - started_at,
            local_day,
            app_name,
            process_name,
            domain,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Extend a still-open network segment. Returns false if the row was removed
/// (day deleted) while the collector was running.
pub fn update_net_event(conn: &Connection, id: i64, ended_at: i64) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE net_event
            SET ended_at = ?2,
                duration_ms = CASE WHEN ?2 > started_at THEN ?2 - started_at ELSE 0 END
          WHERE id = ?1",
        params![id, ended_at],
    )?;
    Ok(changed > 0)
}

/// Domains contacted on a day, with total connection time and the apps involved.
pub fn net_domains_for_day(conn: &Connection, day: &str) -> Result<Vec<NetDomainTotal>> {
    let mut stmt = conn.prepare(
        "SELECT domain,
                SUM(COALESCE(duration_ms, 0)) AS ms,
                GROUP_CONCAT(DISTINCT app_name) AS apps
           FROM net_event
          WHERE local_day = ?1
          GROUP BY domain
          ORDER BY ms DESC",
    )?;
    let rows = stmt.query_map([day], |r| {
        Ok(NetDomainTotal {
            domain: r.get(0)?,
            ms: r.get(1)?,
            apps: r
                .get::<_, Option<String>>(2)?
                .unwrap_or_default()
                .split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn events_for_day(conn: &Connection, day: &str) -> Result<Vec<ActivityEvent>> {
    let mut stmt =
        conn.prepare("SELECT * FROM activity_event WHERE local_day = ?1 ORDER BY started_at ASC")?;
    let rows = stmt.query_map([day], row_to_event)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn delete_day(conn: &Connection, day: &str) -> Result<()> {
    conn.execute("DELETE FROM activity_event WHERE local_day = ?1", [day])?;
    conn.execute("DELETE FROM net_event WHERE local_day = ?1", [day])?;
    conn.execute("DELETE FROM day_meta WHERE local_day = ?1", [day])?;
    // secure_delete zeroes the freed pages; truncating the WAL drops the
    // copies that lived there, so a deleted day is not recoverable from disk.
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    Ok(())
}

pub fn delete_event(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM activity_event WHERE id = ?1", [id])?;
    Ok(())
}

pub fn set_day_label(conn: &Connection, day: &str, label: Option<&str>) -> Result<()> {
    match label {
        Some(l) => conn.execute(
            "INSERT INTO day_meta (local_day, label) VALUES (?1, ?2)
               ON CONFLICT(local_day) DO UPDATE SET label = excluded.label",
            params![day, l],
        )?,
        None => conn.execute("DELETE FROM day_meta WHERE local_day = ?1", [day])?,
    };
    Ok(())
}

pub fn day_label(conn: &Connection, day: &str) -> Result<Option<String>> {
    let l = conn
        .query_row(
            "SELECT label FROM day_meta WHERE local_day = ?1",
            [day],
            |r| r.get(0),
        )
        .ok();
    Ok(l)
}

pub fn all_settings(conn: &Connection) -> Result<HashMap<String, String>> {
    let mut stmt = conn.prepare("SELECT key, value FROM setting")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<HashMap<_, _>>>()?)
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    let v = conn
        .query_row("SELECT value FROM setting WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .ok();
    Ok(v)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO setting (key, value) VALUES (?1, ?2)
           ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn list_category_rules(conn: &Connection) -> Result<Vec<CategoryRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, match_type, pattern, category, priority
           FROM category_rule ORDER BY priority DESC, id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(CategoryRule {
            id: r.get(0)?,
            match_type: r.get(1)?,
            pattern: r.get(2)?,
            category: r.get(3)?,
            priority: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn upsert_category_rule(conn: &Connection, rule: &CategoryRule) -> Result<i64> {
    if rule.id > 0 {
        conn.execute(
            "UPDATE category_rule SET match_type=?1, pattern=?2, category=?3, priority=?4 WHERE id=?5",
            params![rule.match_type, rule.pattern, rule.category, rule.priority, rule.id],
        )?;
        Ok(rule.id)
    } else {
        conn.execute(
            "INSERT INTO category_rule (match_type, pattern, category, priority) VALUES (?1, ?2, ?3, ?4)",
            params![rule.match_type, rule.pattern, rule.category, rule.priority],
        )?;
        Ok(conn.last_insert_rowid())
    }
}

pub fn delete_category_rule(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM category_rule WHERE id = ?1", [id])?;
    Ok(())
}

/// Resolve a category from the rules table: title keywords first, then process.
pub fn categorize(
    conn: &Connection,
    process_name: &str,
    title: Option<&str>,
    url: Option<&str>,
) -> String {
    let rules = list_category_rules(conn).unwrap_or_default();
    let proc_l = process_name.to_lowercase();
    let title_l = title.unwrap_or("").to_lowercase();
    let url_l = url.unwrap_or("").to_lowercase();

    // url_keyword wins (most specific): localhost/github -> code, etc.
    for r in rules.iter().filter(|r| r.match_type == "url_keyword") {
        if !url_l.is_empty() && url_l.contains(&r.pattern.to_lowercase()) {
            return r.category.clone();
        }
    }
    for r in rules.iter().filter(|r| r.match_type == "title_keyword") {
        if !title_l.is_empty() && title_l.contains(&r.pattern.to_lowercase()) {
            return r.category.clone();
        }
    }
    for r in rules.iter().filter(|r| r.match_type == "process") {
        if proc_l == r.pattern.to_lowercase() {
            return r.category.clone();
        }
    }
    "unknown".to_string()
}
