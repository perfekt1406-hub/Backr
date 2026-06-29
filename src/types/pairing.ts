/*
 * Purpose: DTOs for one-tap LAN pairing IPC (start/stop/status, discover, pair, confirm).
 * Role: Mirror the Rust pairing structs serialized over Tauri invoke (snake_case).
 */

import type { Config } from "./config";

/** Returned by `start_pairing` — the code plus the host's own SSH key fingerprint. */
export interface PairingStarted {
  code: string;
  /**
   * SHA256 fingerprint of this host's SSH key (e.g. `SHA256:abc...`), shown on the
   * host screen so the user can verify it matches the one the laptop displays.
   */
  host_key_fingerprint: string;
}

/** A host found on the LAN while it is in pairing mode. */
export interface DiscoveredHost {
  hostname: string;
  /** "ip:port" the client connects to for the pairing request. */
  address: string;
}

/**
 * Returned by `pair_with_host`: the prefilled config draft and the host's SSH key
 * fingerprint. The UI must show `host_key_fingerprint` and require user confirmation
 * before calling `confirm_pairing` to finalize (pin the host key and save config).
 */
export interface PairDraft {
  /** Prefilled config to be saved after the user confirms the fingerprint. */
  config: Config;
  /** SHA256 fingerprint of the host's SSH key (e.g. `SHA256:abc...`). */
  host_key_fingerprint: string;
  /** Full SSH host public key line to be pinned into known_hosts on confirmation. */
  host_pubkey: string;
  /** The resolved SSH target (IP or mDNS hostname) used for known_host pinning. */
  ssh_target: string;
}
