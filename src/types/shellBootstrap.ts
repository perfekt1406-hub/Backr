/*
 * Purpose: JSON shape from `resolve_shell_bootstrap` for SPA routing on startup.
 * Role: Chooses laptop setup wizard vs client dashboard vs backup-host dashboard mode.
 */

export type ShellBootstrap =
  | { mode: "setup" }
  | { mode: "client" }
  | { mode: "host"; backup_root: string; ssh_user?: string | null };
