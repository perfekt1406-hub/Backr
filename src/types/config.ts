/*
 * Purpose: TypeScript mirror of `config.toml` sections deserialized by the Tauri backend.
 * Role: Shared shapes for setup wizard, stores, and `invoke()` payloads.
 */

/** SSH target and remote backup root (`[remote]`). */
export type RemoteConfig = {
  host: string;
  user: string;
  ssh_key: string;
  port: number;
  backup_path: string;
};

/** Local roots (`[local]`). */
export type LocalConfig = {
  projects_path: string;
};

/** Scheduler cadence (`[schedule]`). */
export type ScheduleConfig = {
  interval_hours: number;
};

/** Persisted backup metadata (`[state]`). */
export type StateConfig = {
  last_backup_at: string | null;
};

/** Full persisted configuration document. */
export type Config = {
  remote: RemoteConfig;
  local: LocalConfig;
  schedule: ScheduleConfig;
  state: StateConfig;
};
