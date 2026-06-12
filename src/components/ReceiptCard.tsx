'use client';
import { useState } from 'react';
import type { DaySummary, Receipt } from '@/lib/types';
import { fmtHM, fmtHMS } from '@/lib/api';



export default function ReceiptCard({ s, receipt }: { s: DaySummary; receipt: Receipt | null }) {
  const [showFormula, setShowFormula] = useState(false);
  const [copied, setCopied] = useState(false);


  const copy = async () => {
    if (!receipt) return;
    await navigator.clipboard.writeText(`${receipt.summary}\n${receipt.formula}`);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };

  const weekday = new Date(s.local_day + 'T00:00:00').toLocaleDateString(undefined, { weekday: 'short' });

  return (
    <div className="receipt">
      <div className="r-head">
        <span className="r-title">TRACE RECEIPT</span>
        <span className="r-no">{weekday} · {s.local_day}</span>
      </div>
      <hr className="r-rule" />

      <div className="r-line"><span className="lbl">active</span><span className="val">{fmtHMS(s.active_ms)}</span></div>
      <div className="r-line"><span className="lbl">idle</span><span className="val">{fmtHMS(s.idle_ms)}</span></div>

      <hr className="r-rule" />

      {s.categories.length === 0 && <div className="lempty">no activity</div>}
      {s.categories.map((c) => {
        const build = s.build_categories.includes(c.category);
        return (
          <div className={`r-cat${build ? ' build' : ''}`} key={c.category}>
            <span className="name">{c.category}</span>
            <span className="t">{fmtHM(c.ms)}</span>
          </div>
        );
      })}

      <hr className="r-rule heavy" />

      <div className="r-focus">
        <span className="k">focus ratio</span>
        <span className="pct">{(s.focus_ratio * 100).toFixed(1)}%</span>
      </div>


      <div className="r-actions">
        <button className="btn" onClick={copy}>{copied ? 'Copied' : 'Copy receipt'}</button>
        <button className="btn ghost" onClick={() => setShowFormula((v) => !v)}>
          {showFormula ? 'Hide work' : 'Show the work'}
        </button>
      </div>

      {showFormula && receipt && <pre className="formula">{receipt.formula}</pre>}
    </div>
  );
}
