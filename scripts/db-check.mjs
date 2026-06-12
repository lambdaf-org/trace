// Diagnostic: inspect the live Trace DB (read-only). Run: node scripts/db-check.mjs
import { DatabaseSync } from 'node:sqlite';
import { existsSync } from 'node:fs';

const local = process.env.LOCALAPPDATA + '\\org.lambdaf.trace\\trace.db';
const roaming = process.env.APPDATA + '\\org.lambdaf.trace\\trace.db';
const path = existsSync(local) ? local : roaming;
console.log('db:', path, existsSync(roaming) ? '(NOTE: roaming copy still exists)' : '');

const db = new DatabaseSync(path, { readOnly: true });
const q = (sql) => db.prepare(sql).all();

console.log('--- settings ---');
console.log(q('SELECT key, value FROM setting'));

console.log('--- last 5 activity events ---');
console.log(q(`SELECT id, datetime(started_at/1000,'unixepoch','localtime') AS start,
  app_name, url, category, is_idle FROM activity_event ORDER BY id DESC LIMIT 5`));

console.log('--- last 10 net events ---');
console.log(q(`SELECT id, datetime(started_at/1000,'unixepoch','localtime') AS start,
  datetime(ended_at/1000,'unixepoch','localtime') AS end,
  app_name, domain, duration_ms FROM net_event ORDER BY id DESC LIMIT 10`));

console.log('--- net events today ---');
console.log(q(`SELECT COUNT(*) AS rows, COUNT(DISTINCT domain) AS domains,
  COUNT(DISTINCT process_name) AS apps,
  MAX(datetime(ended_at/1000,'unixepoch','localtime')) AS last_end
  FROM net_event WHERE local_day = date('now','localtime')`));
