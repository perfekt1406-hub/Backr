/*
 * Purpose: Bucket sparse [`ActivityPoint`] markers into local-calendar days for dashboard charts.
 * Role: Pure helpers — no IPC; consumed by [`ActivityLineChart.svelte`](../components/dashboard/ActivityLineChart.svelte).
 */

import type { ActivityPoint } from "../types/activity";

/** Milliseconds in one calendar day (local aggregation uses midnight boundaries). */
const MS_PER_DAY = 86_400_000;

/**
 * Builds `YYYY-MM-DD` for the user's local timezone (not UTC).
 *
 * # Inputs
 *
 * * `d` — instant to reduce to a calendar date.
 */
export function localDateKey(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/**
 * Returns consecutive local date keys from `(today - days + 1)` through `today`, oldest first.
 *
 * # Inputs
 *
 * * `days` — window length (e.g. 14).
 */
export function rollingLocalDateKeys(days: number): string[] {
  const keys: string[] = [];
  const anchor = new Date();
  anchor.setHours(0, 0, 0, 0);
  for (let i = days - 1; i >= 0; i--) {
    const d = new Date(anchor.getTime() - i * MS_PER_DAY);
    keys.push(localDateKey(d));
  }
  return keys;
}

/**
 * Counts activity markers whose local calendar day falls inside the rolling window.
 *
 * # Inputs
 *
 * * `points` — completion markers from [`get_activity_series`].
 * * `days` — number of trailing local days to include.
 *
 * # Returns
 *
 * Ordered `{ dateKey, count }[]` aligned with [`rollingLocalDateKeys`].
 */
export function bucketActivityByDay(
  points: ActivityPoint[],
  days: number,
): { dateKey: string; count: number }[] {
  const keys = rollingLocalDateKeys(days);
  const counts = new Map(keys.map((k) => [k, 0]));

  for (const p of points) {
    const k = localDateKey(new Date(p.ts_unix * 1000));
    if (counts.has(k)) {
      counts.set(k, (counts.get(k) ?? 0) + 1);
    }
  }

  return keys.map((dateKey) => ({ dateKey, count: counts.get(dateKey) ?? 0 }));
}
