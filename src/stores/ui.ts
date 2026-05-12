/*
 * Purpose: Cross-cutting UI chrome (toasts) decoupled from routed screens.
 * Role: Minimal writable store consumed by `ErrorToast.svelte`.
 */

import { writable } from "svelte/store";

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
