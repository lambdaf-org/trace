import { invoke } from '@tauri-apps/api/core';
import type { ActivityEvent, CategoryRule, DaySummary, Receipt } from './types';

export const todayKey = (): string => {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
};

export const shiftDay = (day: string, delta: number): string => {
  const [y, m, d] = day.split('-').map(Number);
  const dt = new Date(y, m - 1, d + delta);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${dt.getFullYear()}-${p(dt.getMonth() + 1)}-${p(dt.getDate())}`;
};

export const dayStartMs = (day: string): number => {
  const [y, m, d] = day.split('-').map(Number);
  return new Date(y, m - 1, d).getTime();
};

export const isTauri = (): boolean =>
  typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const call = <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
  if (!isTauri()) {
    return Promise.reject(new Error('Trace runs as a desktop app — local activity data is not reachable from a browser tab.'));
  }
  return invoke<T>(cmd, args);
};

export const getDaySummary = (day: string) => call<DaySummary>('get_day_summary', { day });
export const getReceipt = (day: string) => call<Receipt>('get_receipt', { day });
export const getEvents = (day: string) => call<ActivityEvent[]>('get_events', { day });
export const deleteDay = (day: string) => call<void>('delete_day', { day });
export const deleteEvent = (id: number) => call<void>('delete_event', { id });
export const setDayLabel = (day: string, label: string | null) =>
  call<void>('set_day_label', { day, label });
export const getSettings = () => call<Record<string, string>>('get_settings');
export const setSetting = (key: string, value: string) =>
  call<void>('set_setting', { key, value });
export const listCategoryRules = () => call<CategoryRule[]>('list_category_rules');
export const pauseTracking = (paused: boolean) => call<void>('pause_tracking', { paused });
export const trackingState = () => call<boolean>('tracking_state');

export const fmtHM = (ms: number): string => {
  const m = Math.floor(ms / 60000);
  return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, '0')}m`;
};

export const fmtClock = (ms: number): string => {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${p(d.getHours())}:${p(d.getMinutes())}`;
};
