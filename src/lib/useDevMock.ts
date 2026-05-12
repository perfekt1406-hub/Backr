/*
 * Purpose: Detects UI-only dev mode backed by synthetic IPC responses instead of Tauri.
 * Role: Gated to non-production builds — enables browser `vite` previews without Rust fixtures.
 */

/**
 * Returns true when mock handlers short-circuit real `invoke()` calls.
 *
 * Enable via `VITE_BACKR_MOCK=1` (see `npm run dev:mock` / `npm run tauri:dev:mock`) or, while in dev,
 * `localStorage.setItem('backr-dev-mock','1')` then reload.
 */
export function useDevMock(): boolean {
  if (import.meta.env.PROD) {
    return false;
  }
  const flag = import.meta.env.VITE_BACKR_MOCK;
  if (flag === "1" || flag === "true") {
    return true;
  }
  try {
    return globalThis.localStorage?.getItem("backr-dev-mock") === "1";
  } catch {
    return false;
  }
}
