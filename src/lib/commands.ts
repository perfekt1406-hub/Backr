/*
 * Purpose: Typed wrappers around every Backr `invoke` command.
 * Role: Centralizes IPC strings and keeps components free of raw command names.
 */

import { invoke } from "@tauri-apps/api/core";

import type { ActivityPoint } from "../types/activity";
import type { BackupStatus, ProjectInfo } from "../types/project";
import type { Config } from "../types/config";
import type { FileEntry, SnapshotEntry, SnapshotFileContents, RestoreEveryProjectRow } from "../types/snapshot";
import * as devMockBackend from "./devMock/backend";
import { useDevMock } from "./useDevMock";

/**
 * Loads persisted configuration from managed state (null before first save).
 *
 * External: `invoke` dispatches to Rust `get_config` returning `Option<Config>`.
 */
export async function getConfig(): Promise<Config | null> {
  if (useDevMock()) {
    return devMockBackend.mockGetConfig();
  }
  return invoke<Config | null>("get_config");
}

/**
 * Persists configuration and restarts the scheduler on the Rust side.
 *
 * External: `invoke` → `save_config(next)`; errors surface as thrown strings.
 */
export async function saveConfig(next: Config): Promise<void> {
  if (useDevMock()) {
    await devMockBackend.mockSaveConfig(next);
    return;
  }
  await invoke("save_config", { next });
}

/**
 * SSH echo probe using supplied credentials (paths expanded server-side where applicable).
 *
 * External: `invoke` → `test_connection(host, user, key_path, ssh_port)`.
 */
export async function testConnection(
  host: string,
  user: string,
  keyPath: string,
  sshPort?: number,
): Promise<void> {
  if (useDevMock()) {
    void host;
    void user;
    void keyPath;
    void sshPort;
    await devMockBackend.mockTestConnection();
    return;
  }
  await invoke("test_connection", {
    host,
    user,
    keyPath,
    sshPort: sshPort ?? null,
  });
}

/**
 * Lists immediate child directories of `local.projects_path`.
 *
 * External: `invoke` → `list_projects`.
 */
export async function listProjects(): Promise<ProjectInfo[]> {
  if (useDevMock()) {
    return devMockBackend.mockListProjects();
  }
  return invoke<ProjectInfo[]>("list_projects");
}

/**
 * Reads mutex + schedule metadata for status badges.
 *
 * External: `invoke` → `get_backup_status`.
 */
export async function getBackupStatus(): Promise<BackupStatus> {
  if (useDevMock()) {
    return devMockBackend.mockGetBackupStatus();
  }
  return invoke<BackupStatus>("get_backup_status");
}

/**
 * Schedules a backup worker (all projects when `project` omitted).
 *
 * External: `invoke` → `run_backup`; rejects when another job holds the lock.
 */
export async function runBackup(project?: string): Promise<void> {
  if (useDevMock()) {
    await devMockBackend.mockRunBackup(project);
    return;
  }
  await invoke("run_backup", { project: project ?? null });
}

/**
 * Remote snapshot folder names for one project, newest-first.
 *
 * External: `invoke` → `list_snapshots`.
 */
export async function listSnapshots(project: string): Promise<SnapshotEntry[]> {
  if (useDevMock()) {
    return devMockBackend.mockListSnapshots(project);
  }
  return invoke<SnapshotEntry[]>("list_snapshots", { project });
}

/**
 * Immediate children under `path` inside a snapshot (use "" for roots).
 *
 * External: `invoke` → `list_files`.
 */
export async function listFiles(
  project: string,
  snapshot: string,
  path: string,
): Promise<FileEntry[]> {
  if (useDevMock()) {
    return devMockBackend.mockListFiles(project, snapshot, path);
  }
  return invoke<FileEntry[]>("list_files", { project, snapshot, path });
}

/**
 * Reads a bounded UTF-8 preview (`head -c` server-side) for monospace rendering.
 *
 * External: `invoke` → `read_snapshot_file`; binary/non-UTF-8 payloads surface as errors.
 */
export async function readSnapshotFile(
  project: string,
  snapshot: string,
  relativePath: string,
): Promise<SnapshotFileContents> {
  if (useDevMock()) {
    return devMockBackend.mockReadSnapshotFile(project, snapshot, relativePath);
  }
  return invoke<SnapshotFileContents>("read_snapshot_file", {
    project,
    snapshot,
    relativePath,
  });
}

/**
 * Rsync-restore an entire snapshot under home (`~/Projects-<id>` or stamped basename if needed), with `-1`, `-2`, … collision suffixes.
 *
 * External: `invoke` → `restore_snapshot`; returns destination path string.
 */
export async function restoreSnapshot(
  project: string,
  snapshot: string,
): Promise<string> {
  if (useDevMock()) {
    return devMockBackend.mockRestoreSnapshot(project, snapshot);
  }
  return invoke<string>("restore_snapshot", { project, snapshot });
}

/**
 * Restores every indexed snapshot for `project` sequentially (newest first).
 *
 * External: `invoke` → `restore_all_snapshots`; returns one destination path per snapshot.
 */
export async function restoreAllSnapshots(project: string): Promise<string[]> {
  if (useDevMock()) {
    return devMockBackend.mockRestoreAllSnapshots(project);
  }
  return invoke<string[]>("restore_all_snapshots", { project });
}

/**
 * Restores every valid snapshot for each local project folder (lex order), skipping empty remotes.
 *
 * External: `invoke` → `restore_all_projects`; returns per-project destination lists.
 */
export async function restoreAllProjects(): Promise<RestoreEveryProjectRow[]> {
  if (useDevMock()) {
    return devMockBackend.mockRestoreAllProjects();
  }
  return invoke<RestoreEveryProjectRow[]>("restore_all_projects");
}

/**
 * Sparse markers derived from `[state].last_backup_at` for the dashboard strip.
 *
 * External: `invoke` → `get_activity_series`.
 */
export async function getActivitySeries(): Promise<ActivityPoint[]> {
  if (useDevMock()) {
    return devMockBackend.mockGetActivitySeries();
  }
  return invoke<ActivityPoint[]>("get_activity_series");
}
