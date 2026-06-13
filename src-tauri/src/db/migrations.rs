//! Schema versioning. Fresh installs get the latest schema directly; existing
//! databases step forward one version at a time.

use anyhow::Result;
use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 8;

const SCHEMA_LATEST: &str = r#"
CREATE TABLE activity_event (
    id                  INTEGER PRIMARY KEY,
    started_at          INTEGER NOT NULL,
    ended_at            INTEGER,
    duration_ms         INTEGER,
    local_day           TEXT    NOT NULL,
    app_name            TEXT    NOT NULL,
    process_name        TEXT    NOT NULL,
    window_title        TEXT,
    url                 TEXT,
    category            TEXT    NOT NULL DEFAULT 'unknown',
    keyboard_count      INTEGER NOT NULL DEFAULT 0,
    mouse_click_count   INTEGER NOT NULL DEFAULT 0,
    mouse_move_distance REAL    NOT NULL DEFAULT 0,
    is_idle             INTEGER NOT NULL DEFAULT 0,
    unclean             INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX idx_event_day     ON activity_event(local_day);
CREATE INDEX idx_event_started ON activity_event(started_at);

CREATE TABLE category_rule (
    id         INTEGER PRIMARY KEY,
    match_type TEXT    NOT NULL,
    pattern    TEXT    NOT NULL,
    category   TEXT    NOT NULL,
    priority   INTEGER NOT NULL DEFAULT 100
);

CREATE TABLE day_meta (
    local_day TEXT PRIMARY KEY,
    label     TEXT
);

CREATE TABLE setting (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

const SEED_SETTINGS: &[(&str, &str)] = &[
    ("idle_threshold_seconds", "60"),
    ("poll_interval_ms", "1000"),
    ("capture_titles", "false"),
    ("build_categories", "code,terminal"),
    ("focus_break_idle_seconds", "120"),
    // URL capture: on, domain-only. 'host' keeps only the domain; 'host_path'
    // also keeps the path. The query string is ALWAYS stripped.
    ("track_urls", "true"),
    ("url_detail", "host"),
    // Additional comma-separated browser process names. The collector already
    // has a broad built-in Chromium/Firefox list; this is the local escape
    // hatch for niche/new browsers without a code release.
    ("browser_processes", ""),
    // Pause survives restarts; a missing key means tracking is on.
    ("tracking_paused", "false"),
];

// (match_type, pattern, category, priority)
const SEED_PROCESS_RULES: &[(&str, &str, &str, i64)] = &[
    ("process", "Code.exe", "code", 100),
    ("process", "devenv.exe", "code", 100),
    ("process", "idea64.exe", "code", 100),
    ("process", "WindowsTerminal.exe", "terminal", 100),
    ("process", "powershell.exe", "terminal", 100),
    ("process", "pwsh.exe", "terminal", 100),
    ("process", "cmd.exe", "terminal", 100),
    ("process", "chrome.exe", "browser", 50),
    ("process", "msedge.exe", "browser", 50),
    ("process", "firefox.exe", "browser", 50),
    ("process", "zen.exe", "browser", 50),
    ("process", "slack.exe", "communication", 100),
    ("process", "Discord.exe", "communication", 100),
    ("process", "Teams.exe", "communication", 100),
    ("process", "Figma.exe", "design", 100),
    ("process", "vlc.exe", "video", 100),
];

// macOS process names are the app's localizedName (from NSWorkspace), not an
// executable filename. categorize() matches the process name exactly, so without
// these every native macOS app falls through to 'unknown' and never counts as
// productive time. Seeded alongside the Windows rules on every platform — the
// names don't collide, so the wrong-OS rules simply never match.
const SEED_PROCESS_RULES_MACOS: &[(&str, &str, &str, i64)] = &[
    ("process", "Visual Studio Code", "code", 100),
    ("process", "Code", "code", 100),
    ("process", "Xcode", "code", 100),
    ("process", "IntelliJ IDEA", "code", 100),
    ("process", "Terminal", "terminal", 100),
    ("process", "iTerm2", "terminal", 100),
    ("process", "Warp", "terminal", 100),
    ("process", "Safari", "browser", 50),
    ("process", "Google Chrome", "browser", 50),
    ("process", "Chromium", "browser", 50),
    ("process", "Firefox", "browser", 50),
    ("process", "Arc", "browser", 50),
    ("process", "Brave Browser", "browser", 50),
    ("process", "Microsoft Edge", "browser", 50),
    ("process", "Opera", "browser", 50),
    ("process", "Vivaldi", "browser", 50),
    ("process", "Zen", "browser", 50),
    ("process", "Slack", "communication", 100),
    ("process", "Discord", "communication", 100),
    ("process", "Microsoft Teams", "communication", 100),
    ("process", "Figma", "design", 100),
    ("process", "VLC", "video", 100),
];

// URL rules outrank process rules (priority 200) so a browser on localhost is
// 'code', not 'browser'. This is what finally makes the focus ratio honest.
const SEED_URL_RULES: &[(&str, &str, &str, i64)] = &[
    ("url_keyword", "localhost", "code", 200),
    ("url_keyword", "127.0.0.1", "code", 200),
    ("url_keyword", "github.com", "code", 200),
    ("url_keyword", "gitlab.com", "code", 200),
    ("url_keyword", "stackoverflow.com", "code", 200),
    ("url_keyword", "docs.rs", "code", 200),
    ("url_keyword", "developer.mozilla.org", "code", 200),
    ("url_keyword", "youtube.com", "video", 200),
    ("url_keyword", "twitch.tv", "video", 200),
    ("url_keyword", "x.com", "social", 200),
    ("url_keyword", "twitter.com", "social", 200),
    ("url_keyword", "reddit.com", "social", 200),
    ("url_keyword", "news.ycombinator.com", "social", 200),
    ("url_keyword", "discord.com", "communication", 200),
    ("url_keyword", "mail.google.com", "communication", 200),
];

const SEED_LAMBDAF_RULES: &[(&str, &str, &str, i64)] = &[
    ("url_keyword", "lambdaf.org", "code", 200),
    ("title_keyword", "lambdaforge", "code", 150),
    ("title_keyword", "lambdaf-org", "code", 150),
];

pub fn run(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)",
        [],
    )?;

    let current: Option<i64> = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
            r.get(0)
        })
        .ok();

    match current {
        None => {
            conn.execute_batch(SCHEMA_LATEST)?;
            seed_settings(conn)?;
            seed_rules(conn, SEED_PROCESS_RULES)?;
            seed_rules(conn, SEED_PROCESS_RULES_MACOS)?;
            seed_rules(conn, SEED_URL_RULES)?;
            seed_rules(conn, SEED_LAMBDAF_RULES)?;
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                [SCHEMA_VERSION],
            )?;
        }
        Some(1) => {
            migrate_1_to_2(conn)?;
            migrate_2_to_3(conn)?;
            migrate_3_to_4(conn)?;
            migrate_4_to_5(conn)?;
            migrate_5_to_6(conn)?;
            migrate_6_to_7(conn)?;
            migrate_7_to_8(conn)?;
            conn.execute("UPDATE schema_version SET version = 8", [])?;
        }
        Some(2) => {
            migrate_2_to_3(conn)?;
            migrate_3_to_4(conn)?;
            migrate_4_to_5(conn)?;
            migrate_5_to_6(conn)?;
            migrate_6_to_7(conn)?;
            migrate_7_to_8(conn)?;
            conn.execute("UPDATE schema_version SET version = 8", [])?;
        }
        Some(3) => {
            migrate_3_to_4(conn)?;
            migrate_4_to_5(conn)?;
            migrate_5_to_6(conn)?;
            migrate_6_to_7(conn)?;
            migrate_7_to_8(conn)?;
            conn.execute("UPDATE schema_version SET version = 8", [])?;
        }
        Some(4) => {
            migrate_4_to_5(conn)?;
            migrate_5_to_6(conn)?;
            migrate_6_to_7(conn)?;
            migrate_7_to_8(conn)?;
            conn.execute("UPDATE schema_version SET version = 8", [])?;
        }
        Some(5) => {
            migrate_5_to_6(conn)?;
            migrate_6_to_7(conn)?;
            migrate_7_to_8(conn)?;
            conn.execute("UPDATE schema_version SET version = 8", [])?;
        }
        Some(6) => {
            migrate_6_to_7(conn)?;
            migrate_7_to_8(conn)?;
            conn.execute("UPDATE schema_version SET version = 8", [])?;
        }
        Some(7) => {
            migrate_7_to_8(conn)?;
            conn.execute("UPDATE schema_version SET version = 8", [])?;
        }
        Some(v) if v < SCHEMA_VERSION => {
            conn.execute("UPDATE schema_version SET version = ?1", [SCHEMA_VERSION])?;
        }
        _ => {}
    }
    Ok(())
}

/// v1 -> v2: add the url column and the URL capture settings + rules to an
/// existing database without losing recorded history.
fn migrate_1_to_2(conn: &Connection) -> Result<()> {
    conn.execute("ALTER TABLE activity_event ADD COLUMN url TEXT", [])?;
    seed_settings(conn)?; // INSERT OR IGNORE — only new keys land
    seed_rules(conn, SEED_URL_RULES)?;
    // make sure browser process rules sit below url rules
    conn.execute(
        "UPDATE category_rule SET priority = 50 WHERE match_type='process' AND category='browser'",
        [],
    )?;
    Ok(())
}

/// v2 -> v3 previously added network-domain capture. It is retired now, so
/// this remains a no-op to preserve version continuity.
fn migrate_2_to_3(_conn: &Connection) -> Result<()> {
    Ok(())
}

/// v3 -> v4: recognize Lambdaforge/Lambdaf work even when Zen only exposes the
/// page title and not the address bar value.
fn migrate_3_to_4(conn: &Connection) -> Result<()> {
    seed_rules(conn, SEED_LAMBDAF_RULES)?;
    Ok(())
}

/// v4 -> v5: configurable browser process allow-list for URL capture.
fn migrate_4_to_5(conn: &Connection) -> Result<()> {
    seed_settings(conn)?;
    Ok(())
}

/// v5 -> v6: public privacy pass. Window titles are opt-in from this point on,
/// and the retired network metadata table is removed if an old local database
/// still has it.
fn migrate_5_to_6(conn: &Connection) -> Result<()> {
    seed_settings(conn)?;
    conn.execute(
        "UPDATE setting SET value = 'false' WHERE key = 'capture_titles'",
        [],
    )?;
    conn.execute("DROP TABLE IF EXISTS net_event", [])?;
    Ok(())
}

/// v6 -> v7: macOS support. Existing databases predate the macOS process rules,
/// so native Mac apps were categorized as 'unknown' and never counted toward
/// productive time. Add the rules without disturbing recorded history. Only
/// inserts rules that aren't already present, so a Windows DB that somehow has
/// them (or a re-run) won't accumulate duplicates.
fn migrate_6_to_7(conn: &Connection) -> Result<()> {
    for (mt, pat, cat, prio) in SEED_PROCESS_RULES_MACOS {
        let exists: bool = conn.query_row(
            "SELECT 1 FROM category_rule WHERE match_type = ?1 AND pattern = ?2 LIMIT 1",
            rusqlite::params![mt, pat],
            |_| Ok(true),
        )
        .unwrap_or(false);
        if !exists {
            conn.execute(
                "INSERT INTO category_rule (match_type, pattern, category, priority) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![mt, pat, cat, prio],
            )?;
        }
    }
    Ok(())
}

/// v7 -> v8: re-categorize history. Adding the macOS rules in v6->v7 only
/// affects events recorded afterwards; everything captured before stayed
/// 'unknown' (the category is written once, at capture time). Re-run the rules
/// over existing non-idle events so past macOS activity counts as productive
/// too. Idle events keep their 'idle' category. Safe to skip on a brand-new DB
/// (no rows yet) and idempotent — re-running just recomputes the same values.
fn migrate_7_to_8(conn: &Connection) -> Result<()> {
    let rows: Vec<(i64, String, Option<String>, Option<String>)> = {
        let mut stmt = conn.prepare(
            "SELECT id, process_name, window_title, url
               FROM activity_event WHERE is_idle = 0",
        )?;
        let mapped = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    };

    for (id, process, title, url) in rows {
        let category =
            crate::db::repo::categorize(conn, &process, title.as_deref(), url.as_deref());
        conn.execute(
            "UPDATE activity_event SET category = ?1 WHERE id = ?2",
            rusqlite::params![category, id],
        )?;
    }
    Ok(())
}

fn seed_settings(conn: &Connection) -> Result<()> {
    for (k, v) in SEED_SETTINGS {
        conn.execute(
            "INSERT OR IGNORE INTO setting (key, value) VALUES (?1, ?2)",
            rusqlite::params![k, v],
        )?;
    }
    Ok(())
}

fn seed_rules(conn: &Connection, rules: &[(&str, &str, &str, i64)]) -> Result<()> {
    for (mt, pat, cat, prio) in rules {
        conn.execute(
            "INSERT INTO category_rule (match_type, pattern, category, priority) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![mt, pat, cat, prio],
        )?;
    }
    Ok(())
}
