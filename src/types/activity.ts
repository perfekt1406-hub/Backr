/*
 * Purpose: Activity strip markers from `get_activity_series`.
 * Role: Sparse backup-history visualization without a dedicated events table.
 */

/** Single dashboard activity marker. */
export type ActivityPoint = {
  ts_unix: number;
  label: string;
};
