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
  import { getCurrentWindow } from "@tauri-apps/api/window";

  import { getConfig, getDaemonError, resolveShellBootstrap } from "./lib/commands";
  import { registerMockProgressAppender } from "./lib/mockProgressSink";
  import routes from "./routes";
  import type { ShellBootstrap } from "./types/shellBootstrap";
  import { appendProgressLine } from "./stores/backup";
  import { hostDashboardRoot, hostSshUser, shellKind } from "./stores/shell";

  let checking = $state(true);
  // Non-null when the daemon could not be reached at startup: the shell renders a
  // clear "can't reach backrd" screen with a Retry button instead of falling
  // through to a broken half-state (every command would otherwise fail).
  let daemonError = $state<string | null>(null);

  // Listener teardown handles + a guard so a Retry never double-registers them.
  let unlistenBackup: (() => void) | undefined;
  let unlistenFocus: (() => void) | undefined;
  let removeHashListener: (() => void) | undefined;
  let listenersReady = false;

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

  /**
   * Applies a resolved bootstrap: sets shell stores and routes accordingly.
   * Shared by the initial run, the focus re-check, and the Retry button.
   */
  async function applyBootstrap(boot: ShellBootstrap): Promise<void> {
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
  }

  /**
   * Resolves the bootstrap, retrying a few times to ride out a daemon that is
   * still starting (the daemon spawn + socket bind can lag the GUI launch).
   */
  async function resolveBootstrapWithRetry(): Promise<ShellBootstrap> {
    let lastErr: unknown;
    for (let attempt = 0; attempt < 4; attempt++) {
      try {
        return await resolveShellBootstrap();
      } catch (err) {
        lastErr = err;
        await new Promise((resolve) => setTimeout(resolve, 400));
      }
    }
    throw lastErr;
  }

  /**
   * Runs (or re-runs, via Retry) the shell bootstrap. On persistent failure it
   * surfaces the daemon error instead of leaving the UI in a broken half-state.
   * IPC listeners are registered once, on the first successful bootstrap.
   */
  async function runBootstrap(): Promise<void> {
    checking = true;
    daemonError = null;
    try {
      let boot: ShellBootstrap = await resolveBootstrapWithRetry();
      if (devShellToggleEnabled() && getDevShellKindPreference() === "host") {
        boot = {
          mode: "host",
          backup_root: DEV_MOCK_HOST_BACKUP_ROOT,
          ssh_user: DEV_MOCK_HOST_SSH_USER,
        };
      }
      await applyBootstrap(boot);

      if (!listenersReady) {
        listenersReady = true;
        unlistenBackup = await listenBackupProgress((line) => appendProgressLine(line));

        // Re-evaluate bootstrap whenever the window is focused after being hidden
        // (e.g. config deleted while hidden in tray — next show picks up new state).
        unlistenFocus = await getCurrentWindow().onFocusChanged(async ({ payload: focused }) => {
          if (!focused) return;
          try {
            const fresh = await resolveShellBootstrap();
            if (fresh.mode === get(shellKind)) return;
            await applyBootstrap(fresh);
          } catch {
            // Ignore re-check errors — stale mode is better than a crash on focus.
          }
        });

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
      }
    } catch (err) {
      // Daemon unreachable after retries — prefer the reason the daemon recorded
      // at startup, falling back to the raw IPC error.
      const recorded = await getDaemonError().catch(() => null);
      daemonError = recorded ?? (err instanceof Error ? err.message : String(err));
    } finally {
      checking = false;
    }
  }

  onMount(() => {
    registerMockProgressAppender(appendProgressLine);

    void runBootstrap();

    return () => {
      unlistenFocus?.();
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
  {:else if daemonError}
    <div class="flex flex-1 flex-col items-center justify-center gap-4 px-8 text-center">
      <div class="label-caps tracking-[0.2em] text-[var(--muted)]">Can't reach backrd</div>
      <p class="max-w-md text-sm text-[var(--text)]">
        The Backr daemon isn't responding. It runs in the background and the app talks to it
        over a local socket — start it, then retry.
      </p>
      <pre
        class="max-w-md overflow-auto rounded border border-[var(--border)] bg-[var(--bg)] p-3 text-left text-xs text-[var(--muted)]">{daemonError}</pre>
      <button
        class="rounded border border-[var(--border)] px-4 py-2 text-sm hover:bg-[var(--border)]"
        onclick={() => void runBootstrap()}
      >
        Retry
      </button>
    </div>
  {:else}
    <SidebarNav />
    <main class="flex min-h-0 min-w-0 flex-1 flex-col border-l border-[var(--border)] bg-[var(--bg)]">
      <Router {routes} />
    </main>
  {/if}
</div>
