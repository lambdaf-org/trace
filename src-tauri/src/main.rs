// Hide the console window on Windows release builds.
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod collector;
mod commands;
mod db;
mod metrics;
mod model;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use model::AppState;
use tauri::Manager;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;

fn main() {
    // Open + migrate the database before anything else touches it.
    let conn = db::open().expect("failed to open/migrate database");
    // Close any segment left open by an unclean shutdown, so we never invent time.
    db::repo::recover_unclean(&conn).expect("crash recovery failed");

    // Pause is a privacy promise; it must survive restarts (dev rebuilds,
    // autostart at login), so it is restored from settings, not reset to off.
    let paused_at_start = db::repo::get_setting(&conn, "tracking_paused")
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);

    let db = Arc::new(Mutex::new(conn));
    let paused = Arc::new(AtomicBool::new(paused_at_start));
    let data_epoch = Arc::new(AtomicU64::new(0));

    let collector_db = db.clone();
    let collector_paused = paused.clone();
    let collector_data_epoch = data_epoch.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState {
            db: db.clone(),
            paused: paused.clone(),
            data_epoch: data_epoch.clone(),
        })
        .setup(move |app| {
            // Tray: Open / Pause tracking / Quit. Pause is a one-click privacy control.
            let open = MenuItemBuilder::with_id("open", "Open").build(app)?;
            let pause = MenuItemBuilder::with_id("pause", "Pause tracking").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&open, &pause, &quit])
                .build()?;

            let tray_paused = paused.clone();
            let tray_db = db.clone();
            let _tray = TrayIconBuilder::with_id("trace-tray")
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "open" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "pause" => {
                        let now = !tray_paused.load(Ordering::Relaxed);
                        tray_paused.store(now, Ordering::Relaxed);
                        if let Ok(conn) = tray_db.lock() {
                            let _ = db::repo::set_setting(
                                &conn,
                                "tracking_paused",
                                if now { "true" } else { "false" },
                            );
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            // Start the OS collector on its own thread.
            collector::spawn(
                collector_db.clone(),
                collector_paused.clone(),
                collector_data_epoch.clone(),
            );
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_day_summary,
            commands::get_receipt,
            commands::get_events,
            commands::delete_day,
            commands::delete_event,
            commands::data_location,
            commands::open_data_folder,
            commands::purge_all_data,
            commands::set_day_label,
            commands::get_settings,
            commands::set_setting,
            commands::list_category_rules,
            commands::upsert_category_rule,
            commands::delete_category_rule,
            commands::pause_tracking,
            commands::tracking_state,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Trace");
}
