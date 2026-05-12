/*
 * Purpose: DTOs for backup-host dashboard IPC (`host_list_snapshot_projects`, `host_volume_summary`, `host_disk_inventory`).
 * Role: Mirrors Rust structs serialized over Tauri invoke for NAS-local snapshot browsing.
 */

export type HostProjectRow = {
  name: string;
  snapshot_count: number;
  last_backup_at: string | null;
  /** Newest snapshot folder names (up to three), newest-first. */
  recent_snapshots: string[];
};

export type HostVolumeSummary = {
  backup_root: string;
  bytes_avail: number | null;
  bytes_size: number | null;
  filesystem_source?: string | null;
  mount_point?: string | null;
  used_bytes?: number | null;
  used_percent?: string | null;
};

export type HostDiskProjectBytes = {
  name: string;
  bytes: number;
};

export type HostDiskInventory = {
  backup_root: string;
  backup_root_bytes: number;
  projects: HostDiskProjectBytes[];
  from_cache: boolean;
  scanned_at: string | null;
};
