/*
 * Purpose: DTO mirrors for backup-host filesystem dashboard IPC.
 * Role: Typed payloads for `host_list_snapshot_projects` / `host_volume_summary`.
 */

/** Snapshot directory row under one project on the backup host disk. */
export type HostSnapshotRow = {
  id: string;
  modified_iso: string | null;
};

/** Project directory row directly under `backup_root`. */
export type HostProjectRow = {
  name: string;
  snapshots: HostSnapshotRow[];
};

/** Optional `df` summary for the volume holding `backup_root`. */
export type HostVolumeSummary = {
  backup_root: string;
  bytes_avail: number | null;
  bytes_size: number | null;
};
