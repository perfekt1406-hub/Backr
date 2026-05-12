/*
 * Purpose: Persists which dashboard shell to simulate during browser/Vite dev with mock IPC.
 * Role: Session-scoped storage only — no dependency on `commands` or `devMock/backend` (avoids import cycles).
 */

import { useDevMock } from "./useDevMock";

/** Tab-scoped key for laptop vs backup-host chrome when mocks are active. */
const SESSION_KEY = "backr-dev-shell-kind";

/** Matches mock remote `backup_path` so host dashboard IPC fixtures stay aligned. */
export const DEV_MOCK_HOST_BACKUP_ROOT = "/srv/backr";

/** Label shown next to host chrome in dev mock mode. */
export const DEV_MOCK_HOST_SSH_USER = "backr";

/**
 * Whether mock-driven dev runs should expose the host/client dashboard switcher.
 *
 * External: [`useDevMock`] mirrors `VITE_BACKR_MOCK` / `localStorage` flags documented in that module.
 */
export function devShellToggleEnabled(): boolean {
  return import.meta.env.DEV && useDevMock();
}

/**
 * Reads the chosen dashboard shell for [`mockResolveShellBootstrap`](./devMock/backend.ts).
 *
 * # Returns
 *
 * `'host'`, `'client'`, or `null` when the user has not chosen yet (mock defaults to client).
 *
 * External: `sessionStorage.getItem` reads the tab-local preference.
 */
export function getDevShellKindPreference(): "client" | "host" | null {
  if (!devShellToggleEnabled()) {
    return null;
  }
  try {
    const raw = sessionStorage.getItem(SESSION_KEY);
    if (raw === "host" || raw === "client") {
      return raw;
    }
  } catch {
    /* unavailable storage */
  }
  return null;
}

/**
 * Writes dashboard preference for subsequent reloads and for [`mockResolveShellBootstrap`].
 *
 * External: `sessionStorage.setItem` persists for the browser tab session.
 */
export function setDevShellKindPreference(kind: "client" | "host"): void {
  if (!devShellToggleEnabled()) {
    return;
  }
  try {
    sessionStorage.setItem(SESSION_KEY, kind);
  } catch {
    /* ignore */
  }
}
