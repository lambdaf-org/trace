'use client';

import { useCallback, useEffect, useRef, useState } from 'react';
import {
  getDaySummary, getReceipt, getEvents, setDayLabel, pauseTracking,
  todayKey, shiftDay, isTauri, trackingState,
} from '@/lib/api';
import type { ActivityEvent, DaySummary, Receipt } from '@/lib/types';
import ReceiptCard from '@/components/ReceiptCard';
import Vitals from '@/components/Vitals';
import Timeline from '@/components/Timeline';
import Ledger from '@/components/Ledger';
import EmptyState from '@/components/EmptyState';

const REFRESH_MS = 1000;

export default function Page() {
  const [day, setDay] = useState(todayKey());
  const [summary, setSummary] = useState<DaySummary | null>(null);
  const [receipt, setReceipt] = useState<Receipt | null>(null);
  const [events, setEvents] = useState<ActivityEvent[]>([]);
  const [tracking, setTracking] = useState(true);
  const [err, setErr] = useState<string | null>(null);
  const [inTauri, setInTauri] = useState<boolean | null>(null);
  const loadSeq = useRef(0);

  useEffect(() => { setInTauri(isTauri()); }, []);

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

      <div className="footer">
        local only · no account · no cloud · counts keystrokes, never keys · records window titles, private windows too · URLs kept as domain, query strings stripped · delete any day anytime, gone for real
      </div>
    </div>
  );
}
