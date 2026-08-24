/*
 * TypeScript mirror of the Rust `BackrCommandError` type returned by all Tauri commands.
 *
 * When a Tauri command fails, the IPC layer serializes the Rust error as:
 *   { "kind": "<Kind>", "message": "<human-readable description>" }
 *
 * The frontend can switch on `kind` to show context-aware error UI without parsing
 * human-readable message strings. Keep in sync with `src-tauri/src/error.rs`.
 */

/**
 * Discriminant values that map 1-to-1 with the Rust `ErrorKind` enum variants.
 *
 * - `NotConfigured`    — no configuration saved yet; navigate to setup.
 * - `BackupInProgress` — a job is already running; show a non-fatal notice.
 * - `SshFailed`        — SSH remote operation failed; show the message.
 * - `RsyncFailed`      — rsync transfer failed; show the message.
 * - `Io`               — local filesystem error.
 * - `InvalidInput`     — caller-supplied value was rejected by validation.
 * - `Config`           — TOML parse/save error.
 * - `Pairing`          — mDNS or HTTP pairing failure.
 * - `TaskFailed`       — tokio spawn_blocking join error.
 */
export type ErrorKind =
  | "NotConfigured"
  | "SshFailed"
  | "RsyncFailed"
  | "Io"
  | "BackupInProgress"
  | "InvalidInput"
  | "Config"
  | "Pairing"
  | "TaskFailed";

/**
 * Typed error returned from every Tauri command when the result is `Err`.
 *
 * Tauri delivers this as the rejection value of `invoke(...)`, so catch it with:
 *
 * ```ts
 * import type { BackrCommandError } from "$types/error";
 *
 * try {
 *   await invoke("list_snapshots", { project });
 * } catch (e) {
 *   const err = e as BackrCommandError;
 *   if (err.kind === "NotConfigured") { ... }
 * }
 * ```
 */
export type BackrCommandError = {
  /** Stable discriminant for routing; do NOT match on `message` text. */
  kind: ErrorKind;
  /** Human-readable description for display or logging. */
  message: string;
};
