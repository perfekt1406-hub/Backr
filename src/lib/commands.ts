/*
 * Purpose: Typed wrappers around every Backr `invoke` command.
 * Role: Centralizes IPC strings and keeps components free of raw command names.
 */

import { invoke } from "@tauri-apps/api/core";

import type { ActivityPoint } from "../types/activity";
import type { BackupStatus, ProjectInfo } from "../types/project";
import type { Config } from "../types/config";
import type { FileEntry, SnapshotEntry, SnapshotFileContents, RestoreEveryProjectRow } from "../types/snapshot";
import type {
  HostDiskInventory,
  HostProjectRow,
  HostVolumeSummary,
} from "../types/hostDashboard";
import type { AuthorizedPubkeyEntry, HostRemovePubkeyResult, HostTrustAppendResult, HostTrustStatus } from "../types/hostTrust";
import type { DiscoveredHost, PairDraft, PairingStarted } from "../types/pairing";
import type { ShellBootstrap } from "../types/shellBootstrap";
import type { SystemInfo } from "../types/systemInfo";
import type { UpdateStatus } from "../types/update";
import * as devMockBackend from "./devMock/backend";
import { useDevMock } from "./useDevMock";

/**
 * Resolves whether the UI should open setup, laptop client, or backup-host dashboard.
 *
 * External: `invoke` → `resolve_shell_bootstrap`.
 */
export async function resolveShellBootstrap(): Promise<ShellBootstrap> {
  if (useDevMock()) {
    return devMockBackend.mockResolveShellBootstrap();
  }
  return invoke<ShellBootstrap>("resolve_shell_bootstrap");
}

/**
 * Returns the daemon-connectivity error recorded at GUI startup, or null when the
 * daemon was reachable. Used by the shell to show a clear "backrd unreachable"
 * screen instead of a broken half-state when IPC calls fail.
 *
 * External: `invoke` → `get_daemon_error` returning `Option<String>`.
 */
export async function getDaemonError(): Promise<string | null> {
  if (useDevMock()) {
    return null;
  }
  return invoke<string | null>("get_daemon_error");
}

/**
 * Lists local snapshot directories under `backup_root` on this machine (host dashboard).
 *
 * External: `invoke` → `host_list_snapshot_projects`.
 */
export async function hostListSnapshotProjects(backupRoot: string): Promise<HostProjectRow[]> {
  if (useDevMock()) {
    return devMockBackend.mockHostListSnapshotProjects();
  }
  return invoke<HostProjectRow[]>("host_list_snapshot_projects", { backupRoot });
}

/**
 * Reports coarse disk usage for the filesystem backing `backup_root` via `df`.
 *
 * External: `invoke` → `host_volume_summary`.
 */
export async function hostVolumeSummary(backupRoot: string): Promise<HostVolumeSummary> {
  if (useDevMock()) {
    return devMockBackend.mockHostVolumeSummary(backupRoot);
  }
  return invoke<HostVolumeSummary>("host_volume_summary", { backupRoot });
}

/**
 * Measures backup-tree directory sizes via `du` with TTL JSON cache under `~/.config/backr/host_du_cache.json`.
 *
 * External: `invoke` → `host_disk_inventory`; long scans run in a blocking worker on the Rust side.
 *
 * @param forceRefresh When true, ignores cache TTL and attempts a fresh scan (still falls back to stale cache if `du` fails).
 */
export async function hostDiskInventory(
  backupRoot: string,
  forceRefresh = false,
): Promise<HostDiskInventory> {
  if (useDevMock()) {
    return devMockBackend.mockHostDiskInventory(backupRoot, forceRefresh);
  }
  return invoke<HostDiskInventory>("host_disk_inventory", {
    backupRoot,
    forceRefresh,
  });
}

/**
 * Reads authorized_keys stats for the backup-host Trust page.
 *
 * External: `invoke` → `host_trust_status`.
 */
export async function hostTrustStatus(): Promise<HostTrustStatus> {
  if (useDevMock()) {
    return devMockBackend.mockHostTrustStatus();
  }
  return invoke<HostTrustStatus>("host_trust_status");
}

/**
 * Lists all parsed pubkey entries in authorized_keys for the host Settings trusted-keys panel.
 *
 * External: `invoke` → `host_list_authorized_pubkeys`.
 */
export async function hostListAuthorizedPubkeys(): Promise<AuthorizedPubkeyEntry[]> {
  if (useDevMock()) {
    return devMockBackend.mockHostListAuthorizedPubkeys();
  }
  return invoke<AuthorizedPubkeyEntry[]>("host_list_authorized_pubkeys");
}

/**
 * Removes the pubkey line matching `rawLine` exactly from authorized_keys.
 *
 * External: `invoke` → `host_remove_authorized_pubkey`.
 */
export async function hostRemoveAuthorizedPubkey(rawLine: string): Promise<HostRemovePubkeyResult> {
  if (useDevMock()) {
    return devMockBackend.mockHostRemoveAuthorizedPubkey(rawLine);
  }
  return invoke<HostRemovePubkeyResult>("host_remove_authorized_pubkey", { rawLine });
}

/**
 * Appends one pubkey line from the Trust UI (or returns sudo fallback text).
 *
 * External: `invoke` → `host_append_authorized_pubkey`.
 */
export async function hostAppendAuthorizedPubkey(pubkeyLine: string): Promise<HostTrustAppendResult> {
  if (useDevMock()) {
    return devMockBackend.mockHostAppendAuthorizedPubkey(pubkeyLine);
  }
  // Tauri exposes the Rust `pubkey_line` arg to JS as camelCase `pubkeyLine`.
  return invoke<HostTrustAppendResult>("host_append_authorized_pubkey", {
    pubkeyLine,
  });
}

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
 * Reads hostname, distro/OS label, kernel, architecture, user, and sample clock from this machine.
 *
 * External: `invoke` → `get_system_info`.
 */
export async function getSystemInfo(): Promise<SystemInfo> {
  if (useDevMock()) {
    return devMockBackend.mockGetSystemInfo();
  }
  return invoke<SystemInfo>("get_system_info");
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
 *
 * @param probeRemote When true, probes SSH and refreshes local snapshot stats cache; when false (default), uses only local disk cache so the dashboard works without LAN reachability.
 */
export async function listProjects(probeRemote = false): Promise<ProjectInfo[]> {
  if (useDevMock()) {
    return devMockBackend.mockListProjects();
  }
  // Tauri exposes the Rust `probe_remote` arg to JS as camelCase `probeRemote`.
  return invoke<ProjectInfo[]>("list_projects", { probeRemote });
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

/**
 * Reports the running version and whether a newer release is available.
 *
 * External: `invoke` → `get_update_status` (daemon hits the GitHub Releases API).
 */
export async function getUpdateStatus(): Promise<UpdateStatus> {
  if (useDevMock()) {
    return { current_version: "0.1.0", latest_version: "0.1.0", update_available: false };
  }
  return invoke<UpdateStatus>("get_update_status");
}

/**
 * Asks the daemon to apply the latest update. Returns immediately — the daemon
 * launches an out-of-process worker that swaps binaries and restarts the daemon.
 *
 * External: `invoke` → `apply_update`.
 */
export async function applyUpdate(): Promise<void> {
  if (useDevMock()) {
    return;
  }
  await invoke("apply_update");
}

/**
 * Returns whether automatic updates are enabled.
 *
 * External: `invoke` → `get_update_settings`.
 */
export async function getUpdateSettings(): Promise<boolean> {
  if (useDevMock()) {
    return false;
  }
  const r = await invoke<{ auto_update: boolean }>("get_update_settings");
  return r.auto_update;
}

/**
 * Enables or disables automatic updates; returns the persisted value.
 *
 * External: `invoke` → `set_update_settings(auto_update)`.
 */
export async function setUpdateSettings(autoUpdate: boolean): Promise<boolean> {
  if (useDevMock()) {
    return autoUpdate;
  }
  const r = await invoke<{ auto_update: boolean }>("set_update_settings", { autoUpdate });
  return r.auto_update;
}

/**
 * Host: opens a one-tap pairing window (6-digit code + mDNS advertise + listener).
 *
 * External: `invoke` → `start_pairing`.
 */
export async function startPairing(): Promise<PairingStarted> {
  if (useDevMock()) {
    return devMockBackend.mockStartPairing();
  }
  return invoke<PairingStarted>("start_pairing");
}

/**
 * Host: closes the pairing window if one is open.
 *
 * External: `invoke` → `stop_pairing`.
 */
export async function stopPairing(): Promise<void> {
  if (useDevMock()) {
    await devMockBackend.mockStopPairing();
    return;
  }
  await invoke("stop_pairing");
}

/**
 * Host: reports whether a pairing window is currently open.
 *
 * External: `invoke` → `pairing_status`.
 */
export async function pairingStatus(): Promise<boolean> {
  if (useDevMock()) {
    return devMockBackend.mockPairingStatus();
  }
  return invoke<boolean>("pairing_status");
}

/**
 * Client: browses the LAN for hosts currently in pairing mode.
 *
 * External: `invoke` → `discover_hosts`.
 */
export async function discoverHosts(): Promise<DiscoveredHost[]> {
  if (useDevMock()) {
    return devMockBackend.mockDiscoverHosts();
  }
  return invoke<DiscoveredHost[]>("discover_hosts");
}

/**
 * Client: pairs with a discovered host using the 6-digit code; returns a `PairDraft`
 * containing the prefilled config AND the host's SSH key fingerprint for out-of-band
 * verification. Call `confirmPairing` after the user confirms the fingerprint.
 *
 * External: `invoke` → `pair_with_host`.
 */
export async function pairWithHost(address: string, code: string): Promise<PairDraft> {
  if (useDevMock()) {
    return devMockBackend.mockPairWithHost(address, code);
  }
  return invoke<PairDraft>("pair_with_host", { address, code });
}

/**
 * Client: finalizes a confirmed pair by pinning the host key and returning the
 * ready-to-save config. Call this only after the user has verified the fingerprint
 * shown in the UI matches what is displayed on the host screen.
 *
 * # Inputs
 *
 * * `draft` — the `PairDraft` returned by `pairWithHost`.
 *
 * External: `invoke` → `confirm_pairing`.
 */
export async function confirmPairing(draft: PairDraft): Promise<Config> {
  if (useDevMock()) {
    return devMockBackend.mockConfirmPairing(draft);
  }
  return invoke<Config>("confirm_pairing", { draft });
}
