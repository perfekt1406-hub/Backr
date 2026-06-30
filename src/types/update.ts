/*
 * Purpose: TypeScript mirror of the self-update status reported by the daemon.
 * Role: Shape for the "Software updates" panel in client settings.
 */

/** Current-vs-latest version summary from `get_update_status`. */
export type UpdateStatus = {
  /** Version embedded in the running binaries. */
  current_version: string;
  /** Latest release tag, or null when the lookup failed. */
  latest_version: string | null;
  /** True when a strictly newer release is available. */
  update_available: boolean;
};
