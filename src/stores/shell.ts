/*
 * Purpose: Tracks laptop vs setup wizard vs backup-host dashboard bootstrap outcome.
 * Role: Populated from `resolve_shell_bootstrap` in `App.svelte`; sidebar reads `shellKind`.
 */

import { writable, type Writable } from "svelte/store";

/**
 * SPA bootstrap classification mirrored from Rust [`resolve_shell_bootstrap`].
 *
 * Starts as `"loading"` so no shell-specific chrome renders before bootstrap resolves.
 * `App.svelte` sets the final value once `resolve_shell_bootstrap` returns.
 */
export const shellKind: Writable<"client" | "setup" | "host" | "loading"> = writable("loading");

/** Absolute backup root passed through [`ShellBootstrap`] for NAS-local dashboards. */
export const hostDashboardRoot: Writable<string | null> = writable(null);

/** Optional SSH user label surfaced next to host chrome — informational only. */
export const hostSshUser: Writable<string | null> = writable(null);
