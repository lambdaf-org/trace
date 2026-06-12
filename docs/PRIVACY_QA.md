# Privacy QA Checklist

Run this before a public release that claims "local-only."

## Build and launch

- `npm run build` completes.
- `cargo test` passes in `src-tauri`.
- `cargo check` passes in `src-tauri`.
- `npm run tauri dev` opens the desktop app.

## Local-only audit

- Search confirms no runtime `fetch`, XHR, WebSocket, analytics, telemetry, crash reporting, remote logging, cloud sync, or account/auth code.
- The app shell does not load Google Fonts or any other remote font/script.
- Tauri CSP blocks external runtime connections except local Tauri IPC/dev server behavior.
- Dependency registry URLs in lockfiles are treated as build/install metadata, not runtime app behavior.

## Collection behavior

- New databases seed `capture_titles = false`.
- Browser capture stores host/domain by default and strips query strings/fragments before storage.
- Input capture stores counts only; no key identity, typed text, screenshots, clipboard, file contents, packet contents, or document contents are collected.
- The Privacy & Data section copy matches the implementation.

## Purge all data

- The visible **Purge all data** button opens a confirmation dialog.
- Cancel closes the dialog and deletes nothing.
- Confirm deletes rows from `activity_event`, `day_meta`, and retired `net_event` tables if present.
- Settings and category rules remain.
- Tracking is paused after purge.
- The dashboard refreshes to empty data after purge.
- The success/failure message is visible and clear.
- The SQLite WAL is checkpointed/truncated and `VACUUM` runs without error.

## Storage location

- The Privacy & Data section shows the data folder and `trace.db` path.
- **Open data folder** opens the local app-data folder.
- On Windows, the active database path is under `%LOCALAPPDATA%/org.lambdaf.trace/`, not `%APPDATA%`.
