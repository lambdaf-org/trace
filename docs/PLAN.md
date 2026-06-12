# TRACE — V0 IMPLEMENTATION PLAN

Local-only computer activity receipts for builders. Windows-first.
No account. No cloud. Metadata only. The machine kept receipts; you get to read them.

---

## 0. Stack decisions (and the one gotcha)

| Layer | Choice | Why |
|---|---|---|
| Shell | **Tauri v2** | Small binary, Rust backend owns the data, no Electron bloat. |
| Backend | **Rust** | Same process does collection + DB + metrics. |
| DB | **SQLite via `rusqlite` (`bundled` feature)** | Statically linked SQLite, single file on disk, no service. |
| Frontend | **Next.js, static export** | App Router with `output: 'export'`. |
| Styling | Tailwind + a few Shadcn primitives | Plain tables, mono formula blocks. |
| OS APIs | **`windows` crate** (official Microsoft bindings) | Foreground, raw input, idle. |

**The gotcha:** Tauri serves a *static* frontend. Next.js must be exported, not run as a server. Set in `next.config.js`:

```js
module.exports = {
  output: 'export',
  images: { unoptimized: true },
};
```

Then point Tauri at the export in `tauri.conf.json`:

```jsonc
{
  "build": {
    "frontendDist": "../out",
    "devUrl": "http://localhost:3000",
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build"
  }
}
```

Decision worth making early: **the Rust side owns the SQLite file.** The collector writes to it directly; the frontend never opens the DB — it asks Rust via `#[tauri::command]`. One writer, no contention surprises, and the privacy boundary is a single Rust module.

---

## 1. Folder structure

```
trace/
├─ docs/
│  ├─ PLAN.md                 # this file
│  └─ PRIVACY.md              # the literal "records / does not record" list
├─ src/                       # Next.js (static export)
│  ├─ app/
│  │  ├─ layout.tsx
│  │  └─ page.tsx             # the day dashboard
│  ├─ components/
│  │  ├─ VerdictCard.tsx
│  │  ├─ ReceiptText.tsx      # copyable plain-text receipt
│  │  ├─ FormulaView.tsx      # monospace "show your work" block
│  │  ├─ Timeline.tsx         # hour-by-hour activity bar
│  │  ├─ CategoryTable.tsx
│  │  ├─ TopAppsTable.tsx
│  │  └─ MetricsGrid.tsx
│  ├─ lib/
│  │  ├─ api.ts               # invoke() wrappers, typed
│  │  └─ types.ts             # DaySummary, ActivityEvent, Verdict
│  └─ styles/globals.css      # cream / dark / forge-red tokens
├─ src-tauri/
│  ├─ Cargo.toml
│  ├─ tauri.conf.json
│  ├─ build.rs
│  └─ src/
│     ├─ main.rs              # tauri::Builder, setup(), spawn collector, tray
│     ├─ model.rs            # ActivityEvent, Category, DaySummary, Verdict
│     ├─ commands.rs         # all #[tauri::command]s
│     ├─ db/
│     │  ├─ mod.rs
│     │  ├─ migrations.rs     # schema_version + CREATE statements
│     │  └─ repo.rs           # insert/close events, queries
│     ├─ collector/
│     │  ├─ mod.rs            # spawns the collector thread + message loop
│     │  ├─ foreground.rs     # GetForegroundWindow / SetWinEventHook, title, pid→process
│     │  ├─ idle.rs           # GetLastInputInfo
│     │  ├─ input.rs          # raw input counters + cursor-distance poll
│     │  └─ segmenter.rs      # raw signals -> ActivityEvent rows
│     └─ metrics/
│        ├─ mod.rs            # daily aggregation
│        └─ verdicts.rs       # opinionated, receipt-backed verdict rules
├─ package.json
└─ next.config.js
```

---

## 2. SQLite schema

Timestamps are **Unix epoch milliseconds, UTC**. A denormalized `local_day` (`YYYY-MM-DD`, computed from the local timezone at `started_at`) makes day grouping trivial and correct across DST.

```sql
PRAGMA journal_mode = WAL;        -- reads while the collector writes
PRAGMA foreign_keys = ON;

CREATE TABLE schema_version (version INTEGER NOT NULL);

CREATE TABLE activity_event (
    id                  INTEGER PRIMARY KEY,
    started_at          INTEGER NOT NULL,            -- epoch ms, UTC
    ended_at            INTEGER,                     -- NULL while the segment is open
    duration_ms         INTEGER,                     -- set on close
    local_day           TEXT    NOT NULL,            -- 'YYYY-MM-DD' local
    app_name            TEXT    NOT NULL,            -- friendly: "VS Code"
    process_name        TEXT    NOT NULL,            -- raw: "Code.exe"
    window_title        TEXT,                        -- nullable / redactable
    category            TEXT    NOT NULL DEFAULT 'unknown',
    keyboard_count      INTEGER NOT NULL DEFAULT 0,  -- counts only, never keys
    mouse_click_count   INTEGER NOT NULL DEFAULT 0,
    mouse_move_distance REAL    NOT NULL DEFAULT 0,  -- pixels, approximate
    is_idle             INTEGER NOT NULL DEFAULT 0,  -- 0 | 1
    unclean             INTEGER NOT NULL DEFAULT 0   -- 1 if closed by crash recovery
);
CREATE INDEX idx_event_day     ON activity_event(local_day);
CREATE INDEX idx_event_started ON activity_event(started_at);

-- process/title -> category, user-editable, highest priority wins
CREATE TABLE category_rule (
    id         INTEGER PRIMARY KEY,
    match_type TEXT    NOT NULL,                     -- 'process' | 'title_keyword'
    pattern    TEXT    NOT NULL,                     -- case-insensitive
    category   TEXT    NOT NULL,
    priority   INTEGER NOT NULL DEFAULT 100
);

-- optional per-day intent, powers claim-vs-reality verdicts
CREATE TABLE day_meta (
    local_day TEXT PRIMARY KEY,
    label     TEXT                                   -- e.g. "coding day"
);

CREATE TABLE setting (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

Seed `setting`: `idle_threshold_seconds=60`, `poll_interval_ms=1000`, `capture_titles=true`, `build_categories=code,terminal`, `focus_break_idle_seconds=120`.

Seed `category_rule` (process matches): `Code.exe→code`, `devenv.exe→code`, `WindowsTerminal.exe→terminal`, `powershell.exe/pwsh.exe/cmd.exe→terminal`, `chrome.exe/msedge.exe/firefox.exe→browser`, `slack.exe/discord.exe/Teams.exe→communication`, `Figma.exe→design`, `vlc.exe→video`. Ship sane defaults; users edit the table.

---

## 3. The collector (the hard part)

Everything below is **unprivileged** — no admin, no driver, no injection. That is the whole credibility of "not spyware." The only cost: you cannot read the window title of a higher-integrity process (an elevated terminal, the UAC dialog). That is fine — store `NULL`/`"<protected>"` and move on.

### 3a. One thread, one hidden window, one message loop

`SetWinEventHook` (foreground changes) and raw input (`WM_INPUT`) both deliver through a thread's Win32 message queue, so the collector is a single dedicated thread that:

1. creates a **message-only window** (`HWND_MESSAGE`),
2. registers raw input devices to it with `RIDEV_INPUTSINK` (so it receives input even when not focused),
3. installs `SetWinEventHook(EVENT_SYSTEM_FOREGROUND, …)`,
4. sets a 1s `WM_TIMER` for idle polling + title re-check + cursor-distance sampling,
5. runs `GetMessage`/`DispatchMessage` forever.

Counters live as plain fields on the thread (incremented in the window proc — same thread, no locks). Finalized segments are pushed to the DB writer over an `mpsc` channel so the window proc never touches SQLite.

### 3b. Foreground app + title + process

```rust
// illustrative — feature-gate windows crate as the compiler directs
use windows::Win32::Foundation::{HWND, MAX_PATH};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_NAME_FORMAT};

fn foreground_snapshot() -> Option<(String /*title*/, String /*process*/)> {
    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0 == 0 { return None; }

        let mut buf = [0u16; 512];
        let n = GetWindowTextW(hwnd, &mut buf);
        let title = String::from_utf16_lossy(&buf[..n as usize]);

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut path = [0u16; MAX_PATH as usize];
        let mut len = path.len() as u32;
        QueryFullProcessImageNameW(h, PROCESS_NAME_FORMAT(0), windows::core::PWSTR(path.as_mut_ptr()), &mut len).ok()?;
        let full = String::from_utf16_lossy(&path[..len as usize]);
        let process = full.rsplit('\\').next().unwrap_or(&full).to_string(); // "Code.exe"
        Some((title, process))
    }
}
```

`PROCESS_QUERY_LIMITED_INFORMATION` is the unprivileged path and works for same/lower integrity processes — exactly the right capability ceiling.

### 3c. Idle

```rust
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::System::SystemInformation::GetTickCount;

fn idle_ms() -> u32 {
    unsafe {
        let mut lii = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        GetLastInputInfo(&mut lii);
        GetTickCount().wrapping_sub(lii.dwTime) // wrapping_sub handles 32-bit tick rollover
    }
}
```

When `idle_ms()` crosses `idle_threshold_seconds`, close the active segment and open an idle one (`is_idle = 1`, counts = 0). On the next input, close idle and reopen for the current foreground. `GetLastInputInfo` is system-wide and stores **nothing** about the input — ideal.

### 3d. Input counts without capturing input

Register raw input for keyboard (usage page `0x01`, usage `0x06`) and mouse (`0x01`/`0x02`) with `RIDEV_INPUTSINK`. In the `WM_INPUT` handler, `GetRawInputData` → on keyboard make events `keyboard_count += 1`; on mouse button-down flags `mouse_click_count += 1`. **You never inspect which key.** That is the difference between a counter and a keylogger, and it is enforceable in one function.

Mouse travel: rather than decode raw deltas (mickeys, not pixels), poll `GetCursorPos()` on the 1s timer (or 100ms for smoother distance) and sum Euclidean pixel deltas. Position is summed and discarded, never stored.

> Alternative considered: global `WH_KEYBOARD_LL` hooks. They work but are what actual keyloggers use, so antivirus flags them and the privacy story gets harder to tell. Raw input with `INPUTSINK` is the defensible choice. Note this in `PRIVACY.md`.

### 3e. Segmenter

A **segment** is a continuous run with the same `(app_name, window_title, is_idle)`. Close-and-reopen on: foreground change (WinEvent), title change (1s poll catches in-app title switches), or idle flip. On close, set `ended_at`, `duration_ms`, attach the accumulated counts, run categorization, write the row.

**Crash recovery:** on startup, any row with `ended_at IS NULL` is a dangling segment from an unclean exit — close it at its `started_at` (zero duration) and set `unclean = 1` so the data stays honest rather than inventing time.

---

## 4. Metrics — with exact definitions

Computed deterministically over a day's ordered events. The receipt must *show* these, so each needs a definition you can print.

- **Active time** = `SUM(duration_ms)` where `is_idle = 0`. **Idle time** = same where `is_idle = 1`.
- **Top apps** = group non-idle events by `app_name`, sum duration, order desc.
- **Category breakdown** = group non-idle events by `category`.
- **Context switch** = a transition between two consecutive **non-idle** events with different `app_name`. (Idle gaps don't count as switches.)
- **Longest focus block** = the longest-duration maximal run of consecutive non-idle events all in `build_categories`, broken only by idle longer than `focus_break_idle_seconds` or a non-build segment longer than 60s. Brief glances tolerated.
- **Most fragmented hour** = the local clock hour with the most context switches (tie-break: shortest mean active-segment duration).
- **Focus ratio** = build-tool active time ÷ total active time.

Two queries to anchor the style; the run-length metrics (focus block, fragmentation) are cleaner computed in Rust over the ordered vector:

```sql
-- active vs idle for a day
SELECT is_idle, SUM(duration_ms) AS ms
FROM activity_event WHERE local_day = ?1 GROUP BY is_idle;

-- context switches via window function (SQLite >= 3.25)
SELECT COUNT(*) FROM (
  SELECT app_name, LAG(app_name) OVER (ORDER BY started_at) AS prev
  FROM activity_event WHERE local_day = ?1 AND is_idle = 0
) WHERE prev IS NOT NULL AND app_name <> prev;
```

---

## 5. The receipt + verdicts (the lambdaforge part)

The receipt has two layers: the **summary** and the **formula view** that shows how each number was produced — literally the worked arithmetic from your example (`3h 12m + 42m + 28m + 36m = 4h 58m`). Generate both as plain text so the copy button yields exactly what's on screen.

**Verdicts are deterministic and receipt-backed** — every verdict line prints the metric that triggered it. No vibes. A starter rule set:

| Condition | Verdict |
|---|---|
| `active ≥ 5h` and `focus_ratio ≥ 0.60` | "Real build day. {ratio}% of active time in build tools." |
| `ai_time > code_time` | "You prompted more than you implemented. AI {ai}m vs code {code}m." |
| `switches ≥ 80` and `mean_active_segment < 3m` | "Not lazy — fragmented. {switches} switches, avg focus {mean}m." |
| `browser` is top non-build bucket and `focus_ratio < 0.50` | "Biggest leak: browser, {b}m, and build never recovered past {ratio}%." |
| `day_meta.label` set and `focus_ratio` below claim threshold | "You called this a {label}. Only {ratio}% of it was inside build tools." |

The last row is why `day_meta.label` exists — it turns the tool from descriptive into the confrontational receipt you want. It's optional and cheap.

---

## 6. Commands (the Rust↔frontend surface)

```
get_day_summary(local_day) -> DaySummary           // metrics + verdicts
get_receipt(local_day)     -> { summary, formula } // two plain-text blocks
get_events(local_day)      -> Vec<ActivityEvent>   // for the timeline
delete_day(local_day)      -> ()
delete_event(id)           -> ()
set_day_label(local_day, label) -> ()
get_settings() / set_setting(key, value)
list_category_rules() / upsert_category_rule(rule) / delete_category_rule(id)
pause_tracking(bool)                                // tray affordance
```

Tray + autostart via `tauri-plugin-autostart`; a tray menu with **Open**, **Pause tracking**, **Quit**. "Pause tracking" is itself a privacy feature — make it one click.

---

## 7. Staged MVP checklist

Each phase has an acceptance test. Don't start the next until the current one passes.

**Phase 0 — Scaffold.** Tauri v2 + Next static export render a window; one `invoke` round-trips; SQLite file opens and migrates to v1.
✓ A trivial command returns data into the dashboard. DB file exists on disk.

**Phase 1 — Foreground tracking.** WinEvent foreground hook + 1s title poll + segmenter writing rows (no counts, no idle yet).
✓ Switching apps for two minutes produces correct, non-overlapping rows with sane durations.

**Phase 2 — Idle.** `GetLastInputInfo` polling; idle segments with `is_idle=1`.
✓ Walking away for >threshold creates one idle segment; returning resumes the right app.

**Phase 3 — Input counts.** Raw input keyboard/click counters + cursor-distance poll, flushed onto segments.
✓ Counts are non-zero and roughly proportional to activity; verified that no key identity is ever read (code review of `input.rs`).

**Phase 4 — Metrics.** Daily aggregation: active/idle, top apps, categories, focus ratio, switches, longest focus block, fragmented hour.
✓ Numbers reconcile by hand against a short known session.

**Phase 5 — Dashboard + receipt.** Timeline, tables, metrics grid, copyable receipt, formula view.
✓ Copied receipt is byte-identical to the on-screen formula block.

**Phase 6 — Categorization + verdicts + day label.** Default + editable `category_rule`; verdict engine; optional intent label.
✓ Editing a rule reclassifies the day; each verdict prints its triggering metric.

**Phase 7 — Privacy + lifecycle.** Delete day/session; settings (idle threshold, title capture toggle, build categories); visible `PRIVACY.md` copy in-app; crash recovery for dangling segments.
✓ Deleting a day removes all its rows; disabling title capture stores `NULL`; killing the app mid-segment leaves a clean `unclean=1` row on restart.

**Phase 8 — Ship.** System tray, autostart, pause toggle, MSI/NSIS bundle via the Tauri bundler.
✓ Installs clean, starts on login minimized to tray, tracks unattended for a full day.

---

## 8. Things to verify against current docs before pinning

Architecture above is stable, but lock exact versions when you start: latest **Tauri 2.x** + matching CLI, current **`windows` crate** feature flags (the compiler names the exact `Win32_*` features you must enable), **`rusqlite`** with `bundled`, and **`tauri-plugin-autostart`**. The Win32 APIs themselves (`GetForegroundWindow`, `GetLastInputInfo`, raw input, `SetWinEventHook`) are decades-stable.
