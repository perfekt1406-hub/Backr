/*
 * Purpose: Tracks laptop vs setup wizard vs backup-host dashboard bootstrap outcome.
 * Role: Populated from `resolve_shell_bootstrap` in `App.svelte`; sidebar reads `shellKind`.
 */

import { writable, type Writable } from "svelte/store";

/** SPA bootstrap classification mirrored from Rust [`resolve_shell_bootstrap`]. */
export const shellKind: Writable<"client" | "setup" | "host"> = writable("client");

/** Absolute backup root passed through [`ShellBootstrap`] for NAS-local dashboards. */
export const hostDashboardRoot: Writable<string | null> = writable(null);

/** Optional SSH user label surfaced next to host chrome — informational only. */
export const hostSshUser: Writable<string | null> = writable(null);
