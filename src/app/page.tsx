'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import {
  getDaySummary, getReceipt, getEvents, setDayLabel, pauseTracking,
  todayKey, shiftDay, isTauri, trackingState, getDataLocation,
  openDataFolder, purgeAllData,
} from '@/lib/api';
import type { ActivityEvent, DataLocation, DaySummary, PurgeResult, Receipt } from '@/lib/types';
import ReceiptCard from '@/components/ReceiptCard';
import Vitals from '@/components/Vitals';
import Timeline from '@/components/Timeline';
import Ledger from '@/components/Ledger';
import EmptyState from '@/components/EmptyState';

const REFRESH_MS = 1000;
type PurgeState = 'idle' | 'confirming' | 'purging' | 'success' | 'error';

const commandHint = (ex: unknown): string => {
  const message = String(ex);
  if (message.includes('not found')) {
    return `${message}. The desktop backend is older than this UI; quit Trace and restart the Tauri app so the new local-data commands are registered.`;
  }
  return message;
};

export default function Page() {
  const [day, setDay] = useState(todayKey());
  const [summary, setSummary] = useState<DaySummary | null>(null);
  const [receipt, setReceipt] = useState<Receipt | null>(null);
  const [events, setEvents] = useState<ActivityEvent[]>([]);
  const [tracking, setTracking] = useState(true);
  const [err, setErr] = useState<string | null>(null);
  const [inTauri, setInTauri] = useState<boolean | null>(null);
  const [dataLocation, setDataLocation] = useState<DataLocation | null>(null);
  const [dataLocationError, setDataLocationError] = useState<string | null>(null);
  const [purgeState, setPurgeState] = useState<PurgeState>('idle');
  const [purgeResult, setPurgeResult] = useState<PurgeResult | null>(null);
  const [purgeError, setPurgeError] = useState<string | null>(null);
  const loadSeq = useRef(0);

  useEffect(() => { setInTauri(isTauri()); }, []);

  useEffect(() => {
    if (!inTauri) return;
    getDataLocation()
      .then((location) => {
        setDataLocation(location);
        setDataLocationError(null);
      })
      .catch((ex) => setDataLocationError(commandHint(ex)));
  }, [inTauri]);

  const load = useCallback(async (d: string, quiet = false) => {
    const seq = ++loadSeq.current;
    if (!quiet) setErr(null);
    try {
      const [s, r, e, t] = await Promise.all([
        getDaySummary(d),
        getReceipt(d),
        getEvents(d),
        trackingState(),
      ]);
      if (seq !== loadSeq.current) return;
      setSummary(s); setReceipt(r); setEvents(e);
      setTracking(t);
      setErr(null);
    } catch (ex) {
      if (seq === loadSeq.current) setErr(String(ex));
    }
  }, []);

  useEffect(() => { if (inTauri) load(day); }, [day, load, inTauri]);

  useEffect(() => {
    if (!inTauri) return;

    const refresh = () => { void load(day, true); };
    const id = window.setInterval(refresh, REFRESH_MS);
    window.addEventListener('focus', refresh);
    return () => {
      window.clearInterval(id);
      window.removeEventListener('focus', refresh);
    };
  }, [day, load, inTauri]);

  const toggleTracking = async () => {
    const next = !tracking;
    // Invalidate in-flight refreshes so a stale trackingState() can't
    // overwrite the optimistic toggle.
    loadSeq.current++;
    setTracking(next);
    try { await pauseTracking(!next); } catch { /* ignore */ }
  };

  const onLabel = async (v: string) => {
    await setDayLabel(day, v.trim() === '' ? null : v.trim());
    load(day);
  };

  const onOpenDataFolder = async () => {
    setPurgeError(null);
    try {
      await openDataFolder();
    } catch (ex) {
      setPurgeState('error');
      setPurgeError(`Could not open data folder: ${commandHint(ex)}`);
    }
  };

  const onConfirmPurge = async () => {
    if (purgeState === 'purging') return;
    setPurgeState('purging');
    setPurgeError(null);
    setPurgeResult(null);
    try {
      const result = await purgeAllData();
      setPurgeResult(result);
      setPurgeState('success');
      setSummary(null);
      setReceipt(null);
      setEvents([]);
      setTracking(false);
      await load(day);
    } catch (ex) {
      setPurgeError(commandHint(ex));
      setPurgeState('error');
    }
  };

  const purgeSummary = purgeResult
    ? `Purged ${purgeResult.activity_events_deleted} activity segment${purgeResult.activity_events_deleted === 1 ? '' : 's'} and ${purgeResult.day_labels_deleted} day label${purgeResult.day_labels_deleted === 1 ? '' : 's'}. Tracking is paused.`
    : null;

  const buildCats = summary?.build_categories ?? ['code', 'terminal'];
  const isEmpty = !summary || (summary.active_ms === 0 && summary.idle_ms === 0 && events.length === 0);
  const isToday = day === todayKey();
  const current = events.length > 0 ? events[events.length - 1] : null;
  const currentLabel = current
    ? current.is_idle ? 'idle' : (current.url ?? current.window_title ?? current.app_name)
    : null;
  const currentSeconds = current?.duration_ms ? Math.max(1, Math.round(current.duration_ms / 1000)) : 0;

  if (inTauri === false) {
    return (
      <div className="app">
        <header className="bar">
          <div className="wordmark">TR<span className="d">∆</span>CE</div>
        </header>
        <div className="empty" style={{ marginTop: 40 }}>
          <div className="big">Trace runs as a desktop app</div>
          <p>This is the dev server in a browser tab — it has no access to your local activity data. Open the Trace desktop window instead.</p>
        </div>
      </div>
    );
  }

  return (
    <div className="app">
      <header className="bar">
        <div className="wordmark">TR<span className="d">∆</span>CE</div>
        <div className="bar-right">
          <button className={`status${tracking ? '' : ' paused'}`} onClick={toggleTracking}
            title={tracking ? 'Pause recording' : 'Resume recording'}>
            <span className="dot" />{tracking ? 'tracking' : 'paused'}
          </button>
          <div className="daynav">
            <button onClick={() => setDay(shiftDay(day, -1))}>←</button>
            <span className="date">{day}</span>
            <button onClick={() => setDay(shiftDay(day, 1))}>→</button>
            <button className="today" onClick={() => setDay(todayKey())}>today</button>
          </div>
        </div>
      </header>

      <div className="intent" style={{ marginTop: 16 }}>
        <input
          defaultValue={summary?.label ?? ''}
          key={summary?.label ?? day}
          placeholder="what was today meant to be? e.g. coding day"
          onBlur={(e) => onLabel(e.target.value)}
        />
      </div>

      {current && (
        <div className="now">
          <span className="now-k">{isToday ? 'right now' : 'latest'}</span>
          <span className="now-app">{current.app_name}</span>
          {currentLabel && <span className="now-title" title={currentLabel}>{currentLabel}</span>}
          <span className="now-time">{currentSeconds}s</span>
        </div>
      )}

      {err && <div className="empty" style={{ marginTop: 20 }}><div className="big">Couldn’t load this day</div><p>{err}</p></div>}

      {!err && isEmpty && <div style={{ marginTop: 20 }}><EmptyState tracking={tracking} /></div>}

      {!err && !isEmpty && summary && (
        <>
          <div className="hero">
            <ReceiptCard s={summary} receipt={receipt} />
            <Vitals s={summary} />
          </div>

          <div className="eyebrow">the day, hour by hour</div>
          <Timeline events={events} day={day} buildCats={buildCats} />

          <div className="eyebrow">the ledger</div>
          <div className="ledgers">
            <Ledger
              title="categories"
              rows={summary.categories.map((c) => ({ name: c.category, ms: c.ms, build: buildCats.includes(c.category) }))}
              empty="no activity recorded"
            />
            <Ledger
              title="sites"
              rows={summary.top_sites.map((s) => ({ name: s.host, ms: s.ms }))}
              empty="no browser domains"
            />
            <Ledger
              title="apps"
              rows={summary.top_apps.map((a) => ({ name: a.app_name, ms: a.ms }))}
            />
          </div>
        </>
      )}

      <section className="privacy" aria-labelledby="privacy-title">
        <div>
          <h2 id="privacy-title">Privacy & data</h2>
          <p>
            Trace stores activity data locally only, in SQLite on this machine. This app has no
            account, cloud sync, analytics, telemetry, remote logging, crash reporter, or hidden
            upload path. Private browser windows are treated like any other browser window if the
            address bar exposes a domain.
          </p>

          <div className="privacy-grid">
            <div>
              <h3>Collected</h3>
              <ul>
                <li>Foreground app and process name.</li>
                <li>Segment start time, end time, duration, and idle state.</li>
                <li>Counts of keypresses and mouse clicks, never which keys.</li>
                <li>Mouse travel distance in pixels, never cursor positions.</li>
                <li>Browser domain only when available; query strings and fragments are stripped.</li>
                <li>Window titles only if explicitly enabled in local settings.</li>
              </ul>
            </div>
            <div>
              <h3>Never collected</h3>
              <ul>
                <li>Typed text, passwords, form fields, or message contents.</li>
                <li>Screenshots, camera, microphone, clipboard, or document contents.</li>
                <li>File contents, file paths, packet contents, or network traffic.</li>
                <li>Full browser URLs by default.</li>
              </ul>
            </div>
          </div>
        </div>

        <div>
          <div className="data-panel">
            <h3>Local storage</h3>
            <div className="path-row">
              <span>Folder</span>
              <code>{dataLocationError ? 'unavailable' : dataLocation?.data_dir ?? 'loading...'}</code>
            </div>
            <div className="path-row">
              <span>SQLite</span>
              <code>{dataLocationError ? 'unavailable' : dataLocation?.database_path ?? 'loading...'}</code>
            </div>
            <button className="btn ghost" onClick={onOpenDataFolder} disabled={!dataLocation}>
              open data folder
            </button>
            {dataLocationError && (
              <div className="status-line err">Data location unavailable: {dataLocationError}</div>
            )}
          </div>

          <div className="danger-zone">
            <h3>Danger zone</h3>
            <p>
              Purge permanently deletes local Trace history and day labels from SQLite, then
              compacts the database. Settings and category rules are kept. Tracking is paused after
              purge so new data is not recreated immediately.
            </p>
            <button
              className="btn danger-btn"
              onClick={() => {
                setPurgeError(null);
                setPurgeState('confirming');
              }}
              disabled={purgeState === 'purging'}
            >
              {purgeState === 'purging' ? 'purging...' : 'purge all data'}
            </button>
            {purgeState === 'success' && purgeSummary && (
              <div className="status-line ok">{purgeSummary}</div>
            )}
            {purgeState === 'error' && purgeError && (
              <div className="status-line err">Action failed: {purgeError}</div>
            )}
          </div>
        </div>
      </section>

      {(purgeState === 'confirming' || purgeState === 'purging') && (
        <div className="modal-backdrop" role="presentation">
          <div className="modal" role="dialog" aria-modal="true" aria-labelledby="purge-title">
            <h2 id="purge-title">Purge all local Trace history?</h2>
            <p>
              This permanently deletes activity segments, browser domains, input counts, idle
              records, and day labels from the local Trace database. Settings and category rules are
              kept, and tracking will be paused after the purge.
            </p>
            <div className="modal-actions">
              <button
                className="btn ghost"
                onClick={() => setPurgeState('idle')}
                disabled={purgeState === 'purging'}
              >
                cancel
              </button>
              <button
                className="btn danger-btn"
                onClick={onConfirmPurge}
                disabled={purgeState === 'purging'}
              >
                {purgeState === 'purging' ? 'purging...' : 'yes, purge all data'}
              </button>
            </div>
          </div>
        </div>
      )}

      <div className="footer">
        local only · no account · no cloud · counts keystrokes, never keys · window titles off by default · browser URLs kept as domain, query strings stripped · purge all data anytime
      </div>
    </div>
  );
}
