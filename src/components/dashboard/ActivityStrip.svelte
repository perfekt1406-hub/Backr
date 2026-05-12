<!--
  Purpose: Sparse telemetry strip summarizing persisted backup completions.
  Role: Calls `get_activity_series` for dashboard situational awareness without charts yet.
-->
<script lang="ts">
  import { Activity } from "lucide-svelte";

  import * as commands from "../../lib/commands";
  import type { ActivityPoint } from "../../types/activity";
  import { backupStatus } from "../../stores/backup";
  import { compactTimestamp } from "../../lib/time";

  let points = $state<ActivityPoint[]>([]);

  /** Pulls completion markers — re-run when dashboard polling refreshes mutex snapshot. */
  async function pullActivity(): Promise<void> {
    try {
      points = await commands.getActivitySeries();
    } catch {
      points = [];
    }
  }

  $effect(() => {
    void $backupStatus?.last_backup_at;
    void $backupStatus?.in_progress;
    void pullActivity();
  });
</script>

<section
  class="rounded-[8px] border border-[var(--border)] bg-[var(--bg2)] px-5 py-4 panel-plate"
>
  <div class="mb-3 flex items-center gap-2">
    <Activity size={16} class="text-[var(--accent)]" aria-hidden="true" />
    <span class="label-caps text-[var(--muted)]">Persisted backup timestamps</span>
  </div>
  {#if points.length === 0}
    <p class="text-[13px] text-[var(--muted2)]">
      No completion timestamps in config yet — run a backup to update <code class="text-[var(--text)]">state.last_backup_at</code>.
    </p>
  {:else}
    <ul class="flex flex-col gap-2 text-[13px]">
      {#each points as p}
        <li class="flex items-center justify-between gap-3 border-b border-[var(--border)] pb-2 last:border-0 last:pb-0">
          <span class="text-[var(--success)]">{p.label.replaceAll("_", " ")}</span>
          <span class="font-mono text-[12px] text-[var(--muted)]">
            {compactTimestamp(new Date(p.ts_unix * 1000).toISOString())}
          </span>
        </li>
      {/each}
    </ul>
  {/if}
</section>
