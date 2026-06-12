//! Shared types: DB rows, the open segment the collector accumulates, and the
//! serialized shapes the frontend receives.

use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub id: i64,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_ms: Option<i64>,
    pub local_day: String,
    pub app_name: String,
    pub process_name: String,
    pub window_title: Option<String>,
    pub url: Option<String>,
    pub category: String,
    pub keyboard_count: i64,
    pub mouse_click_count: i64,
    pub mouse_move_distance: f64,
    pub is_idle: bool,
    pub unclean: bool,
}

#[derive(Debug, Clone)]
pub struct OpenSegment {
    pub event_id: Option<i64>,
    pub started_at: i64,
    pub app_name: String,
    pub process_name: String,
    pub window_title: Option<String>,
    pub url: Option<String>,
    pub is_idle: bool,
    pub base_keys: u64,
    pub base_clicks: u64,
    pub base_dist: u64,
}

impl OpenSegment {
    /// Segment boundary identity: app + title + url + idle. A new URL opens a new
    /// segment, so github.com and localhost become separate rows.
    pub fn key(&self) -> (String, Option<String>, Option<String>, bool) {
        (
            self.app_name.clone(),
            self.window_title.clone(),
            self.url.clone(),
            self.is_idle,
        )
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryTotal {
    pub category: String,
    pub ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppTotal {
    pub app_name: String,
    pub ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SiteTotal {
    pub host: String,
    pub ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Verdict {
    pub line: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DaySummary {
    pub local_day: String,
    pub active_ms: i64,
    pub idle_ms: i64,
    pub categories: Vec<CategoryTotal>,
    pub top_apps: Vec<AppTotal>,
    pub top_sites: Vec<SiteTotal>,
    pub focus_ratio: f64,
    pub build_ms: i64,
    pub context_switches: i64,
    pub longest_focus_ms: i64,
    pub most_fragmented_hour: Option<i32>,
    pub label: Option<String>,
    pub build_categories: Vec<String>,
    pub verdicts: Vec<Verdict>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Receipt {
    pub summary: String,
    pub formula: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryRule {
    pub id: i64,
    pub match_type: String, // 'process' | 'title_keyword' | 'url_keyword'
    pub pattern: String,
    pub category: String,
    pub priority: i64,
}

pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub paused: Arc<AtomicBool>,
}
