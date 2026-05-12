/*
 * Purpose: DTOs returned by `list_projects` / `get_backup_status`.
 * Role: Strongly typed dashboard and status chrome rows.
 */

/** One directory under the configured projects root. */
export type ProjectInfo = {
  name: string;
  last_backup_at: string | null;
  snapshot_count: number;
};

/** Aggregate scheduler + mutex snapshot for the UI. */
export type BackupStatus = {
  last_backup_at: string | null;
  next_backup_at: string | null;
  in_progress: boolean;
  active_project: string | null;
};
