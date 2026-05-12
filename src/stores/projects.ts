/*
 * Purpose: Dashboard rows reflecting immediate children of `local.projects_path`.
 * Role: `refreshProjects` re-queries SSH-backed snapshot metadata through IPC.
 */

import { writable, type Writable } from "svelte/store";

import * as commands from "../lib/commands";
import type { ProjectInfo } from "../types/project";
import { showToast } from "./ui";

/** Cached sort order follows backend lexicographic ordering. */
export const projects: Writable<ProjectInfo[]> = writable([]);

/**
 * Reloads project folders plus remote snapshot summaries.
 *
 * External: `commands.listProjects` performs filesystem enumeration + SSH probes.
 */
export async function refreshProjects(): Promise<void> {
  try {
    const rows = await commands.listProjects();
    projects.set(rows);
  } catch (err) {
    showToast(String(err));
  }
}
