/*
 * Purpose: DTOs for backup-host dashboard IPC (`host_list_snapshot_projects`, `host_volume_summary`).
 * Role: Mirrors Rust structs serialized over Tauri invoke for NAS-local snapshot browsing.
 */

export type HostProjectRow = {
  name: string;
  snapshot_count: number;
  last_backup_at: string | null;
};

export type HostVolumeSummary = {
  backup_root: string;
  bytes_avail: number | null;
  bytes_size: number | null;
};
