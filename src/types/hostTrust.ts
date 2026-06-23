/*
 * Purpose: DTOs for backup-host trust/key IPC commands.
 * Role: Consumed by HostSetupGuide, HostSettingsView, and commands.ts wrappers.
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

/** One parsed pubkey entry from authorized_keys — used in the host Settings key list. */
export type AuthorizedPubkeyEntry = {
  /** OpenSSH key type token, e.g. `ssh-ed25519`. */
  key_type: string;
  /** Base-64 key material. */
  key_b64: string;
  /** Trailing comment, typically `user@machine`. */
  comment: string;
  /** Exact raw line — used as the identity key when calling `host_remove_authorized_pubkey`. */
  raw_line: string;
};

/** Result of removing one pubkey from authorized_keys. */
export type HostRemovePubkeyResult = {
  removed: boolean;
  pubkey_line_count: number;
};
