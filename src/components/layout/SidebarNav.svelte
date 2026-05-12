<!--
  Purpose: Left rail navigation invoking hash routes without full reloads.
  Role: Persistent shell chrome—IBM Plex Mono stack and token-driven borders per `brand-aesthetic.md`.
-->
<script lang="ts">
  import { HardDrive, FolderGit2, Radar, Settings2 } from "lucide-svelte";
  import { link } from "svelte-spa-router";

  import { shellKind } from "../../stores/shell";
  import { useDevMock } from "../../lib/useDevMock";

  const devMockActive = useDevMock();
</script>

<aside
  class="flex w-[260px] shrink-0 flex-col gap-6 border-r border-[var(--border)] bg-[var(--bg2)] px-5 py-6 panel-plate"
>
  {#if devMockActive}
    <div
      class="rounded-[5px] border border-[var(--warn)] bg-[var(--bg4)] px-3 py-2 text-[11px] leading-snug text-[var(--warn)]"
      role="status"
    >
      <span class="font-semibold uppercase tracking-[0.12em]">Dev mock data</span>
      — IPC bypassed; rsync lines are synthetic.
      <span class="mt-1 block text-[10px] text-[var(--muted2)]">npm run dev:mock · localStorage backr-dev-mock=1</span>
    </div>
  {/if}
  <div class="flex items-start gap-3">
    <div
      class="flex h-10 w-10 items-center justify-center rounded-[5px] border border-[var(--border2)] bg-[var(--bg3)] text-[var(--accent)] panel-plate"
    >
      <Radar size={20} aria-hidden="true" />
    </div>
    <div class="min-w-0">
      <div class="text-[11px] font-semibold uppercase tracking-[0.18em] text-[var(--muted)]">
        {#if $shellKind === "host"}
          Backup host
        {:else}
          Snapshot backup
        {/if}
      </div>
      <div class="truncate text-lg font-semibold text-[var(--text)]">Backr</div>
      <p class="mt-2 text-[11px] leading-snug text-[var(--muted2)]">
        {#if $shellKind === "host"}
          Local snapshot tree · read-only view
        {:else}
          SSH · rsync · hardlink snapshots
        {/if}
      </p>
    </div>
  </div>

  <nav class="flex flex-col gap-1 text-[13px]" aria-label="Primary">
    {#if $shellKind === "host"}
      <a
        href="/host"
        class="flex items-center gap-2 rounded-[5px] px-3 py-2 text-[var(--text)] transition-colors hover:bg-[var(--bg3)] hover:text-[var(--accent-hover)]"
        use:link
      >
        <HardDrive size={18} class="text-[var(--accent)]" aria-hidden="true" />
        Storage
      </a>
    {:else if $shellKind === "setup"}
      <a
        href="/setup"
        class="flex items-center gap-2 rounded-[5px] px-3 py-2 text-[var(--text)] transition-colors hover:bg-[var(--bg3)] hover:text-[var(--accent-hover)]"
        use:link
      >
        <Settings2 size={18} class="text-[var(--accent)]" aria-hidden="true" />
        Setup
      </a>
    {:else}
      <a
        href="/"
        class="flex items-center gap-2 rounded-[5px] px-3 py-2 text-[var(--text)] transition-colors hover:bg-[var(--bg3)] hover:text-[var(--accent-hover)]"
        use:link
      >
        <FolderGit2 size={18} class="text-[var(--accent)]" aria-hidden="true" />
        Projects
      </a>
      <a
        href="/setup"
        class="flex items-center gap-2 rounded-[5px] px-3 py-2 text-[var(--muted2)] transition-colors hover:bg-[var(--bg3)] hover:text-[var(--accent-hover)]"
        use:link
      >
        <Settings2 size={18} class="text-[var(--muted)]" aria-hidden="true" />
        Settings
      </a>
    {/if}
  </nav>

  <div class="mt-auto border-t border-[var(--border)] pt-4">
    <p class="label-caps leading-relaxed text-[var(--muted)]">
      {#if $shellKind === "host"}
        On-disk snapshots<br />
        <span class="text-[var(--muted2)]">pushed from clients</span>
      {:else}
        Local projects<br />
        <span class="text-[var(--muted2)]">→ remote snapshot tree</span>
      {/if}
    </p>
  </div>
</aside>
