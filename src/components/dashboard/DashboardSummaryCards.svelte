<!--
  Purpose: Compact aggregate tiles beside the dashboard title (projects, snapshots, backup cadence).
  Role: Derives metrics from [`projects`], [`backupStatus`], and [`config`] stores — no IPC here.
-->
<script lang="ts">
  import { backupStatus } from "../../stores/backup";
  import { config } from "../../stores/config";
  import { projects } from "../../stores/projects";
  import { relativeFromIso } from "../../lib/time";

  /** Row-level aggregates plus schedule hints for square summary tiles. */
  const stats = $derived.by(() => {
    const list = $projects;
    let newestIso: string | null = null;
    let maxT = 0;
    let cachedOnly = 0;
    for (const r of list) {
      if (r.stats_from_cache) {
        cachedOnly += 1;
      }
      if (!r.last_backup_at) {
        continue;
      }
      const t = Date.parse(r.last_backup_at);
      if (!Number.isNaN(t) && t >= maxT) {
        maxT = t;
        newestIso = r.last_backup_at;
      }
    }
    return {
      totalProjects: list.length,
      totalSnapshots: list.reduce((s, r) => s + r.snapshot_count, 0),
      newestProjectBackupIso: newestIso,
      cachedStatsProjects: cachedOnly,
    };
  });
</script>

<!-- Fills header middle column height (Overview → subtitle) via parent `self-stretch` + `h-full`. -->
<div class="flex h-full min-h-[7.5rem] w-max max-w-full flex-col items-center justify-between gap-2">
  <div
    class="grid h-full min-h-[7rem] w-max min-w-0 flex-1 grid-cols-4 gap-2 md:gap-3"
    style="grid-template-rows: 1fr;"
    aria-label="Backup summary"
  >
    <div
      class="flex min-h-0 min-w-[5.25rem] flex-col justify-between rounded-[8px] border border-[var(--border)] bg-[var(--bg3)] p-3 panel-plate md:min-w-[6.25rem] md:p-3.5"
    >
      <span class="label-caps text-[10px] leading-tight text-[var(--muted)]">Projects</span>
      <span class="text-3xl font-semibold tabular-nums leading-none text-[var(--text)] md:text-4xl"
        >{stats.totalProjects}</span
      >
      <span class="text-[10px] text-[var(--muted2)]">under root</span>
    </div>
    <div
      class="flex min-h-0 min-w-[5.25rem] flex-col justify-between rounded-[8px] border border-[var(--border)] bg-[var(--bg3)] p-3 panel-plate md:min-w-[6.25rem] md:p-3.5"
    >
      <span class="label-caps text-[10px] leading-tight text-[var(--muted)]">Snapshots</span>
      <span class="text-3xl font-semibold tabular-nums leading-none text-[var(--text)] md:text-4xl"
        >{stats.totalSnapshots}</span
      >
      <span class="text-[10px] text-[var(--muted2)]">total indexed</span>
    </div>
    <div
      class="flex min-h-0 min-w-[5.25rem] flex-col justify-between rounded-[8px] border border-[var(--border)] bg-[var(--bg3)] p-3 panel-plate md:min-w-[6.25rem] md:p-3.5"
    >
      <span class="label-caps text-[10px] leading-tight text-[var(--muted)]">Latest row</span>
      <span class="line-clamp-3 flex min-h-0 flex-1 items-center text-[13px] font-medium leading-snug text-[var(--text)] md:text-[14px]">
        {relativeFromIso(stats.newestProjectBackupIso)}
      </span>
      <span class="text-[10px] text-[var(--muted2)]">newest project backup</span>
    </div>
    <div
      class="flex min-h-0 min-w-[5.25rem] flex-col justify-between rounded-[8px] border border-[var(--border)] bg-[var(--bg3)] p-3 panel-plate md:min-w-[6.25rem] md:p-3.5"
    >
      <span class="label-caps text-[10px] leading-tight text-[var(--muted)]">Cadence</span>
      <span class="text-3xl font-semibold tabular-nums leading-none text-[var(--text)] md:text-4xl">
        {$config?.schedule.interval_hours ?? "—"}
      </span>
      <span class="text-[10px] leading-snug text-[var(--muted2)]">
        {#if $backupStatus}
          Next {relativeFromIso($backupStatus.next_backup_at)}
        {:else}
          hours between runs
        {/if}
      </span>
    </div>
  </div>

  {#if stats.cachedStatsProjects > 0 && stats.totalProjects > 0}
    <p class="max-w-[26rem] shrink-0 text-center text-[10px] leading-snug text-[var(--muted2)]">
      {stats.cachedStatsProjects} of {stats.totalProjects} rows use cached counts — sync when online for live totals.
    </p>
  {/if}
</div>
