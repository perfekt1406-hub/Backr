/*
 * Purpose: DTOs for backup-host «Trust keys» IPC (`host_trust_status`, `host_append_authorized_pubkey`).
 * Role: Consumed by `HostTrustKeysView` and `commands.ts` wrappers.
 */

/** Snapshot of `authorized_keys` for the backup UNIX account (usually **backr**). */
export type HostTrustStatus = {
  ssh_user: string;
  authorized_keys_path: string;
  pubkey_line_count: number;
};

/** Result of attempting to append one pubkey line from the Trust UI. */
export type HostTrustAppendResult = {
  appended: boolean;
  skipped_duplicate: boolean;
  pubkey_line_count: number;
  sudo_script?: string;
  message: string;
};
