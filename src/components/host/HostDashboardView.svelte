<!--
  Purpose: Read-only dashboard when Backr runs on the backup machine (`host_dashboard.toml`).
  Role: Lists local snapshot folders plus coarse disk stats (`df`) and optional backup-tree sizes (`du`) via IPC.
        Shows HostSetupGuide in place of the empty state until the first backup snapshot arrives.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";

  import { HardDrive } from "lucide-svelte";

  import * as commands from "../../lib/commands";
  import type {
    HostDiskInventory,
    HostProjectRow,
    HostVolumeSummary,
  } from "../../types/hostDashboard";
  import { hostDashboardRoot, hostSshUser } from "../../stores/shell";
  import HostSetupGuide from "./HostSetupGuide.svelte";
  import { relativeFromIso } from "../../lib/time";

  /** Minimum percent free space before showing a low-volume warning (whole-filesystem `df` semantics). */
  const LOW_FREE_PCT = 10;

  let rows = $state<HostProjectRow[]>([]);
  let volume = $state<HostVolumeSummary | null>(null);
  let inventory = $state<HostDiskInventory | null>(null);
  let loadErr = $state<string | null>(null);
  let inventoryErr = $state<string | null>(null);
  let sortMode = $state<"name" | "activity">("name");
  let lastRefreshedAt = $state<string | null>(null);

  /** Sorted copy of [`rows`] according to [`sortMode`] for stable rendering. */
  const sortedRows = $derived.by(() => {
    const list = [...rows];
    if (sortMode === "name") {
      list.sort((a, b) => a.name.localeCompare(b.name));
      return list;
    }
    list.sort((a, b) => {
      const ta = a.last_backup_at ? Date.parse(a.last_backup_at) : 0;
      const tb = b.last_backup_at ? Date.parse(b.last_backup_at) : 0;
      if (tb !== ta) return tb - ta;
      return a.name.localeCompare(b.name);
    });
    return list;
  });

  /** Dashboard aggregates derived from snapshot rows (counts + newest backup instant). */
  const summaryStats = $derived.by(() => {
    let newestBackupIso: string | null = null;
    let maxT = 0;
    for (const r of rows) {
      if (!r.last_backup_at) continue;
      const t = Date.parse(r.last_backup_at);
      if (!Number.isNaN(t) && t >= maxT) {
        maxT = t;
        newestBackupIso = r.last_backup_at;
      }
    }
    return {
      totalProjects: rows.length,
      totalSnapshots: rows.reduce((s, r) => s + r.snapshot_count, 0),
      newestBackupIso,
    };
  });

  /**
   * Refreshes snapshot listing, volume summary, and disk inventory in parallel.
   *
   * External: `commands.hostListSnapshotProjects`, `hostVolumeSummary`, `hostDiskInventory` invoke Rust IPC.
   *
   * @param opts.forceDiskInventory When true, bypasses `du` TTL cache server-side.
   */
  async function refresh(opts?: { forceDiskInventory?: boolean }): Promise<void> {
    const root = get(hostDashboardRoot);
    if (!root?.trim()) {
      loadErr = "missing backup root (marker)";
      return;
    }
    loadErr = null;
    const forceDu = opts?.forceDiskInventory ?? false;

    const [projectsOut, volumeOut, inventoryOut] = await Promise.allSettled([
      commands.hostListSnapshotProjects(root),
      commands.hostVolumeSummary(root),
      commands.hostDiskInventory(root, forceDu),
    ]);

    if (projectsOut.status === "fulfilled") {
      rows = projectsOut.value;
    } else {
      rows = [];
      loadErr = String(projectsOut.reason);
    }

    if (volumeOut.status === "fulfilled") {
      volume = volumeOut.value;
    } else {
      volume = null;
    }

    if (inventoryOut.status === "fulfilled") {
      inventory = inventoryOut.value;
      inventoryErr = null;
    } else {
      inventoryErr = String(inventoryOut.reason);
    }

    lastRefreshedAt = new Date().toISOString();
  }

  onMount(() => {
    void refresh();
    const id = window.setInterval(() => void refresh(), 15000);
    return () => window.clearInterval(id);
  });

  /** Pretty-print byte totals using SI gigabytes for readability. */
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

  /** Percent free from `df` avail/size when both exist; used for threshold warnings. */
  function pctFreeVolume(vol: HostVolumeSummary): number | null {
    const { bytes_avail: a, bytes_size: s } = vol;
    if (a == null || s == null || s <= 0) return null;
    return (a / s) * 100;
  }

  /** Bar segment percentages for used vs free (`df` semantics); returns null when insufficient data. */
  function volumeUsedFreePct(vol: HostVolumeSummary): { usedPct: number; freePct: number } | null {
    const size = vol.bytes_size;
    if (size == null || size <= 0) return null;
    const used =
      vol.used_bytes ??
      (vol.bytes_avail != null ? size - vol.bytes_avail : null);
    if (used == null || used < 0) return null;
    const usedPct = Math.min(100, Math.max(0, (used / size) * 100));
    return { usedPct, freePct: Math.max(0, 100 - usedPct) };
  }

  /** Share of backup-root tree (`du`) for one project row; null when inventory missing or zero denominator. */
  function projectTreeSharePct(projectName: string): number | null {
    if (!inventory || inventory.backup_root_bytes <= 0) return null;
    const hit = inventory.projects.find((p) => p.name === projectName);
    if (!hit || hit.bytes <= 0) return null;
    return Math.min(100, (hit.bytes / inventory.backup_root_bytes) * 100);
  }

  /** Byte total under `backup_root/<project>` when inventory lists it. */
  function projectTreeBytes(projectName: string): number | null {
    const hit = inventory?.projects.find((p) => p.name === projectName);
    return hit ? hit.bytes : null;
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

  {#if rows.length > 0 || volume}
    <section
      class="flex flex-wrap gap-x-8 gap-y-3 rounded-[8px] border border-[var(--border)] bg-[var(--bg2)] px-5 py-4 text-[13px] text-[var(--text)] panel-plate"
      aria-label="Dashboard summary"
    >
      <div>
        <span class="label-caps text-[var(--muted)]">Projects</span>
        <span class="ml-2 font-semibold tabular-nums">{summaryStats.totalProjects}</span>
      </div>
      <div>
        <span class="label-caps text-[var(--muted)]">Snapshots</span>
        <span class="ml-2 font-semibold tabular-nums">{summaryStats.totalSnapshots}</span>
      </div>
      <div class="min-w-[200px]">
        <span class="label-caps text-[var(--muted)]">Last backup (any project)</span>
        <span class="ml-2">{relativeFromIso(summaryStats.newestBackupIso)}</span>
      </div>
      {#if lastRefreshedAt}
        <div>
          <span class="label-caps text-[var(--muted)]">Dashboard refreshed</span>
          <span class="ml-2 text-[var(--muted2)]">{relativeFromIso(lastRefreshedAt)}</span>
        </div>
      {/if}
    </section>
  {/if}

  {#if volume && (volume.bytes_avail != null || volume.bytes_size != null)}
    {@const segs = volumeUsedFreePct(volume)}
    {@const freePct = pctFreeVolume(volume)}
    {@const lowSpace = freePct != null && freePct < LOW_FREE_PCT}
    <section
      class="flex flex-col gap-4 rounded-[8px] border border-[var(--border)] bg-[var(--bg2)] px-5 py-4 panel-plate"
      aria-label="Filesystem containing backup path"
    >
      <div class="flex flex-wrap items-start gap-4">
        <HardDrive size={20} class="mt-0.5 shrink-0 text-[var(--accent)]" aria-hidden="true" />
        <div class="min-w-0 flex-1">
          <p class="label-caps text-[var(--muted)]">Volume (whole disk / filesystem)</p>
          <p class="mt-1 text-[12px] leading-snug text-[var(--muted2)]">
            Shows the <strong class="font-medium text-[var(--muted)]">entire volume</strong> that contains this backup
            path — not bytes used by Backr snapshots alone.
          </p>
          {#if volume.filesystem_source || volume.mount_point}
            <p class="mt-2 font-mono text-[11px] text-[var(--muted)]">
              {#if volume.filesystem_source}<span>{volume.filesystem_source}</span>{/if}
              {#if volume.filesystem_source && volume.mount_point}<span class="text-[var(--muted2)]"> → </span>{/if}
              {#if volume.mount_point}<span>{volume.mount_point}</span>{/if}
            </p>
          {/if}
          {#if segs}
            <div
              class="mt-4 flex h-2 w-full max-w-xl overflow-hidden rounded-full bg-[var(--border)]"
              role="img"
              aria-label="Disk space used versus free on this volume"
            >
              <div
                class="bg-[var(--accent)] transition-[width] duration-300"
                style={`width: ${segs.usedPct.toFixed(2)}%`}
              ></div>
              <div
                class="bg-[var(--border2)] transition-[width] duration-300"
                style={`width: ${segs.freePct.toFixed(2)}%`}
              ></div>
            </div>
          {/if}
          <p class="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-[13px] text-[var(--text)]">
            <span><span class="text-[var(--muted2)]">Total</span> {formatBytes(volume.bytes_size)}</span>
            <span
              ><span class="text-[var(--muted2)]">Used</span>
              {formatBytes(
                volume.used_bytes ??
                  (volume.bytes_size != null && volume.bytes_avail != null
                    ? volume.bytes_size - volume.bytes_avail
                    : null),
              )}</span
            >
            <span><span class="text-[var(--muted2)]">Avail</span> {formatBytes(volume.bytes_avail)}</span>
            {#if freePct != null}
              <span class="tabular-nums text-[var(--muted2)]">{freePct.toFixed(1)}% free</span>
            {/if}
            {#if volume.used_percent}
              <span class="text-[var(--muted2)]">df used {volume.used_percent}</span>
            {/if}
          </p>
        </div>
      </div>
      {#if lowSpace}
        <div
          class="rounded-[6px] border border-[var(--danger)]/40 bg-[var(--bg4)] px-3 py-2 text-[12px] text-[var(--danger)]"
          role="status"
        >
          Low free space on this volume (&lt; {LOW_FREE_PCT}% free). Consider freeing disk space or expanding storage.
        </div>
      {/if}
    </section>
  {/if}

  {#if inventoryErr}
    <div
      class="rounded-[8px] border border-[var(--border2)] bg-[var(--bg3)] px-4 py-3 text-[12px] text-[var(--muted)]"
      role="status"
    >
      Folder size scan unavailable: {inventoryErr}. Snapshot listing still reflects directories on disk.
    </div>
  {/if}

  {#if loadErr}
    <div class="rounded-[8px] border border-[var(--danger)] bg-[var(--bg4)] px-4 py-3 text-[13px] text-[var(--danger)]">
      {loadErr}
    </div>
  {/if}

  <section class="flex flex-col gap-4">
    <div class="flex flex-wrap items-center justify-between gap-4">
      <h2 class="label-caps text-[var(--muted)]">Projects on disk</h2>
      <div class="flex flex-wrap items-center gap-3">
        <div class="flex rounded-[6px] border border-[var(--border)] p-0.5 text-[11px] uppercase tracking-[0.12em]">
          <button
            type="button"
            class="rounded-[4px] px-2 py-1 transition-colors"
            class:bg-[var(--bg4)]={sortMode === "name"}
            class:text-[var(--text)]={sortMode === "name"}
            class:text-[var(--muted)]={sortMode !== "name"}
            onclick={() => {
              sortMode = "name";
            }}
          >
            A–Z
          </button>
          <button
            type="button"
            class="rounded-[4px] px-2 py-1 transition-colors"
            class:bg-[var(--bg4)]={sortMode === "activity"}
            class:text-[var(--text)]={sortMode === "activity"}
            class:text-[var(--muted)]={sortMode !== "activity"}
            onclick={() => {
              sortMode = "activity";
            }}
          >
            Recent
          </button>
        </div>
        <button
          type="button"
          class="text-[11px] uppercase tracking-[0.14em] text-[var(--accent)] hover:text-[var(--accent-hover)]"
          onclick={() => void refresh({ forceDiskInventory: true })}
        >
          Rescan folder sizes
        </button>
      </div>
    </div>
    {#if inventory && inventory.backup_root_bytes > 0}
      <p class="text-[11px] text-[var(--muted2)]">
        Bars show each project’s share of the measured backup folder tree (`du`).
        {#if inventory.from_cache}
          <span class="text-[var(--muted)]">Sizes served from cache — use “Rescan folder sizes” for a fresh pass.</span>
        {/if}
      </p>
    {/if}

    {#if sortedRows.length === 0 && !loadErr}
      <!-- Show the setup guide until the first backup snapshot arrives. -->
      <HostSetupGuide projectCount={rows.length} />
    {:else}
      <div class="flex flex-col gap-3">
        {#each sortedRows as row}
          {@const share = projectTreeSharePct(row.name)}
          {@const treeBytes = projectTreeBytes(row.name)}
          <div
            class="rounded-[8px] border border-[var(--border)] bg-[var(--bg3)] px-4 py-3 panel-plate"
          >
            <div class="flex items-start justify-between gap-4">
              <div class="min-w-0 flex-1">
                <div class="truncate font-semibold text-[var(--text)]">{row.name}</div>
                <div class="mt-1 text-[11px] uppercase tracking-[0.12em] text-[var(--muted)]">
                  <span>Last {relativeFromIso(row.last_backup_at)}</span>
                  <span class="text-[var(--muted2)]"> · {row.snapshot_count} snapshots</span>
                </div>
                {#if row.recent_snapshots?.length}
                  <p class="mt-2 font-mono text-[11px] leading-relaxed text-[var(--muted2)]">
                    Recent:
                    {row.recent_snapshots.join(", ")}
                  </p>
                {/if}
              </div>
            </div>
            {#if share != null && treeBytes != null}
              <div class="mt-3">
                <div class="h-1.5 w-full overflow-hidden rounded-full bg-[var(--border)]">
                  <div
                    class="h-full rounded-full bg-[var(--accent)] opacity-90"
                    style={`width: ${share.toFixed(2)}%`}
                  ></div>
                </div>
                <p class="mt-1 text-[10px] text-[var(--muted2)]">
                  {formatBytes(treeBytes)} · {share.toFixed(1)}% of measured backup tree
                </p>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </section>
</div>
