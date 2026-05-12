/*
 * Purpose: Discriminated union returned by `resolve_shell_bootstrap` for hash-router guards.
 * Role: Chooses setup wizard vs laptop client vs read-only backup-host dashboard.
 */

/** Routing/bootstrap mode resolved once at shell startup (see `resolve_shell_bootstrap`). */
export type ShellBootstrap =
  | { mode: "setup" }
  | { mode: "client" }
  | { mode: "host"; backup_root: string; ssh_user?: string };
