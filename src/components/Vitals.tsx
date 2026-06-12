import type { DaySummary } from '@/lib/types';
import { fmtHMS } from '@/lib/api';

export default function Vitals({ s }: { s: DaySummary }) {
  const frag = s.most_fragmented_hour === null
    ? '—'
    : `${String(s.most_fragmented_hour).padStart(2, '0')}:00`;

  return (
    <div className="vitals">
      <div className="focus-card">
        <div className="num">{(s.focus_ratio * 100).toFixed(1)}%</div>
        <div className="lbl">focus ratio</div>
        <div className="sub">{fmtHMS(s.build_ms)} build ÷ {fmtHMS(s.active_ms)} active</div>
      </div>

      <div className="vital-grid">
        <div className="vital"><div className="k">active</div><div className="v">{fmtHMS(s.active_ms)}</div></div>
        <div className="vital"><div className="k">idle</div><div className="v">{fmtHMS(s.idle_ms)}</div></div>
        <div className="vital"><div className="k">switches</div><div className="v">{s.context_switches}</div></div>
        <div className="vital"><div className="k">longest focus</div><div className="v">{fmtHMS(s.longest_focus_ms)}</div></div>
        <div className="vital"><div className="k">fragmented hr</div><div className="v">{frag}</div></div>
        <div className="vital"><div className="k">sites</div><div className="v">{s.top_sites.length}</div></div>
      </div>

      {s.verdicts.map((v, i) => (
        <div className="verdict" key={i}>
          <div className="line">{v.line}</div>
          <div className="ev">{v.evidence}</div>
        </div>
      ))}
    </div>
  );
}
