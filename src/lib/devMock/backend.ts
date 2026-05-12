/*
 * Purpose: In-memory stand-in for Rust Tauri commands when `useDevMock()` is active.
 * Role: Supplies deterministic data for UI QA — filesystem trees, mutex simulation, config edits.
 */

import type { ActivityPoint } from "../../types/activity";
import type { Config } from "../../types/config";
import type { BackupStatus, ProjectInfo } from "../../types/project";
import type {
  FileEntry,
  SnapshotEntry,
  SnapshotFileContents,
  RestoreEveryProjectRow,
} from "../../types/snapshot";
import { emitMockProgressLine } from "../mockProgressSink";
import {
  createInitialMockConfig,
  mockChildrenAt,
  MOCK_PROJECT_ROWS,
  MOCK_SNAPSHOT_NAMES,
  mockSnapshotFileContents,
  mockSnapshotsForProject,
  seedActivityPoints,
} from "./fixtures";

/** Mutable config snapshot — `save_config` mock writes here only. */
let mockConfig: Config = structuredClone(createInitialMockConfig());

/** Mutex aligned with production error string when overlapping backups fire. */
let backupRunning = false;

/** Active synthetic job label for status badges. */
let activeProject: string | null = null;

/** Rolling markers feeding `get_activity_series`. */
let activityHistory: ActivityPoint[] = [...seedActivityPoints()];

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms);
  });
}

/**
 * Resets mutable mock state — reserved for tests or future debug controls.
 *
 * External: mutates module-level configuration clones only.
 */
export function resetDevMockState(): void {
  mockConfig = structuredClone(createInitialMockConfig());
  backupRunning = false;
  activeProject = null;
  activityHistory = [...seedActivityPoints()];
}

/** Returns optional persisted config — mock mode always supplies a concrete document. */
export function mockGetConfig(): Config | null {
  return structuredClone(mockConfig);
}

/**
 * Pretends to persist configuration (memory only).
 *
 * External: mirrors Rust `save_config` side-effect surface without touching disk.
 */
export async function mockSaveConfig(next: Config): Promise<void> {
  await delay(120);
  mockConfig = structuredClone(next);
}

/** Always succeeds — no SSH probe in mock mode. */
export async function mockTestConnection(): Promise<void> {
  await delay(200);
}

/** Static dashboard listing independent of local filesystem. */
export async function mockListProjects(): Promise<ProjectInfo[]> {
  await delay(80);
  return structuredClone(MOCK_PROJECT_ROWS);
}

/** Scheduler/mutex snapshot synthesized from `mockConfig` plus runtime flags. */
export async function mockGetBackupStatus(): Promise<BackupStatus> {
  await delay(40);
  const last = mockConfig.state.last_backup_at;
  const hours = mockConfig.schedule.interval_hours;
  const nextBackup =
    last != null
      ? new Date(new Date(last).getTime() + hours * 3600_000).toISOString()
      : new Date(Date.now() + hours * 3600_000).toISOString();

  return {
    last_backup_at: last,
    next_backup_at: nextBackup,
    in_progress: backupRunning,
    active_project: activeProject,
  };
}

/**
 * Simulates rsync progress lines and advances `[state].last_backup_at` on success.
 *
 * External: uses `emitMockProgressLine` to mirror `backup://progress` emissions.
 */
export async function mockRunBackup(project?: string): Promise<void> {
  if (backupRunning) {
    throw new Error("a backup is already in progress");
  }
  backupRunning = true;
  activeProject = project ?? null;

  const scope = project ?? "all projects";
  emitMockProgressLine(`[mock backr] scheduling snapshot for ${scope}`);

  try {
    await delay(350);
    emitMockProgressLine(
      "[mock rsync] sending incremental file list… done (mock throughput ~120 MB/s)",
    );
    await delay(400);
    emitMockProgressLine(
      `[mock rsync] ${project ? `${project}/` : ""}2026-05-11_15-00-00/`,
    );
    await delay(450);
    emitMockProgressLine("[mock backr] wrote snapshot metadata");
    const now = new Date().toISOString();
    mockConfig = {
      ...mockConfig,
      state: { ...mockConfig.state, last_backup_at: now },
    };
    activityHistory = [
      ...activityHistory.slice(-20),
      { ts_unix: Math.floor(Date.now() / 1000), label: "backup_complete" },
    ];
    emitMockProgressLine("[mock backr] backup completed successfully");
  } finally {
    backupRunning = false;
    activeProject = null;
  }
}

/** Lists synthetic snapshots for arbitrary project slugs. */
export async function mockListSnapshots(project: string): Promise<SnapshotEntry[]> {
  await delay(100);
  void project;
  return mockSnapshotsForProject(project);
}

/** Directory listing backed by `fixtures.mockChildrenAt`. */
export async function mockListFiles(
  _project: string,
  _snapshot: string,
  path: string,
): Promise<FileEntry[]> {
  await delay(70);
  void _project;
  void _snapshot;
  const normalized = path.trim().replace(/^\/+|\/+$/g, "");
  return structuredClone(mockChildrenAt(normalized));
}

/** UTF-8 preview slice mirroring remote `head -c` without SSH in mock mode. */
export async function mockReadSnapshotFile(
  _project: string,
  _snapshot: string,
  relativePath: string,
): Promise<SnapshotFileContents> {
  await delay(90);
  void _project;
  void _snapshot;
  return mockSnapshotFileContents(relativePath);
}

/** Returns a fake restore destination — no files are written. */
export async function mockRestoreSnapshot(
  _project: string,
  snapshot: string,
): Promise<string> {
  await delay(400);
  emitMockProgressLine(`[mock rsync] restore pull for ${snapshot} (dry-run)`);
  const home = "/home/dev";
  return `${home}/Projects-${snapshot}-mock`;
}

/**
 * Simulates bulk restore — one fake path per mock snapshot, newest-first (matches production order).
 *
 * External: reads `MOCK_SNAPSHOT_NAMES`; emits progress lines only.
 */
export async function mockRestoreAllSnapshots(_project: string): Promise<string[]> {
  void _project;
  const home = "/home/dev";
  const ordered = [...MOCK_SNAPSHOT_NAMES].sort((a, b) => b.localeCompare(a));
  const paths: string[] = [];
  for (const snapshot of ordered) {
    await delay(220);
    emitMockProgressLine(`[mock rsync] restore pull for ${snapshot} (dry-run, bulk)`);
    paths.push(`${home}/Projects-${snapshot}-mock`);
  }
  return paths;
}

/**
 * Simulates `restore_all_projects` — one bulk mock restore per dashboard row with snapshots.
 *
 * External: skips `MOCK_PROJECT_ROWS` where `snapshot_count === 0`; uses [`mockRestoreAllSnapshots`] per project.
 */
export async function mockRestoreAllProjects(): Promise<RestoreEveryProjectRow[]> {
  await delay(200);
  emitMockProgressLine("[mock backr] restore all projects (dry-run)");
  const sorted = [...MOCK_PROJECT_ROWS].sort((a, b) => a.name.localeCompare(b.name));
  const rows: RestoreEveryProjectRow[] = [];
  for (const row of sorted) {
    if (row.snapshot_count === 0) {
      continue;
    }
    const destinations = await mockRestoreAllSnapshots(row.name);
    rows.push({ project: row.name, destinations });
  }
  return rows;
}

/** Activity markers accumulated across simulated backup cycles. */
export async function mockGetActivitySeries(): Promise<ActivityPoint[]> {
  await delay(40);
  return structuredClone(activityHistory);
}
