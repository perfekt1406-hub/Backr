/*
 * Purpose: Holds bootstrap routing kind after `resolve_shell_bootstrap` completes.
 * Role: Sidebar chrome and `$effect` redirects switch on laptop vs backup-host dashboard vs setup.
 */

import { writable } from "svelte/store";

/** High-level shell: setup wizard, normal backup client, or read-only host viewer. */
export type ShellKind = "setup" | "client" | "host";

export const shellKind = writable<ShellKind>("client");

/** Canonical backup root path when `shellKind` is `host` (from bootstrap / marker file). */
export const hostDashboardRoot = writable<string | null>(null);

/** Optional SSH account name from `/etc/backr/host.toml` (informational in UI). */
export const hostSshUser = writable<string | null>(null);
