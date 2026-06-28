<!--
  Purpose: Root chrome registering IPC listeners and guarding routes against unset configuration.
  Role: Resolves laptop vs backup-host dashboard vs setup via `resolve_shell_bootstrap`, then mounts hash routes.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import Router, { replace } from "svelte-spa-router";

  import ErrorToast from "./components/shared/ErrorToast.svelte";
  import DevScreenSwitcher from "./components/layout/DevScreenSwitcher.svelte";
  import SidebarNav from "./components/layout/SidebarNav.svelte";
  import { listenBackupProgress } from "./lib/events";
  import {
    DEV_MOCK_HOST_BACKUP_ROOT,
    DEV_MOCK_HOST_SSH_USER,
    devShellToggleEnabled,
    getDevShellKindPreference,
  } from "./lib/devShellPreference";
  import { getConfig, resolveShellBootstrap } from "./lib/commands";
  import { registerMockProgressAppender } from "./lib/mockProgressSink";
  import routes from "./routes";
  import type { ShellBootstrap } from "./types/shellBootstrap";
  import { appendProgressLine } from "./stores/backup";
  import { hostDashboardRoot, hostSshUser, shellKind } from "./stores/shell";

  let checking = $state(true);

  /**
   * Applies hash redirects once bootstrap mode and optional laptop config are known.
   */
  function routeForBootstrap(
    mode: "setup" | "client" | "host",
    cfgKnown: boolean,
  ): void {
    const hash = window.location.hash || "#/";
    if (mode === "host") {
      if (!hash.includes("/host")) {
        replace("/host");
      }
      return;
    }
    if (mode === "setup") {
      if (!hash.includes("/setup")) {
        replace("/setup");
      }
      return;
    }
    if (hash.includes("/host")) {
      replace("/");
    }
    if (!cfgKnown && !hash.includes("/setup")) {
      replace("/setup");
    }
    if (cfgKnown && hash.includes("/setup")) {
      replace("/");
    }
  }

  onMount(() => {
    registerMockProgressAppender(appendProgressLine);

    let unlistenBackup: (() => void) | undefined;
    let removeHashListener: (() => void) | undefined;

    void (async () => {
      try {
        const bootstrap = await resolveShellBootstrap();

        let boot: ShellBootstrap = bootstrap;
        if (devShellToggleEnabled() && getDevShellKindPreference() === "host") {
          boot = {
            mode: "host",
            backup_root: DEV_MOCK_HOST_BACKUP_ROOT,
            ssh_user: DEV_MOCK_HOST_SSH_USER,
          };
        }

        if (boot.mode === "host") {
          shellKind.set("host");
          hostDashboardRoot.set(boot.backup_root);
          hostSshUser.set(boot.ssh_user ?? null);
          routeForBootstrap("host", false);
        } else if (boot.mode === "setup") {
          shellKind.set("setup");
          hostDashboardRoot.set(null);
          hostSshUser.set(null);
          routeForBootstrap("setup", false);
        } else {
          shellKind.set("client");
          hostDashboardRoot.set(null);
          hostSshUser.set(null);
          const cfg = await getConfig();
          routeForBootstrap("client", cfg != null);
        }

        unlistenBackup = await listenBackupProgress((line) => appendProgressLine(line));

        const onHash = (): void => {
          const mode = get(shellKind);
          if (mode === "host") {
            routeForBootstrap("host", false);
          } else if (mode === "setup") {
            routeForBootstrap("setup", false);
          } else {
            void getConfig().then((c) => routeForBootstrap("client", c != null));
          }
        };
        window.addEventListener("hashchange", onHash);
        removeHashListener = () => window.removeEventListener("hashchange", onHash);
      } finally {
        checking = false;
      }
    })();

    return () => {
      unlistenBackup?.();
      removeHashListener?.();
    };
  });
</script>

<ErrorToast />

{#if devShellToggleEnabled()}
  <DevScreenSwitcher />
{/if}

<div class="flex h-screen bg-[var(--bg)] text-[var(--text)]">
  {#if checking}
    <div
      class="flex flex-1 items-center justify-center label-caps tracking-[0.2em] text-[var(--muted)]"
    >
      Initializing shell…
    </div>
  {:else}
    <SidebarNav />
    <main class="flex min-h-0 min-w-0 flex-1 flex-col border-l border-[var(--border)] bg-[var(--bg)]">
      <Router {routes} />
    </main>
  {/if}
</div>
