import type { ActivityEvent } from '@/lib/types';
import { dayStartMs, fmtClock } from '@/lib/api';

const DAY_MS = 86_400_000;

export default function Timeline({
  events, day, buildCats,
}: { events: ActivityEvent[]; day: string; buildCats: string[] }) {
  const start = dayStartMs(day);
  const segs = events.filter((e) => (e.duration_ms ?? 0) > 0);

  const color = (e: ActivityEvent) => {
    if (e.is_idle) return 'var(--idle)';
    if (buildCats.includes(e.category)) return 'var(--forge)';
    return 'var(--ink)';
  };

  return (
    <div>
      <div className="tl">
        {segs.map((e) => {
          const left = Math.max(0, ((e.started_at - start) / DAY_MS) * 100);
          const width = Math.max(0.25, ((e.duration_ms ?? 0) / DAY_MS) * 100);
          const where = e.is_idle ? 'idle' : (e.url ?? e.app_name);
          return (
            <div
              key={e.id}
              className="seg"
              title={`${where} · ${fmtClock(e.started_at)} · ${Math.round((e.duration_ms ?? 0) / 60000)}m`}
              style={{ left: `${left}%`, width: `${width}%`, background: color(e), opacity: e.is_idle ? 1 : 0.92 }}
            />
          );
        })}
      </div>
      <div className="tl-axis">
        <span>00</span><span>06</span><span>12</span><span>18</span><span>24</span>
      </div>
      <div className="legend">
        <span><i style={{ background: 'var(--forge)' }} />build</span>
        <span><i style={{ background: 'var(--ink)' }} />other</span>
        <span><i style={{ background: 'var(--idle)' }} />idle</span>
      </div>
    </div>
  );
}
