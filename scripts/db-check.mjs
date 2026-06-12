// Diagnostic: inspect the live Trace DB (read-only). Run: node scripts/db-check.mjs
import { DatabaseSync } from 'node:sqlite';
import { existsSync } from 'node:fs';

const local = process.env.LOCALAPPDATA + '\\org.lambdaf.trace\\trace.db';
const roaming = process.env.APPDATA + '\\org.lambdaf.trace\\trace.db';
const path = existsSync(local) ? local : roaming;
console.log('db:', path, existsSync(roaming) ? '(NOTE: roaming copy still exists)' : '');

const db = new DatabaseSync(path, { readOnly: true });
const q = (sql) => db.prepare(sql).all();
const tableExists = (name) =>
  q(`SELECT 1 FROM sqlite_master WHERE type='table' AND name='${name}' LIMIT 1`).length > 0;

console.log('--- settings ---');
console.log(q('SELECT key, value FROM setting'));

console.log('--- last 5 activity events ---');
console.log(q(`SELECT id, datetime(started_at/1000,'unixepoch','localtime') AS start,
  app_name, process_name, window_title, url, category, keyboard_count,
  mouse_click_count, mouse_move_distance, is_idle
  FROM activity_event ORDER BY id DESC LIMIT 5`));

if (tableExists('net_event')) {
  console.log('NOTE: retired net_event table exists in this old local DB; app purge removes its rows.');
}
