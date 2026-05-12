/*
 * Purpose: Subscribes to backend-emitted Tauri events used by the UI.
 * Role: Wraps `@tauri-apps/api/event` so views avoid raw event channel strings.
 */

import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { useDevMock } from "./useDevMock";

/** Rust emits rsync / coordinator messages on this channel (`backup/rsync.rs`). */
const BACKUP_PROGRESS_EVENT = "backup://progress";

/**
 * Registers a listener for streaming backup lines until the returned teardown runs.
 *
 * External: `listen` from `@tauri-apps/api/event` — unwraps string payloads from the emitter.
 */
export async function listenBackupProgress(
  onLine: (line: string) => void,
): Promise<UnlistenFn> {
  if (useDevMock()) {
    void onLine;
    return () => {};
  }
  try {
    return await listen<string>(BACKUP_PROGRESS_EVENT, (event) => {
      onLine(event.payload);
    });
  } catch {
    return () => {};
  }
}
