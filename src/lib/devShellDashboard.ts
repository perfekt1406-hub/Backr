/*
 * Purpose: Switches between laptop and backup-host dashboard chrome during dev + mock IPC runs.
 * Role: Updates shell stores and hash routes to match [`getDevShellKindPreference`](./devShellPreference.ts).
 */

import { replace } from "svelte-spa-router";

import { getConfig } from "./commands";
import {
  DEV_MOCK_HOST_BACKUP_ROOT,
  DEV_MOCK_HOST_SSH_USER,
  devShellToggleEnabled,
  setDevShellKindPreference,
} from "./devShellPreference";
import { hostDashboardRoot, hostSshUser, shellKind } from "../stores/shell";

/**
 * Re-applies laptop vs host shell mode and navigates without a full reload.
 *
 * # Inputs
 *
 * * `kind` — `'host'` opens `#/host` with mock backup root; `'client'` opens the normal dashboard or setup when no config exists.
 *
 * External: [`replace`] updates the hash route; [`getConfig`] chooses `/` vs `/setup` for client mode.
 */
export function switchDevDashboard(kind: "client" | "host"): void {
  if (!devShellToggleEnabled()) {
    return;
  }
  setDevShellKindPreference(kind);

  if (kind === "host") {
    shellKind.set("host");
    hostDashboardRoot.set(DEV_MOCK_HOST_BACKUP_ROOT);
    hostSshUser.set(DEV_MOCK_HOST_SSH_USER);
    replace("/host");
    return;
  }

  shellKind.set("client");
  hostDashboardRoot.set(null);
  hostSshUser.set(null);
  void getConfig().then((cfg) => replace(cfg != null ? "/" : "/setup"));
}

/** Re-export so sidebars can gate UI with one import path. */
export { devShellToggleEnabled } from "./devShellPreference";
