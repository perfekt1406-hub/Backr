/*
 * Purpose: Cross-cutting UI chrome (toasts) decoupled from routed screens.
 * Role: Minimal writable store consumed by `ErrorToast.svelte`; also exports
 *       `handleCommandError` for typed error routing from backend command failures.
 */

import { writable } from "svelte/store";

import type { BackrCommandError } from "../types/error";
import { shellKind } from "./shell";

/** Ephemeral user-visible diagnostic line (errors + confirmations). */
export const toastMessage = writable<string | null>(null);

/**
 * Shows a toast that clears automatically after a short delay.
 *
 * External: `setTimeout` schedules clearing without coupling to Svelte lifecycle.
 */
export function showToast(message: string, ttlMs = 6500): void {
  toastMessage.set(message);
  window.setTimeout(() => toastMessage.set(null), ttlMs);
}

/**
 * Parses an unknown thrown value into a `BackrCommandError` shape.
 *
 * The Tauri backend may emit:
 *   1. A typed JSON object `{ kind, message }` (structured error path).
 *   2. A plain string (legacy path — e.g. `Result<T, String>` commands).
 *
 * Both are normalised so callers can always branch on `kind`.
 *
 * @param err - The raw caught value from a rejected invoke promise.
 * @returns A normalised `BackrCommandError` with `kind` and `message`.
 */
function parseBackrError(err: unknown): BackrCommandError {
  if (err !== null && typeof err === "object") {
    const obj = err as Record<string, unknown>;
    if (typeof obj["kind"] === "string" && typeof obj["message"] === "string") {
      return { kind: obj["kind"] as BackrCommandError["kind"], message: obj["message"] };
    }
  }

  const raw = String(err);

  // Detect the plain-string "not configured" messages emitted by legacy commands.
  const lower = raw.toLowerCase();
  if (
    lower.includes("not configured") ||
    lower.includes("configure the application")
  ) {
    return { kind: "NotConfigured", message: raw };
  }

  if (lower.includes("backup is already in progress") || lower.includes("backup in progress")) {
    return { kind: "BackupInProgress", message: raw };
  }

  if (lower.includes("remote command failed") || lower.includes("ssh")) {
    return { kind: "SshFailed", message: raw };
  }

  if (lower.includes("rsync")) {
    return { kind: "RsyncFailed", message: raw };
  }

  if (lower.includes("i/o error") || lower.includes("io error") || lower.includes("no such file")) {
    return { kind: "Io", message: raw };
  }

  return { kind: "TaskFailed", message: raw };
}

/**
 * Routes a command error by kind: navigates to setup for `NotConfigured`, shows a toast otherwise.
 *
 * Use this as the single catch handler for all Tauri invoke wrappers instead of
 * `showToast(String(err))`.
 *
 * External: `shellKind.set` navigates to setup; `showToast` surfaces the message.
 *
 * @param err - The raw caught value from a rejected invoke promise.
 */
export function handleCommandError(err: unknown): void {
  const parsed = parseBackrError(err);

  if (parsed.kind === "NotConfigured") {
    shellKind.set("setup");
    return;
  }

  showToast(parsed.message);
}
