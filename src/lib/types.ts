// Mirrors the serde shapes from the Rust side (snake_case).

export interface ActivityEvent {
  id: number;
  started_at: number;
  ended_at: number | null;
  duration_ms: number | null;
  local_day: string;
  app_name: string;
  process_name: string;
  window_title: string | null;
  url: string | null;
  category: string;
  keyboard_count: number;
  mouse_click_count: number;
  mouse_move_distance: number;
  is_idle: boolean;
  unclean: boolean;
}

export interface CategoryTotal { category: string; ms: number; }
export interface AppTotal { app_name: string; ms: number; }
export interface SiteTotal { host: string; ms: number; }
export interface Verdict { line: string; evidence: string; }

export interface DaySummary {
  local_day: string;
  active_ms: number;
  idle_ms: number;
  categories: CategoryTotal[];
  top_apps: AppTotal[];
  top_sites: SiteTotal[];
  focus_ratio: number;
  build_ms: number;
  context_switches: number;
  longest_focus_ms: number;
  most_fragmented_hour: number | null;
  label: string | null;
  build_categories: string[];
  verdicts: Verdict[];
}

export interface Receipt { summary: string; formula: string; }

export interface DataLocation {
  data_dir: string;
  database_path: string;
}

export interface PurgeResult {
  activity_events_deleted: number;
  day_labels_deleted: number;
  retired_network_events_deleted: number;
  settings_preserved: boolean;
  tracking_paused: boolean;
}

export interface CategoryRule {
  id: number;
  match_type: string;
  pattern: string;
  category: string;
  priority: number;
}
