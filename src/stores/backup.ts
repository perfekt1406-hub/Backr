/*
 * Purpose: Backup lifecycle indicators plus scrolling rsync console output.
 * Role: Feeds dashboard controls and subscribes to `backup://progress` tap events.
 */

import { writable, type Writable } from "svelte/store";

import * as commands from "../lib/commands";
import type { BackupStatus } from "../types/project";
import { showToast } from "./ui";

/** Mutex / schedule snapshot from Rust `AppState`. */
export const backupStatus: Writable<BackupStatus | null> = writable(null);

/** Bounded FIFO of textual rsync lines for the dashboard console. */
export const progressLog: Writable<string[]> = writable([]);

/** Maximum retained console lines before truncation from the head. */
const PROGRESS_CAP = 500;

/**
 * Polls `get_backup_status` for spinner accuracy across tray + UI triggers.
 *
 * External: `commands.getBackupStatus` joins persisted cadence with atomic flags.
 */
export async function refreshBackupStatus(): Promise<void> {
  try {
    const status = await commands.getBackupStatus();
    backupStatus.set(status);
  } catch (err) {
    backupStatus.set(null);
    showToast(String(err));
  }
}

/**
 * Queues on-demand backup work for one project or the entire fleet.
 *
 * External: `commands.runBackup` schedules Tokio work after mutex acquisition.
 */
export async function requestBackup(project?: string): Promise<void> {
  try {
    await commands.runBackup(project);
    await refreshBackupStatus();
  } catch (err) {
    showToast(String(err));
  }
}

/**
 * Appends rsync output lines emitted through `listenBackupProgress`.
 *
 * External: mutates the local store only (no IPC).
 */
export function appendProgressLine(line: string): void {
  progressLog.update((prev) => [...prev.slice(-PROGRESS_CAP), line]);
}

/** Clears the scrolling rsync console after inspection. */
export function clearProgressLog(): void {
  progressLog.set([]);
}
