/*
 * Purpose: Memoizes lazy directory listings for snapshot browsing.
 * Role: Avoids redundant `list_files` round trips while navigating expanded folders.
 */

import { writable, type Writable } from "svelte/store";

import type { FileEntry } from "../types/snapshot";

/** Stable tuple key for cached fetch results. */
export function filesCacheKey(
  project: string,
  snapshot: string,
  path: string,
): string {
  return `${project}|${snapshot}|${path}`;
}

/** Flat cache mapping composite keys to immediate children arrays. */
export const filesCache: Writable<Map<string, FileEntry[]>> = writable(new Map());

/** Drops memoized entries after restores or manual refreshes. */
export function clearFilesCache(): void {
  filesCache.set(new Map());
}
