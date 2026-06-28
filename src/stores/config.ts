/*
 * Purpose: Shared configuration snapshot synchronized with Rust managed state.
 * Role: Setup wizard and guards call `loadConfig` / `saveCfg` to mutate IPC + store.
 */

import { writable, type Writable } from "svelte/store";

import * as commands from "../lib/commands";
import type { Config } from "../types/config";
import { handleCommandError } from "./ui";

/** Latest configuration (`null` before first successful disk load). */
export const config: Writable<Config | null> = writable(null);

/**
 * Pulls configuration via `get_config` and publishes into `config`.
 *
 * External: `commands.getConfig` invokes the Rust command.
 */
export async function loadConfig(): Promise<Config | null> {
  try {
    const next = await commands.getConfig();
    config.set(next);
    return next;
  } catch (err) {
    handleCommandError(err);
    return null;
  }
}

/**
 * Persists `next`, refreshes the store, and surfaces failures through `handleCommandError`.
 *
 * External: `commands.saveConfig` persists atomically and restarts the scheduler.
 */
export async function saveCfg(next: Config): Promise<boolean> {
  try {
    await commands.saveConfig(next);
    config.set(next);
    return true;
  } catch (err) {
    handleCommandError(err);
    return false;
  }
}
