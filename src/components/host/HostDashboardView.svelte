<!--
  Purpose: Read-only dashboard when Backr runs on the backup machine (`host_dashboard.toml`).
  Role: Lists local snapshot folders plus coarse disk stats via IPC (`host_*` commands).
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";

  import { HardDrive } from "lucide-svelte";

  import * as commands from "../../lib/commands";
  import type { HostProjectRow, HostVolumeSummary } from "../../types/hostDashboard";
  import { hostDashboardRoot, hostSshUser } from "../../stores/shell";
  import { relativeFromIso } from "../../lib/time";

  let rows = $state<HostProjectRow[]>([]);
  let volume = $state<HostVolumeSummary | null>(null);
  let loadErr = $state<string | null>(null);

  /** Loads filesystem-derived snapshot listing plus optional volume telemetry from Rust. */
  async function refresh(): Promise<void> {
    const root = get(hostDashboardRoot);
    if (!root?.trim()) {
      loadErr = "missing backup root (marker)";
      return;
    }
    loadErr = null;
    try {
      rows = await commands.hostListSnapshotProjects(root);
    } catch (err) {
      rows = [];
      loadErr = String(err);
    }
    try {
      volume = await commands.hostVolumeSummary(root);
    } catch {
      volume = null;
    }
  }

  onMount(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), 15000);
    return () => window.clearInterval(id);
  });

  function formatBytes(n: number | null | undefined): string {
    if (n == null || Number.isNaN(n)) {
      return "—";
    }
    const gb = n / 1e9;
    if (gb >= 1000) {
      return `${(gb / 1000).toFixed(1)} TB`;
    }
    return `${gb.toFixed(1)} GB`;
  }
</script>

<div class="flex min-h-0 flex-1 flex-col gap-8 px-10 py-10">
  <header class="flex flex-wrap items-start justify-between gap-6 border-b border-[var(--border)] pb-6">
    <div>
      <p class="label-caps mb-2 text-[var(--muted)]">Backup host</p>
      <h1 class="text-2xl font-semibold tracking-tight text-[var(--text)]">Local snapshot storage</h1>
      <p class="mt-2 max-w-xl text-[13px] text-[var(--muted2)]">
        Read-only view of directories under this machine’s backup root. Clients push snapshots over SSH —
        no rsync runs from this UI.
      </p>
      {#if $hostDashboardRoot}
        <p class="mt-3 font-mono text-[12px] text-[var(--muted)]">
          {$hostDashboardRoot}
          {#if $hostSshUser}
            <span class="text-[var(--muted2)]"> · SSH user {$hostSshUser}</span>
          {/if}
        </p>
      {/if}
    </div>
    <button
      type="button"
      class="shrink-0 text-[11px] uppercase tracking-[0.14em] text-[var(--accent)] hover:text-[var(--accent-hover)]"
      onclick={() => void refresh()}
    >
      Refresh
    </button>
  </header>

  {#if volume && (volume.bytes_avail != null || volume.bytes_size != null)}
    <section
      class="flex flex-wrap items-center gap-6 rounded-[8px] border border-[var(--border)] bg-[var(--bg2)] px-5 py-4 panel-plate"
    >
      <HardDrive size={20} class="text-[var(--accent)]" aria-hidden="true" />
      <div>
        <p class="label-caps text-[var(--muted)]">Volume</p>
        <p class="mt-1 text-[13px] text-[var(--text)]">
          <span class="text-[var(--muted2)]">Available</span>
          {formatBytes(volume.bytes_avail)}
          <span class="mx-2 text-[var(--border)]">·</span>
          <span class="text-[var(--muted2)]">Size</span>
          {formatBytes(volume.bytes_size)}
        </p>
      </div>
    </section>
  {/if}

  {#if loadErr}
    <div class="rounded-[8px] border border-[var(--danger)] bg-[var(--bg4)] px-4 py-3 text-[13px] text-[var(--danger)]">
      {loadErr}
    </div>
  {/if}

  <section class="flex flex-col gap-4">
    <h2 class="label-caps text-[var(--muted)]">Projects on disk</h2>
    {#if rows.length === 0 && !loadErr}
      <div
        class="rounded-[8px] border border-dashed border-[var(--border2)] px-4 py-8 text-center text-[13px] text-[var(--muted)]"
      >
        No project folders yet — backups from your laptop will appear as subdirectories here.
      </div>
    {:else}
      <div class="flex flex-col gap-3">
        {#each rows as row}
          <div
            class="flex items-center justify-between gap-4 rounded-[8px] border border-[var(--border)] bg-[var(--bg3)] px-4 py-3 panel-plate"
          >
            <div class="min-w-0">
              <div class="truncate font-semibold text-[var(--text)]">{row.name}</div>
              <div class="mt-1 text-[11px] uppercase tracking-[0.12em] text-[var(--muted)]">
                <span>Last {relativeFromIso(row.last_backup_at)}</span>
                <span class="text-[var(--muted2)]"> · {row.snapshot_count} snapshots</span>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </section>
</div>
