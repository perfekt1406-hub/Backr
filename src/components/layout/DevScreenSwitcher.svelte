<!--
  Purpose: Dev-only screen switcher for browser mock previews — jump between the
  client setup/pair flow, client dashboard, host dashboard, and host trust/pairing
  screens without real backend state.
  Role: Rendered by App.svelte only when devShellToggleEnabled() (VITE_BACKR_MOCK + dev).
-->
<script lang="ts">
  import { replace } from "svelte-spa-router";

  import {
    DEV_MOCK_HOST_BACKUP_ROOT,
    DEV_MOCK_HOST_SSH_USER,
    setDevShellKindPreference,
  } from "../../lib/devShellPreference";
  import { hostDashboardRoot, hostSshUser, shellKind } from "../../stores/shell";

  interface Screen {
    label: string;
    kind: "client" | "setup" | "host";
    route: string;
  }

  const screens: Screen[] = [
    { label: "Client · Setup / Pair", kind: "setup", route: "/setup" },
    { label: "Client · Dashboard", kind: "client", route: "/" },
    { label: "Host · Dashboard", kind: "host", route: "/host" },
    { label: "Host · Trust keys (pairing)", kind: "host", route: "/host/trust" },
  ];

  let current = $state(screens[0].label);

  /** Sets the shell stores to match a screen, then navigates to it. */
  function go(label: string): void {
    const s = screens.find((x) => x.label === label);
    if (!s) return;
    current = label;
    if (s.kind === "host") {
      hostDashboardRoot.set(DEV_MOCK_HOST_BACKUP_ROOT);
      hostSshUser.set(DEV_MOCK_HOST_SSH_USER);
      setDevShellKindPreference("host");
    } else {
      hostDashboardRoot.set(null);
      hostSshUser.set(null);
      setDevShellKindPreference("client");
    }
    shellKind.set(s.kind);
    replace(s.route);
  }
</script>

<div
  class="fixed right-3 top-3 z-50 flex items-center gap-2 rounded-[6px] border border-[var(--border)] bg-[var(--bg2)] px-3 py-1.5 text-[11px] shadow-lg panel-plate"
>
  <span class="label-caps text-[var(--muted)]">Dev screen</span>
  <select
    class="rounded-[4px] border border-[var(--border)] bg-[var(--bg4)] px-2 py-1 text-[12px] text-[var(--text)]"
    value={current}
    onchange={(e) => go((e.currentTarget as HTMLSelectElement).value)}
  >
    {#each screens as s (s.label)}
      <option value={s.label}>{s.label}</option>
    {/each}
  </select>
</div>
