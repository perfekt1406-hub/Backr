<!--
  Purpose: Per-project screen listing remote snapshots and triggering targeted backups.
  Role: Embeds `SnapshotTimeline` plus scoped backup triggers under router `:name` params.
-->
<script lang="ts">
  import { link } from "svelte-spa-router";

  import BackupNowButton from "../dashboard/BackupNowButton.svelte";
  import SnapshotTimeline from "./SnapshotTimeline.svelte";
  import { backupStatus } from "../../stores/backup";

  interface Props {
    params?: { name?: string };
  }

  let { params = {} }: Props = $props();

  const project = $derived(decodeURIComponent(params.name ?? ""));
  const busy = $derived($backupStatus?.in_progress ?? false);
</script>

<div class="flex min-h-0 flex-1 flex-col">
  <div class="flex flex-wrap items-center justify-between gap-4 border-b border-[var(--border)] px-10 py-6">
    <div class="flex items-center gap-3 text-[12px] uppercase tracking-[0.14em] text-[var(--muted)]">
      <a href="/" class="hover:text-[var(--accent)]" use:link>← Projects</a>
      <span class="text-[var(--border2)]">/</span>
      <span class="text-[var(--text)]">{project}</span>
    </div>
    <BackupNowButton {busy} project={project} variant="ghost" />
  </div>
  <SnapshotTimeline {params} />
</div>
