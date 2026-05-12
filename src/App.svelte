<!--
  Purpose: Root chrome registering IPC listeners and guarding routes against unset configuration.
  Role: Hosts sidebar navigation plus hash-router outlets wrapped by shared instrumentation chrome.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import Router, { replace } from "svelte-spa-router";

  import ErrorToast from "./components/shared/ErrorToast.svelte";
  import SidebarNav from "./components/layout/SidebarNav.svelte";
  import { listenBackupProgress } from "./lib/events";
  import { getConfig } from "./lib/commands";
  import { registerMockProgressAppender } from "./lib/mockProgressSink";
  import routes from "./routes";
  import { appendProgressLine } from "./stores/backup";

  let checking = $state(true);

  onMount(() => {
    registerMockProgressAppender(appendProgressLine);

    let unlisten: (() => void) | undefined;

    void (async () => {
      try {
        const cfg = await getConfig();
        const hash = window.location.hash || "#/";
        if (!cfg && !hash.includes("/setup")) {
          replace("/setup");
        }
        if (cfg && hash.includes("/setup")) {
          replace("/");
        }
        unlisten = await listenBackupProgress((line) => appendProgressLine(line));
      } finally {
        checking = false;
      }
    })();

    return () => {
      unlisten?.();
    };
  });
</script>

<ErrorToast />

<div class="flex h-screen bg-[var(--bg)] text-[var(--text)]">
  <SidebarNav />
  <main class="flex min-h-0 min-w-0 flex-1 flex-col border-l border-[var(--border)] bg-[var(--bg)]">
    {#if checking}
      <div
        class="flex flex-1 items-center justify-center label-caps tracking-[0.2em] text-[var(--muted)]"
      >
        Initializing shell…
      </div>
    {:else}
      <Router {routes} />
    {/if}
  </main>
</div>
