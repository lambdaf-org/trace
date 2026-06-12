//! Database open/migrate. The Rust side owns this file; the frontend goes
//! through Tauri commands.

pub mod migrations;
pub mod repo;

use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use rusqlite::Connection;

/// `%LOCALAPPDATA%/org.lambdaf.trace/trace.db` on Windows, platform app-data
/// dir elsewhere. Local, NOT Roaming: a roaming profile syncs %APPDATA% to a
/// server, which would silently break the "local only" promise.
pub fn data_dir() -> PathBuf {
    let base = dirs_app_data();
    let dir = base.join("org.lambdaf.trace");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn database_path() -> PathBuf {
    let dir = data_dir();
    let path = dir.join("trace.db");
    #[cfg(windows)]
    migrate_from_roaming(&dir, &path);
    path
}

#[cfg(windows)]
fn dirs_app_data() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Early builds kept the DB in %APPDATA% (Roaming). Move it once, sidecar
/// files included, so existing history follows and nothing is left behind to
/// roam.
#[cfg(windows)]
fn migrate_from_roaming(new_dir: &std::path::Path, new_db: &std::path::Path) {
    if new_db.exists() {
        return;
    }
    let Some(roaming) = std::env::var_os("APPDATA").map(PathBuf::from) else {
        return;
    };
    let old_dir = roaming.join("org.lambdaf.trace");
    if old_dir.as_path() == new_dir {
        return;
    }
    for suffix in ["", "-wal", "-shm"] {
        let old = old_dir.join(format!("trace.db{suffix}"));
        if !old.exists() {
            continue;
        }
        let new = new_dir.join(format!("trace.db{suffix}"));
        if fs::rename(&old, &new).is_err() && fs::copy(&old, &new).is_ok() {
            let _ = fs::remove_file(&old);
        }
    }
    let _ = fs::remove_dir(&old_dir); // only succeeds once empty
}

#[cfg(not(windows))]
fn dirs_app_data() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn open() -> Result<Connection> {
    let conn = Connection::open(database_path())?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    // "Delete any day anytime" must mean gone: zero freed pages on delete
    // instead of leaving row contents recoverable in the freelist.
    conn.pragma_update(None, "secure_delete", "ON")?;
    migrations::run(&conn)?;
    Ok(conn)
}
