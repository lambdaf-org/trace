//! Schema versioning. Fresh installs get the latest schema directly; existing
//! databases step forward one version at a time.

use anyhow::Result;
use rusqlite::Connection;

pub const SCHEMA_VERSION: i64 = 5;

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
    ("capture_titles", "true"),
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
            conn.execute("UPDATE schema_version SET version = 5", [])?;
        }
        Some(2) => {
            migrate_2_to_3(conn)?;
            migrate_3_to_4(conn)?;
            migrate_4_to_5(conn)?;
            conn.execute("UPDATE schema_version SET version = 5", [])?;
        }
        Some(3) => {
            migrate_3_to_4(conn)?;
            migrate_4_to_5(conn)?;
            conn.execute("UPDATE schema_version SET version = 5", [])?;
        }
        Some(4) => {
            migrate_4_to_5(conn)?;
            conn.execute("UPDATE schema_version SET version = 5", [])?;
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
