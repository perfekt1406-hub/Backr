/*
 * Purpose: DTOs for one-tap LAN pairing IPC (start/stop/status, discover, pair).
 * Role: Mirror the Rust pairing structs serialized over Tauri invoke (snake_case).
 */

/** Returned by `start_pairing` — the code to display and when it expires. */
export interface PairingStarted {
  code: string;
  /** RFC3339 timestamp for the host-side countdown. */
  expires_at: string;
}

/** A host found on the LAN while it is in pairing mode. */
export interface DiscoveredHost {
  hostname: string;
  /** "ip:port" the client connects to for the pairing request. */
  address: string;
}
