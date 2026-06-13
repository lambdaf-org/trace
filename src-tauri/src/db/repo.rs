//! All SQL lives here. Functions take &Connection so the same code serves both
//! the collector (writes) and commands (reads/writes).

use std::collections::HashMap;

use anyhow::{Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

use crate::model::{ActivityEvent, CategoryRule, PurgeResult};

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

pub fn events_for_day(conn: &Connection, day: &str) -> Result<Vec<ActivityEvent>> {
    let mut stmt =
        conn.prepare("SELECT * FROM activity_event WHERE local_day = ?1 ORDER BY started_at ASC")?;
    let rows = stmt.query_map([day], row_to_event)?;
    let mut events = rows.collect::<rusqlite::Result<Vec<_>>>()?;

    // Re-resolve the category on read rather than trusting the value frozen at
    // capture time. This makes the timeline and the focus ratio self-healing:
    // events recorded before a rule or the built-in app map existed (e.g. macOS
    // apps captured by an older build) are categorized with the current logic
    // without needing a data migration. Idle stays idle.
    let rules = list_category_rules(conn).unwrap_or_default();
    for e in events.iter_mut() {
        if !e.is_idle {
            e.category = categorize_with(
                &rules,
                &e.process_name,
                e.window_title.as_deref(),
                e.url.as_deref(),
            );
        }
    }
    Ok(events)
}

pub fn delete_day(conn: &Connection, day: &str) -> Result<()> {
    conn.execute("DELETE FROM activity_event WHERE local_day = ?1", [day])?;
    if table_exists(conn, "net_event")? {
        conn.execute("DELETE FROM net_event WHERE local_day = ?1", [day])?;
    }
    conn.execute("DELETE FROM day_meta WHERE local_day = ?1", [day])?;
    // secure_delete zeroes the freed pages; truncating the WAL drops the
    // copies that lived there, so a deleted day is not recoverable from disk.
    let _ = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()));
    Ok(())
}

pub fn purge_activity_data(conn: &mut Connection) -> Result<PurgeResult> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "secure_delete", "ON")?;

    let tx = conn.transaction()?;
    let activity_events_deleted = tx.execute("DELETE FROM activity_event", [])?;
    let day_labels_deleted = tx.execute("DELETE FROM day_meta", [])?;
    let retired_network_events_deleted = if tx
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'net_event' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some()
    {
        tx.execute("DELETE FROM net_event", [])?
    } else {
        0
    };
    tx.commit()?;

    // Make "purged" mean gone from the main DB and its WAL, not merely hidden
    // from query results. VACUUM cannot run inside a transaction.
    checkpoint_wal(conn)?;
    conn.execute_batch("VACUUM")?;
    checkpoint_wal(conn)?;

    Ok(PurgeResult {
        activity_events_deleted,
        day_labels_deleted,
        retired_network_events_deleted,
        settings_preserved: true,
        tracking_paused: false,
    })
}

fn checkpoint_wal(conn: &Connection) -> Result<()> {
    let busy = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
        r.get::<_, i64>(0)
    })?;
    if busy != 0 {
        bail!("SQLite WAL checkpoint was blocked by another reader");
    }
    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool> {
    let exists = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1 LIMIT 1",
            [table],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    Ok(exists)
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

/// Built-in process -> category fallback, keyed by a normalized app name
/// (lowercased, with any trailing ".exe" removed). This is what makes
/// categorization work without relying on the seeded rule rows being present or
/// on a per-OS exact match: Windows reports "Code.exe", macOS reports the
/// localized "Code" or "Visual Studio Code", and both normalize into this map.
/// User-defined rules in the DB still take precedence — this only runs when no
/// rule matched.
fn builtin_category(process_name: &str) -> Option<&'static str> {
    let norm = process_name
        .trim()
        .to_lowercase()
        .strip_suffix(".exe")
        .map(|s| s.to_string())
        .unwrap_or_else(|| process_name.trim().to_lowercase());

    let cat = match norm.as_str() {
        // editors / IDEs
        "code" | "visual studio code" | "vscode" | "code - insiders" | "vscodium"
        | "cursor" | "windsurf" | "zed" | "devenv" | "visual studio"
        | "idea64" | "idea" | "intellij idea" | "pycharm" | "pycharm64"
        | "webstorm" | "goland" | "clion" | "rider" | "rustrover" | "phpstorm"
        | "datagrip" | "android studio" | "xcode" | "sublime text" | "sublime_text"
        | "atom" | "nova" | "neovim" | "nvim" | "vim" | "macvim" | "emacs" => "code",
        // terminals
        "terminal" | "apple terminal" | "windowsterminal" | "windows terminal"
        | "iterm" | "iterm2" | "warp" | "alacritty" | "kitty" | "wezterm"
        | "ghostty" | "hyper" | "tabby" | "powershell" | "pwsh" | "cmd" => "terminal",
        // browsers
        "chrome" | "google chrome" | "chromium" | "msedge" | "microsoft edge"
        | "firefox" | "firefox developer edition" | "safari" | "safari technology preview"
        | "arc" | "brave" | "brave browser" | "opera" | "opera gx" | "vivaldi"
        | "zen" | "duckduckgo" | "floorp" | "librewolf" | "mullvad browser"
        | "mullvadbrowser" | "pale moon" | "palemoon" | "waterfox" | "orion" => "browser",
        // communication
        "slack" | "discord" | "teams" | "microsoft teams" | "zoom" | "zoom.us"
        | "telegram" | "whatsapp" | "signal" => "communication",
        // design
        "figma" | "sketch" | "adobe xd" | "photoshop" | "illustrator" => "design",
        // video
        "vlc" | "mpv" | "quicktime player" | "iina" => "video",
        _ => return None,
    };
    Some(cat)
}

/// Resolve a category from the rules table: title keywords first, then process,
/// then a built-in app map. Loads the rules itself; see [`categorize_with`] for
/// the hot path that reuses an already-loaded rule set.
pub fn categorize(
    conn: &Connection,
    process_name: &str,
    title: Option<&str>,
    url: Option<&str>,
) -> String {
    let rules = list_category_rules(conn).unwrap_or_default();
    categorize_with(&rules, process_name, title, url)
}

/// Same as [`categorize`] but against a caller-supplied rule set, so a batch
/// (e.g. a whole day of events) can resolve categories without re-querying the
/// rules table for every row.
pub fn categorize_with(
    rules: &[CategoryRule],
    process_name: &str,
    title: Option<&str>,
    url: Option<&str>,
) -> String {
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
    // No user/seeded rule matched: fall back to the built-in app map so common
    // editors/terminals/browsers are recognized even on a DB that never got the
    // platform's seed rules.
    builtin_category(process_name)
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations;

    #[test]
    fn purge_activity_data_deletes_history_and_preserves_settings() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();

        insert_event(
            &conn,
            1_000,
            2_000,
            "2026-06-12",
            "Code",
            "code.exe",
            Some("secret-project.txt"),
            Some("github.com"),
            "code",
            12,
            3,
            42.0,
            false,
        )
        .unwrap();
        set_day_label(&conn, "2026-06-12", Some("private plan")).unwrap();
        set_setting(&conn, "tracking_paused", "false").unwrap();
        conn.execute(
            "CREATE TABLE net_event (id INTEGER PRIMARY KEY, local_day TEXT, app_name TEXT, domain TEXT)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO net_event (local_day, app_name, domain) VALUES ('2026-06-12', 'Arc', 'example.com')",
            [],
        )
        .unwrap();

        let result = purge_activity_data(&mut conn).unwrap();

        assert_eq!(result.activity_events_deleted, 1);
        assert_eq!(result.day_labels_deleted, 1);
        assert_eq!(result.retired_network_events_deleted, 1);
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM activity_event", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM day_meta", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM net_event", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            get_setting(&conn, "tracking_paused").unwrap(),
            Some("false".to_string())
        );
        assert!(!list_category_rules(&conn).unwrap().is_empty());
    }

    #[test]
    fn macos_app_names_categorize_as_productive() {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();

        // macOS reports the localized app name, not an .exe filename.
        assert_eq!(categorize(&conn, "Visual Studio Code", None, None), "code");
        assert_eq!(categorize(&conn, "Terminal", None, None), "terminal");
        assert_eq!(categorize(&conn, "Safari", None, None), "browser");
        assert_eq!(categorize(&conn, "Slack", None, None), "communication");
    }

    #[test]
    fn builtin_map_categorizes_without_any_rules() {
        // No DB rules at all: the built-in app map must still recognize common
        // editors/terminals across both naming conventions.
        let rules: Vec<CategoryRule> = vec![];
        assert_eq!(categorize_with(&rules, "Cursor", None, None), "code");
        assert_eq!(categorize_with(&rules, "Code.exe", None, None), "code");
        assert_eq!(categorize_with(&rules, "iTerm2", None, None), "terminal");
        assert_eq!(categorize_with(&rules, "Ghostty", None, None), "terminal");
        assert_eq!(categorize_with(&rules, "Totally Unknown App", None, None), "unknown");
    }

    #[test]
    fn events_for_day_reresolves_stale_categories() {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();

        // Simulate a macOS event recorded by an older build: stored as 'unknown'.
        insert_event(
            &conn, 1_000, 2_000, "2026-06-13", "Code", "Code", None, None, "unknown", 0, 0, 0.0,
            false,
        )
        .unwrap();

        let events = events_for_day(&conn, "2026-06-13").unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].category, "code");
    }
}
