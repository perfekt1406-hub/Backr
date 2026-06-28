/*
 * Purpose: In-memory stand-in for Rust Tauri commands when `useDevMock()` is active.
 * Role: Supplies deterministic data for UI QA — filesystem trees, mutex simulation, config edits.
 */

import type { ActivityPoint } from "../../types/activity";
import type {
  HostDiskInventory,
  HostProjectRow,
  HostVolumeSummary,
} from "../../types/hostDashboard";
import type { AuthorizedPubkeyEntry, HostRemovePubkeyResult, HostTrustAppendResult, HostTrustStatus } from "../../types/hostTrust";
import type { Config } from "../../types/config";
import type { DiscoveredHost, PairDraft, PairingStarted } from "../../types/pairing";
import type { BackupStatus, ProjectInfo } from "../../types/project";
import type { ShellBootstrap } from "../../types/shellBootstrap";
import type { SystemInfo } from "../../types/systemInfo";
import type {
  FileEntry,
  SnapshotEntry,
  SnapshotFileContents,
  RestoreEveryProjectRow,
} from "../../types/snapshot";
import {
  DEV_MOCK_HOST_BACKUP_ROOT,
  DEV_MOCK_HOST_SSH_USER,
  getDevShellKindPreference,
} from "../devShellPreference";
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

/** Simulated authorized_keys line count for host Trust-keys IPC mocks. */
let mockTrustPubkeyLineCount = 0;

/** When true, the host dashboard mock reports no snapshots (first-run / not-paired preview). */
let mockHostFirstRun = false;

/** Dev switcher toggles the host first-run (no backups, not paired) preview state. */
export function setMockHostFirstRun(firstRun: boolean): void {
  mockHostFirstRun = firstRun;
  if (firstRun) {
    mockTrustPubkeyLineCount = 0; // show the un-paired state
  }
}

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
  mockTrustPubkeyLineCount = 0;
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

/** Fake [`SystemInfo`] for dashboard chrome when IPC is mocked in the browser. */
export async function mockGetSystemInfo(): Promise<SystemInfo> {
  await delay(35);
  return {
    hostname: "mock-workstation",
    os_pretty: "Linux (mock dev browser)",
    kernel_release: "6.12.x-generic",
    arch: "x86_64",
    user: "dev",
    sampled_at_rfc3339: new Date().toISOString(),
  };
}

/**
 * Mock bootstrap — browser dev mock always behaves as a configured laptop client.
 *
 * External: mirrors `resolve_shell_bootstrap` IPC shape for local-only previews.
 */
export async function mockResolveShellBootstrap(): Promise<ShellBootstrap> {
  await delay(15);
  if (getDevShellKindPreference() === "host") {
    return {
      mode: "host",
      backup_root: DEV_MOCK_HOST_BACKUP_ROOT,
      ssh_user: DEV_MOCK_HOST_SSH_USER,
    };
  }
  return { mode: "client" };
}

/**
 * Synthetic NAS snapshot folders aligned with [`MOCK_PROJECT_ROWS`] names where snapshots exist.
 *
 * External: reads snapshot name fixtures so browsing mocks stay consistent.
 */
export async function mockHostListSnapshotProjects(): Promise<HostProjectRow[]> {
  await delay(30);
  if (mockHostFirstRun) {
    return []; // first-run preview → HostDashboardView shows HostSetupGuide
  }
  const orderedSnaps = [...MOCK_SNAPSHOT_NAMES].sort((a, b) => b.localeCompare(a));
  const recentForCount = (n: number): string[] =>
    n <= 0 ? [] : orderedSnaps.slice(0, Math.min(3, orderedSnaps.length));
  return MOCK_PROJECT_ROWS.map((p) => ({
    name: p.name,
    snapshot_count: p.snapshot_count,
    last_backup_at: p.last_backup_at,
    recent_snapshots: recentForCount(p.snapshot_count),
  }));
}

/** Synthetic volume telemetry mirroring enriched GNU `df` fields for UI previews (browser has no real `df`). */
export async function mockHostVolumeSummary(backupRoot: string): Promise<HostVolumeSummary> {
  await delay(15);
  const bytes_size = 5_000_000_000_000;
  const bytes_avail = 480_000_000_000;
  const used = bytes_size - bytes_avail;
  return {
    backup_root: backupRoot,
    bytes_avail,
    bytes_size,
    filesystem_source: "/dev/md127",
    mount_point: "/srv/backr-host",
    used_bytes: used,
    used_percent: `${Math.round((used / bytes_size) * 100)}%`,
  };
}

/**
 * Synthetic `du` inventory totals sized roughly against dashboard rows that carry snapshots.
 *
 * External: when `forceRefresh` is true, uses a longer delay to mimic rescan latency.
 */
export async function mockHostDiskInventory(
  backupRoot: string,
  forceRefresh: boolean,
): Promise<HostDiskInventory> {
  await delay(forceRefresh ? 280 : 50);
  const sized = MOCK_PROJECT_ROWS.filter((p) => p.snapshot_count > 0).map((p, i) => ({
    name: p.name,
    bytes: (i + 1) * 1_234_567_890,
  }));
  const sumProjects = sized.reduce((acc, row) => acc + row.bytes, 0);
  const overhead = Math.round(sumProjects * 0.02);
  return {
    backup_root: backupRoot,
    backup_root_bytes: sumProjects + overhead,
    projects: sized,
    from_cache: !forceRefresh,
    scanned_at: new Date().toISOString(),
  };
}

/**
 * Mock Trust-keys status for the backup-host dashboard in dev mock mode.
 *
 * External: mirrors `host_trust_status` IPC without reading disk.
 */
export async function mockHostTrustStatus(): Promise<HostTrustStatus> {
  await delay(40);
  return {
    ssh_user: DEV_MOCK_HOST_SSH_USER,
    authorized_keys_path: `/home/${DEV_MOCK_HOST_SSH_USER}/.ssh/authorized_keys`,
    pubkey_line_count: mockTrustPubkeyLineCount,
  };
}

/**
 * Mock Trust-keys append — accepts plausible OpenSSH key prefixes.
 *
 * External: mirrors `host_append_authorized_pubkey` IPC shape; updates [`mockTrustPubkeyLineCount`].
 */
export async function mockHostAppendAuthorizedPubkey(
  pubkeyLine: string,
): Promise<HostTrustAppendResult> {
  await delay(120);
  const line = pubkeyLine.trim();
  const looksOk =
    /^(ssh-rsa|ssh-ed25519|ssh-dss|ecdsa-sha2-nistp256|ecdsa-sha2-nistp384|ecdsa-sha2-nistp521|sk-ssh-ed25519|sk-ecdsa-sha2-nistp256)\s+\S/.test(
      line,
    );
  if (!looksOk) {
    return {
      appended: false,
      skipped_duplicate: false,
      pubkey_line_count: mockTrustPubkeyLineCount,
      message: "Invalid OpenSSH public key line (mock)",
    };
  }
  mockTrustPubkeyLineCount += 1;
  return {
    appended: true,
    skipped_duplicate: false,
    pubkey_line_count: mockTrustPubkeyLineCount,
    message: "Appended (mock)",
  };
}

/** Dev mock authorized_keys entries — seeded with two fake laptops for settings preview. */
let mockAuthorizedPubkeys: AuthorizedPubkeyEntry[] = [
  {
    key_type: "ssh-ed25519",
    key_b64: "AAAAC3NzaC1lZDI1NTE5AAAAIMockKeyDataAlpha",
    comment: "alice@macbook-pro",
    raw_line: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMockKeyDataAlpha alice@macbook-pro",
  },
  {
    key_type: "ssh-ed25519",
    key_b64: "AAAAC3NzaC1lZDI1NTE5AAAAIMockKeyDataBeta",
    comment: "bob@thinkpad",
    raw_line: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMockKeyDataBeta bob@thinkpad",
  },
];

/**
 * Mock — lists current authorized_keys entries for host Settings.
 *
 * External: mirrors `host_list_authorized_pubkeys` IPC without reading disk.
 */
export async function mockHostListAuthorizedPubkeys(): Promise<AuthorizedPubkeyEntry[]> {
  await delay(40);
  return [...mockAuthorizedPubkeys];
}

/**
 * Mock — removes the matching raw_line from the in-memory key list.
 *
 * External: mirrors `host_remove_authorized_pubkey` IPC without writing disk.
 */
export async function mockHostRemoveAuthorizedPubkey(
  rawLine: string,
): Promise<HostRemovePubkeyResult> {
  await delay(80);
  const before = mockAuthorizedPubkeys.length;
  mockAuthorizedPubkeys = mockAuthorizedPubkeys.filter((e) => e.raw_line !== rawLine);
  mockTrustPubkeyLineCount = mockAuthorizedPubkeys.length;
  return {
    removed: mockAuthorizedPubkeys.length < before,
    pubkey_line_count: mockAuthorizedPubkeys.length,
  };
}

/** Simulated host pairing window: open until ~8s elapse (mimics a laptop pairing) or stopped. */
let mockPairingOpenedAt: number | null = null;

/** Host: opens a pairing window with a fixed demo code. */
export async function mockStartPairing(): Promise<PairingStarted> {
  await delay(120);
  mockPairingOpenedAt = Date.now();
  return { code: "482913" };
}

/** Host: closes the pairing window. */
export async function mockStopPairing(): Promise<void> {
  await delay(40);
  mockPairingOpenedAt = null;
}

/** Host: the window auto-"pairs" ~8s after opening so the panel demos the paired state. */
export async function mockPairingStatus(): Promise<boolean> {
  await delay(20);
  if (mockPairingOpenedAt == null) {
    return false;
  }
  if (Date.now() - mockPairingOpenedAt > 8000) {
    mockPairingOpenedAt = null; // simulate a laptop having paired
    mockTrustPubkeyLineCount += 1; // its key is now trusted
    return false;
  }
  return true;
}

/** Client: two fake hosts in pairing mode. */
export async function mockDiscoverHosts(): Promise<DiscoveredHost[]> {
  await delay(600);
  return [
    { hostname: "mock-nas", address: "192.168.1.50:8421" },
    { hostname: "backr-host", address: "192.168.1.77:8421" },
  ];
}

/** Client: pretends to pair and returns a PairDraft with a fake fingerprint. */
export async function mockPairWithHost(address: string, code: string): Promise<PairDraft> {
  void code;
  await delay(500);
  const host = address.split(":")[0] ?? "192.168.1.50";
  const cfg = structuredClone(createInitialMockConfig());
  cfg.remote.host = host;
  cfg.remote.user = DEV_MOCK_HOST_SSH_USER;
  cfg.remote.backup_path = DEV_MOCK_HOST_BACKUP_ROOT;
  return {
    config: cfg,
    host_key_fingerprint: "SHA256:mockFingerprintABCDEFGHIJKLMNOPQRSTUVWXYZ01",
    host_pubkey: `ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIMockHostKey mock-host`,
    ssh_target: host,
  };
}

/** Client: confirms a pair draft and returns the finalized config. */
export async function mockConfirmPairing(draft: PairDraft): Promise<Config> {
  await delay(200);
  return draft.config;
}
