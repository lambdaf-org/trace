import { fmtHM } from '@/lib/api';

export interface LedgerRow { name: string; ms: number; build?: boolean }

const fmtLedgerTime = (ms: number): string => {
  if (ms > 0 && ms < 60_000) return `${Math.max(1, Math.round(ms / 1000))}s`;
  return fmtHM(ms);
};

export default function Ledger({
  title, rows, empty,
}: { title: string; rows: LedgerRow[]; empty?: string }) {
  const max = Math.max(1, ...rows.map((r) => r.ms));
  return (
    <div className="ledger">
      <h3>{title}</h3>
      {rows.length === 0 && <div className="lempty">{empty ?? '—'}</div>}
      {rows.map((r) => (
        <div className="lrow" key={r.name}>
          <span className="name" title={r.name}>{r.name}</span>
          <span className="t">{fmtLedgerTime(r.ms)}</span>
          <span className="track"><span className={`fill${r.build ? ' build' : ''}`} style={{ width: `${(r.ms / max) * 100}%` }} /></span>
        </div>
      ))}
    </div>
  );
}
