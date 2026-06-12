//! Database open/migrate. The Rust side owns this file; the frontend goes
//! through Tauri commands.

pub mod migrations;
pub mod repo;

use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use rusqlite::Connection;

/// `%APPDATA%/org.lambdaf.trace/trace.db` on Windows, platform app-data dir elsewhere.
fn db_path() -> PathBuf {
    let base = dirs_app_data();
    let dir = base.join("org.lambdaf.trace");
    let _ = fs::create_dir_all(&dir);
    dir.join("trace.db")
}

#[cfg(windows)]
fn dirs_app_data() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(not(windows))]
fn dirs_app_data() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn open() -> Result<Connection> {
    let conn = Connection::open(db_path())?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrations::run(&conn)?;
    Ok(conn)
}
