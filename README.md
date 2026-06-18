# Trace

> Your machine kept receipts.

![Rust](https://img.shields.io/badge/Rust-1.x-orange?logo=rust&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri-2.x-24C8DB?logo=tauri&logoColor=white)
![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS-blue)
![Local only](https://img.shields.io/badge/cloud-none-success)

Trace is a local-only activity recorder. It watches which app is in front of you, how long you stay there, and how much you type and click, then turns the day into a plain-text **receipt**

No account. No cloud. No sync. No analytics or telemetry. It never stores typed text, screenshots, the clipboard, packet contents, or full URLs by default — only local metadata, in one SQLite file on your disk that you can inspect or purge.

<p align="center">
  <img src="docs/img/trace-showcase.png" alt="Trace Showcase with sample Data" width="640"><br>
  <sub>Trace with a currently tracked Receipt and Focus Ratio (sample data)</sub>
</p>

## Quickstart

Prerequisites: a [Rust toolchain](https://rustup.rs), [Node 18+](https://nodejs.org), and the native build tools for your OS — on Windows the MSVC build tools + WebView2 (preinstalled on Windows 11), on macOS the Xcode Command Line Tools (`xcode-select --install`).

```
git clone https://github.com/lambdaf-org/trace
cd trace
npm install
npm run tauri dev
```

The database lives in the local OS app-data dir — `%LOCALAPPDATA%/org.lambdaf.trace/trace.db` on Windows, `~/Library/Application Support/org.lambdaf.trace/trace.db` on macOS. Trace also exposes this path in the Privacy & Data section, with an "Open data folder" button.

### macOS permissions

macOS gates the signals Trace reads behind per-app permissions (System Settings → Privacy & Security). Each is requested on first use and Trace degrades gracefully if denied (this can cause issues) — it simply omits that one signal:

- **Accessibility** — required for window titles (when enabled) and to count keystrokes/clicks. Without it, app + duration are still recorded.
- **Input Monitoring** — may also be requested for the global keyboard/mouse counters.
- **Automation** (Apple Events) — required to read the active browser tab's domain. Granted per browser the first time Trace asks. Firefox/Gecko browsers don't expose the URL this way, so domains aren't recorded for them.

Build an installer (produces the current platform's bundles — NSIS + MSI on Windows, `.app` + `.dmg` on macOS):

```
npm run tauri icon assets/logo.png   # one-time, generates icons/
npm run tauri build
```

## What Trace records / never records

| Records (metadata only)                 | Never records                           |
| --------------------------------------- | --------------------------------------- |
| Active app + process name               | Typed text / keystroke contents         |
| Window title only if explicitly enabled | Screenshots or screen recording         |
| Active browser **domain** by default    | Query parameters / page text            |
| Start / end / duration                  | Clipboard contents                      |
| Keypress **count** (not keys)           | Packet contents, cookies, storage       |
| Mouse click count + travel distance     | Passwords or secrets                    |
| Idle vs active                          | Clicks/keys mapped to content           |

The keypress counter is the line that matters: the raw-input handler increments a number and never inspects which key fired. That is the whole difference between a counter and a keylogger, and it is enforceable in one function. See [docs/PRIVACY.md](docs/PRIVACY.md).

## Deleting data

Use the **Privacy & Data** section in the app and click **Purge all data**. Trace requires confirmation, then deletes local activity segments, browser-domain rows inside those segments, input counts, idle records, day labels, and any retired network-event table left by an old local database. It preserves settings and category rules, pauses tracking, checkpoints the SQLite WAL, and runs `VACUUM`.

You can also delete the database file from the local app-data folder while Trace is closed. The in-app purge is safer because it handles the open database and sidecar files cleanly.

The receipt's copy button only writes the visible receipt to your OS clipboard after you click it. Trace never reads the clipboard.

## Features

- **Activity segments**: every stretch in an app is one row — app, process, optional title, optional browser domain, duration, and the input counts for that stretch.
- **Idle detection**: away time is measured system-wide via `GetLastInputInfo` and marked as its own segment, never folded into active time.
- **Daily receipt**: a copyable plain-text summary plus a **formula view** that prints the worked arithmetic behind each number — the receipt is the product.
- **Computed verdicts**: opinionated, deterministic verdicts ("you prompted more than you implemented") where each line prints the metric that triggered it. No vibes.
- **Editable categories**: a local rules table maps processes to categories (`code`, `terminal`, `browser`, …); you own the rules.
- **Claim vs reality**: label a day's intent ("coding day") and the receipt reconciles what you said against what the machine measured.
- **One file, your disk**: SQLite, WAL-mode, no service, no network. Pause from the tray; delete a day or purge all history.

## How it works

```
OS activity  →  Rust collector  →  activity segments  →  SQLite  →  daily metrics  →  receipt
```

A single Rust collector watches foreground app/process, system idle time, input counts from the Raw Input API registered with `RIDEV_INPUTSINK`, and browser domains when available. Window titles are off by default because titles can contain private document or message names. A segmenter closes the current segment and opens a new one whenever the app, optional title, browser domain, or idle state changes, attaching the input counts accumulated during that stretch. Segments land in SQLite; the frontend never touches the database — it asks the Rust side through Tauri commands, so the privacy boundary is one module.

Metrics are computed deterministically over a day's ordered segments — active/idle split, category and app breakdowns, context switches, longest focus block, most interrupted hour, and the focus ratio — then rendered as the receipt and its formula view. Nothing is asserted that the tally cannot back.

## Privacy

Trace is built so the privacy claims are structural, not promises. The collector uses only unprivileged OS calls — Win32 on Windows, and NSWorkspace + the Accessibility API + Apple Events on macOS — with no driver, no injection, no admin. Input is counted, never captured. The runtime app has no analytics, telemetry, cloud sync, crash reporter, remote logger, or external font/script request. Everything stays in the local app-data SQLite database. The full record-vs-never list is in [docs/PRIVACY.md](docs/PRIVACY.md), and the implementation plan is in [docs/PLAN.md](docs/PLAN.md).

## Roadmap

Trace runs on Windows and macOS, sharing one segmenter behind per-OS collectors. Browser-**domain** tracking is read from the address bar via UI Automation on Windows and via Apple Events on macOS, query strings stripped, host-only by default. Future work should keep the same default: local-only, no accounts, no telemetry, no screenshots, and richer capture only behind explicit opt-in settings. A Linux collector is possible behind the same segmenter.

Browser private/incognito windows are not reliably distinguishable from regular windows by the OS collector. If a browser exposes a domain in the address bar, Trace can store that domain under the same host-only rules.

## Contributing

Lambdaforge is open source and contributions are welcome. Start with the [contributor guide](https://github.com/lambdaf-org/contributing), and see the org-wide [CONTRIBUTING](https://github.com/lambdaf-org/.github/blob/main/CONTRIBUTING.md) and [Code of Conduct](https://github.com/lambdaf-org/.github/blob/main/CODE_OF_CONDUCT.md).

## License

This repository does not yet include a `LICENSE` file, so default copyright applies for now. A license is coming soon. If you want to use or build on this before then, please open an issue.
