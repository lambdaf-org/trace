// Temporary diagnostic: inspect the live Trace DB (read-only).
import { DatabaseSync } from 'node:sqlite';

const path = process.env.APPDATA + '\\org.lambdaf.trace\\trace.db';
const db = new DatabaseSync(path, { readOnly: true });

const q = (sql) => db.prepare(sql).all();

console.log('--- settings ---');
console.log(q('SELECT key, value FROM settings'));

console.log('--- last 10 events ---');
console.log(q(`SELECT id, datetime(started_at/1000,'unixepoch','localtime') AS start,
  datetime(ended_at/1000,'unixepoch','localtime') AS end,
  app_name, process_name, url, category, is_idle
  FROM events ORDER BY id DESC LIMIT 10`));

console.log('--- events today ---');
console.log(q(`SELECT COUNT(*) AS n, SUM(url IS NOT NULL) AS with_url,
  MAX(datetime(ended_at/1000,'unixepoch','localtime')) AS last_end
  FROM events WHERE day = date('now','localtime')`));

console.log('--- browser events today ---');
console.log(q(`SELECT process_name, COUNT(*) AS n, SUM(url IS NOT NULL) AS with_url
  FROM events WHERE day = date('now','localtime')
  AND process_name IN ('chrome.exe','msedge.exe','firefox.exe','zen.exe','brave.exe','opera.exe')
  GROUP BY process_name`));
