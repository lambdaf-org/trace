# Trace — Privacy

Trace is a personal forensic tool. It is built so the privacy claims are
*structural* — enforced by which APIs are called and what is written to disk —
not by a promise in a settings page.

## What Trace records

- The foreground app and its process name (e.g. `Code.exe`).
- The window title, **only if title capture is explicitly enabled**. It is off
  by default because titles can contain private document or message names.
- Start time, end time, and duration of each segment.
- A **count** of keypresses during each segment. Never which keys.
- A count of mouse clicks and an approximate pixel travel distance.
- The active browser domain when available, if URL tracking is enabled.
- Whether the segment was active or idle.

## What Trace never records

- Typed text or keystroke contents.
- Screenshots or screen recordings.
- Clipboard contents.
- Network packet contents, cookies, or browser storage.
- Passwords or secrets.
- Full URLs by default, query parameters, fragments, or page contents.

## How the keystroke counter stays a counter

Input is read through the Windows **Raw Input API**, registered with
`RIDEV_INPUTSINK` so counts work across applications. The `WM_INPUT` handler
does exactly one thing with a key event: `keyboard_count += 1`. It never reads
the virtual key code's meaning, never buffers it, never writes it. The function
is small on purpose, so the claim is auditable in a single read.

Trace deliberately does **not** use low-level keyboard hooks
(`WH_KEYBOARD_LL`). Those are the mechanism real keyloggers use; avoiding them
keeps the tool both more honest and less likely to be flagged by antivirus.

## Browser URLs (domain only)

When the foreground app is a browser and URL capture is on (default), Trace
reads the address-bar text through Windows UI Automation — the same accessibility
channel a screen reader uses. It does **not** inspect network packets, install a
browser extension, or read history files.

What is kept is deliberately minimal:

- The string is stripped **before storage**: scheme and `www.` removed, and the
  query string and fragment (everything from `?` or `#`) always dropped.
- By default only the **host** is kept (`github.com`), not the path. An advanced
  local setting (`url_detail = host_path`) can keep the path too, but never the
  query string or fragment.
- URL capture can be turned off entirely (`track_urls = false`).

Domains are what make the focus ratio honest — `localhost` and `github.com`
count as build time instead of generic "browser" time — without recording what
you searched or which exact page you read.

Private/incognito browser windows are not reliably distinguishable from regular
browser windows at the OS/window level. If the browser exposes a domain in the
address bar, Trace can store that domain under the same host-only rules.

## The title caveat

Window titles contain no URLs or query strings, but a title can still be
sensitive on its own — `Q3-layoffs.xlsx`, a private channel name, a document
title. Trace treats the title as the single most sensitive field it can keep:

- Title capture is off by default.
- Title capture can be turned off entirely (stores `NULL`).
- Titles of higher-integrity processes are unreadable by design and stored as
  `<protected>`.

## Where the data lives

One SQLite file in the OS app-data directory
(`%LOCALAPPDATA%/org.lambdaf.trace/trace.db` on Windows). Trace moved away from
Roaming app data so a Windows roaming profile does not silently sync history to
a server.

No analytics, telemetry, cloud sync, crash reporter, remote logger, account
system, or external runtime font/script request is included. The frontend cannot
open the database directly; it talks to Rust through Tauri commands.

## Deleting data

The app's **Privacy & Data** section has a **Purge all data** action. It requires
confirmation, then deletes local activity segments, browser-domain values,
input counts, idle records, day labels, and any retired network-event table left
by an old database. It preserves settings and category rules, pauses tracking,
checkpoints the SQLite WAL, and runs `VACUUM`.

You can also delete the database file from the local app-data folder while Trace
is closed. When the local database and sidecar files are gone, Trace has no
other copy.

## User-controlled exits

The receipt has a **Copy receipt** button that writes the visible receipt text to
the OS clipboard when you click it. Trace never reads clipboard contents and does
not send copied text anywhere.

## Privileges

The collector uses only unprivileged Win32 calls — `GetForegroundWindow`,
`GetLastInputInfo`, Raw Input, `QueryFullProcessImageNameW` with
`PROCESS_QUERY_LIMITED_INFORMATION`. No driver, no DLL injection, no
administrator rights. That ceiling is intentional: a tool that cannot read your
keystrokes or another process's memory cannot quietly become spyware later.
