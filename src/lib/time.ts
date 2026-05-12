/*
 * Purpose: Human-readable timestamps for backup schedules.
 * Role: Thin wrapper over `date-fns` to keep imports centralized.
 */

import {
  formatDistanceToNow,
  formatISO9075,
  parseISO,
} from "date-fns";

/**
 * Relative phrase (“about 2 hours ago”) from an RFC3339 instant or `null`.
 *
 * External: `parseISO` / `formatDistanceToNow` from `date-fns`.
 */
export function relativeFromIso(iso: string | null): string {
  if (!iso) return "never";
  try {
    return formatDistanceToNow(parseISO(iso), { addSuffix: true });
  } catch {
    return iso;
  }
}

/**
 * Compact numeric timestamp suitable for dense instrument panels.
 *
 * External: `formatISO9075` formats without timezone verbosity.
 */
export function compactTimestamp(iso: string): string {
  try {
    return formatISO9075(parseISO(iso));
  } catch {
    return iso;
  }
}
