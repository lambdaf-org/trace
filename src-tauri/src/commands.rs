//! The Rust<->frontend surface. Every command takes the shared connection from
//! managed state; the frontend never opens the database itself.

use std::sync::atomic::Ordering;

use tauri::State;

use crate::db::repo;
use crate::metrics;
use crate::model::{ActivityEvent, AppState, CategoryRule, DaySummary, Receipt};

type Cmd<T> = Result<T, String>;

fn lock<'a>(
    state: &'a State<AppState>,
) -> Result<std::sync::MutexGuard<'a, rusqlite::Connection>, String> {
    state.db.lock().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_day_summary(state: State<AppState>, day: String) -> Cmd<DaySummary> {
    let conn = lock(&state)?;
    metrics::day_summary(&conn, &day).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_receipt(state: State<AppState>, day: String) -> Cmd<Receipt> {
    let conn = lock(&state)?;
    let summary = metrics::day_summary(&conn, &day).map_err(|e| e.to_string())?;
    Ok(metrics::build_receipt(&conn, &summary).map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn get_events(state: State<AppState>, day: String) -> Cmd<Vec<ActivityEvent>> {
    let conn = lock(&state)?;
    repo::events_for_day(&conn, &day).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_day(state: State<AppState>, day: String) -> Cmd<()> {
    let conn = lock(&state)?;
    repo::delete_day(&conn, &day).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_event(state: State<AppState>, id: i64) -> Cmd<()> {
    let conn = lock(&state)?;
    repo::delete_event(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_day_label(state: State<AppState>, day: String, label: Option<String>) -> Cmd<()> {
    let conn = lock(&state)?;
    repo::set_day_label(&conn, &day, label.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Cmd<std::collections::HashMap<String, String>> {
    let conn = lock(&state)?;
    repo::all_settings(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_setting(state: State<AppState>, key: String, value: String) -> Cmd<()> {
    let conn = lock(&state)?;
    repo::set_setting(&conn, &key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_category_rules(state: State<AppState>) -> Cmd<Vec<CategoryRule>> {
    let conn = lock(&state)?;
    repo::list_category_rules(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn upsert_category_rule(state: State<AppState>, rule: CategoryRule) -> Cmd<i64> {
    let conn = lock(&state)?;
    repo::upsert_category_rule(&conn, &rule).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_category_rule(state: State<AppState>, id: i64) -> Cmd<()> {
    let conn = lock(&state)?;
    repo::delete_category_rule(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn pause_tracking(state: State<AppState>, paused: bool) -> Cmd<()> {
    state.paused.store(paused, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub fn tracking_state(state: State<AppState>) -> Cmd<bool> {
    Ok(!state.paused.load(Ordering::Relaxed))
}
