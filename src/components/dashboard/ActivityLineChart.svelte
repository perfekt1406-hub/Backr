<!--
  Purpose: Daily activity line chart from persisted backup-completion markers.
  Role: Calls [`getActivitySeries`], buckets via [`bucketActivityByDay`], renders SVG (no chart npm deps).
-->
<script lang="ts">
  import * as commands from "../../lib/commands";
  import { bucketActivityByDay } from "../../lib/activityChart";
  import type { ActivityPoint } from "../../types/activity";
  import { backupStatus } from "../../stores/backup";

  /** Trailing local days to plot on the X axis. */
  const WINDOW_DAYS = 14;

  /** SVG viewBox width — scales with CSS width 100%. */
  const VB_W = 320;
  /** SVG viewBox height including padding for labels. */
  const VB_H = 140;

  let points = $state<ActivityPoint[]>([]);
  let loadErr = $state<string | null>(null);

  /** Pulls sparse ledger — tied to backup status polling via reactive subscription below. */
  async function pull(): Promise<void> {
    try {
      points = await commands.getActivitySeries();
      loadErr = null;
    } catch (e) {
      loadErr = String(e);
      points = [];
    }
  }

  $effect(() => {
    void $backupStatus?.last_backup_at;
    void $backupStatus?.in_progress;
    void pull();
  });

  /** Ordered daily buckets for chart geometry. */
  const series = $derived(bucketActivityByDay(points, WINDOW_DAYS));

  /** Maximum count in window — at least 1 so division stays finite. */
  const maxCount = $derived(Math.max(1, ...series.map((s) => s.count)));

  const padL = 28;
  const padR = 8;
  const padT = 10;
  const padB = 22;
  const chartW = VB_W - padL - padR;
  const chartH = VB_H - padT - padB;

  /** SVG polyline `points` attribute for the activity line. */
  const linePoints = $derived.by(() => {
    const n = series.length;
    if (n === 0) {
      return "";
    }
    return series
      .map((s, i) => {
        const x = padL + (n === 1 ? chartW / 2 : (i / (n - 1)) * chartW);
        const y = padT + chartH - (s.count / maxCount) * chartH;
        return `${x.toFixed(1)},${y.toFixed(1)}`;
      })
      .join(" ");
  });

  /** circles at each sample for hover-friendly visibility on sparse data. */
  const vertices = $derived.by(() => {
    const n = series.length;
    return series.map((s, i) => {
      const x = padL + (n === 1 ? chartW / 2 : (i / (n - 1)) * chartW);
      const y = padT + chartH - (s.count / maxCount) * chartH;
      return { x, y, count: s.count, dateKey: s.dateKey };
    });
  });

  /** Short labels for a subset of X ticks (local MM/DD). */
  function shortLabel(dateKey: string): string {
    const [, m, d] = dateKey.split("-");
    return `${m}/${d}`;
  }
</script>

<section
  class="rounded-[8px] border border-[var(--border)] bg-[var(--bg3)] px-4 py-3 panel-plate"
  aria-label="Backup activity by day"
>
  <div class="mb-2 flex items-center justify-between gap-2">
    <span class="label-caps text-[var(--muted)]">Activity (last {WINDOW_DAYS} days)</span>
    <span class="text-[10px] tabular-nums text-[var(--muted2)]">max {maxCount} / day</span>
  </div>

  {#if loadErr}
    <p class="text-[12px] text-[var(--danger)]">{loadErr}</p>
  {:else if series.every((s) => s.count === 0)}
    <p class="py-6 text-center text-[12px] text-[var(--muted2)]">
      No backup events recorded in this window — completed backups append markers here.
    </p>
  {:else}
    <svg
      class="w-full text-[var(--accent)]"
      viewBox="0 0 {VB_W} {VB_H}"
      preserveAspectRatio="xMidYMid meet"
      aria-hidden="true"
    >
      <!-- Grid baseline -->
      <line
        x1={padL}
        y1={padT + chartH}
        x2={padL + chartW}
        y2={padT + chartH}
        stroke="var(--border)"
        stroke-width="1"
      />
      <polyline
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        points={linePoints}
      />
      {#each vertices as v}
        <circle cx={v.x} cy={v.y} r="3.5" fill="var(--bg4)" stroke="currentColor" stroke-width="2" />
      {/each}
    </svg>
    <div class="mt-2 flex justify-between font-mono text-[10px] text-[var(--muted2)]">
      <span>{shortLabel(series[0].dateKey)}</span>
      <span class="text-[var(--muted)]">{WINDOW_DAYS}-day window · local dates</span>
      <span>{shortLabel(series[series.length - 1].dateKey)}</span>
    </div>
  {/if}
</section>
