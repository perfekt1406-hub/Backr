/*
 * Purpose: Dashboard rows reflecting immediate children of `local.projects_path`.
 * Role: `refreshProjects` loads cached snapshot stats only; `refreshProjectsRemote` probes SSH when online.
 */

import { writable, type Writable } from "svelte/store";

import * as commands from "../lib/commands";
import type { ProjectInfo } from "../types/project";
import { handleCommandError } from "./ui";

/** Cached sort order follows backend lexicographic ordering. */
export const projects: Writable<ProjectInfo[]> = writable([]);

/**
 * Reloads project folders plus snapshot stats from local disk cache only (no SSH).
 *
 * External: `commands.listProjects(false)` merges folders under `projects_path` with `snapshot_stats.json`.
 */
export async function refreshProjects(): Promise<void> {
  try {
    const rows = await commands.listProjects(false);
    projects.set(rows);
  } catch (err) {
    handleCommandError(err);
  }
}

/**
 * Probes the backup server over SSH and refreshes per-project snapshot counts when reachable.
 *
 * External: `commands.listProjects(true)` falls back to cache entries when SSH fails mid-listing.
 */
export async function refreshProjectsRemote(): Promise<void> {
  try {
    const rows = await commands.listProjects(true);
    projects.set(rows);
  } catch (err) {
    handleCommandError(err);
  }
}
