# Trace — Privacy

Trace is a personal forensic tool. It is built so the privacy claims are
*structural* — enforced by which APIs are called and what is written to disk —
not by a promise in a settings page.

## What Trace records

- The foreground app and its process name (e.g. `Code.exe`).
- The window title, **if title capture is enabled** (it is, by default, and can
  be turned off in settings — see "The title caveat").
- Start time, end time, and duration of each segment.
- A **count** of keypresses during each segment. Never which keys.
- A count of mouse clicks and an approximate pixel travel distance.
- Whether the segment was active or idle.

## What Trace never records

- Typed text or keystroke contents.
- Screenshots or screen recordings.
- Clipboard contents.
- Network packet contents, cookies, or browser storage.
- Passwords or secrets.
- Full URLs, query parameters, or page contents.

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
- By default only the **host** is kept (`github.com`), not the path. A setting
  (`url_detail = host_path`) can keep the path too, but never the query.
- URL capture can be turned off entirely (`track_urls = false`).

Domains are what make the focus ratio honest — `localhost` and `github.com`
count as build time instead of generic "browser" time — without recording what
you searched or which exact page you read.

## The title caveat

Window titles contain no URLs or query strings, but a title can still be
sensitive on its own — `Q3-layoffs.xlsx`, a private channel name, a document
title. Trace treats the title as the single most sensitive field it keeps:

- Title capture can be turned off entirely (stores `NULL`).
- Titles of higher-integrity processes are unreadable by design and stored as
  `<protected>`.

## Where the data lives

One SQLite file in the OS app-data directory
(`%APPDATA%/org.lambdaf.trace/trace.db` on Windows). No network code runs. You
can open it with any SQLite browser, export it, delete a single day or session
from the app, or delete the whole file. When it is gone, it is gone — there is
no copy anywhere else.

## Privileges

The collector uses only unprivileged Win32 calls — `GetForegroundWindow`,
`GetLastInputInfo`, Raw Input, `QueryFullProcessImageNameW` with
`PROCESS_QUERY_LIMITED_INFORMATION`. No driver, no DLL injection, no
administrator rights. That ceiling is intentional: a tool that cannot read your
keystrokes or another process's memory cannot quietly become spyware later.
