/*
 * Purpose: Typed error shape exchanged between Rust backend commands and frontend error handlers.
 * Role: Defines `BackrCommandError` consumed by `handleCommandError` to route errors by kind
 *       instead of displaying raw strings.
 *
 * Wire format emitted by Tauri commands (serialised via serde):
 *   { "kind": "NotConfigured" | "Io" | "Remote" | "BackupInProgress" | "Other", "message": string }
 *
 * When the backend returns a plain `String` error (legacy path), `kind` falls back to "Other"
 * and `message` carries the original string.
 */

/**
 * Discriminated union of error categories surfaced by Backr backend commands.
 *
 * - `NotConfigured` — no configuration has been saved yet; the UI should navigate to setup.
 * - `BackupInProgress` — a backup job is already running; surface a non-fatal notice.
 * - `Remote` — SSH or rsync failure; show the message directly.
 * - `Io` — local filesystem error; show the message directly.
 * - `Other` — any other or unrecognised failure.
 */
export type BackrErrorKind =
  | "NotConfigured"
  | "BackupInProgress"
  | "Remote"
  | "Io"
  | "Other";

/**
 * Structured error returned by Tauri backend commands.
 *
 * Both fields are always present when the backend emits a typed error object.
 * When the backend emits a plain string (legacy), `parseBackrError` normalises it
 * into this shape with `kind = "Other"`.
 */
export interface BackrCommandError {
  /** Discriminator used to route errors — navigate vs toast vs ignore. */
  kind: BackrErrorKind;
  /** Human-readable explanation suitable for display in a toast. */
  message: string;
}
