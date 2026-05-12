<!--
  Purpose: One row in the project table linking into snapshot listings for that directory.
  Role: Shows last backup distance plus snapshot counts returned over SSH.
-->
<script lang="ts">
  import { ChevronRight } from "lucide-svelte";
  import { link } from "svelte-spa-router";

  import type { ProjectInfo } from "../../types/project";
  import { relativeFromIso } from "../../lib/time";

  interface Props {
    row: ProjectInfo;
  }

  let { row }: Props = $props();

  const href = $derived(`/project/${encodeURIComponent(row.name)}`);
</script>

<a
  {href}
  class="group flex items-center justify-between gap-4 rounded-[8px] border border-[var(--border)] bg-[var(--bg3)] px-4 py-3 transition hover:border-[var(--border-glow)] hover:bg-[var(--bg4)] panel-plate"
  use:link
>
  <div class="min-w-0">
    <div class="truncate font-semibold text-[var(--text)]">{row.name}</div>
    <div class="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-[11px] uppercase tracking-[0.12em] text-[var(--muted)]">
      <span>Last {relativeFromIso(row.last_backup_at)}</span>
      <span class="text-[var(--muted2)]">{row.snapshot_count} snapshots</span>
    </div>
  </div>
  <ChevronRight
    size={18}
    class="shrink-0 text-[var(--muted)] transition group-hover:text-[var(--accent)]"
    aria-hidden="true"
  />
</a>
