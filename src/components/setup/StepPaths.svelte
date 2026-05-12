<!--
  Purpose: Declares local project root and backup cadence fields prior to persisting config.
  Role: Binds `[local]`, `[schedule]`, and remote `backup_path` on the shared wizard draft.
-->
<script lang="ts">
  import type { Config } from "../../types/config";

  interface Props {
    draft: Config;
  }

  let { draft = $bindable() }: Props = $props();
</script>

<div class="grid max-w-3xl gap-6">
  <label class="flex flex-col gap-2 text-[12px] text-[var(--muted)]">
    <span class="label-caps text-[var(--muted)]">Local projects root</span>
    <input
      class="rounded-[5px] border border-[var(--border)] bg-[var(--bg4)] px-3 py-2 font-mono text-[13px] text-[var(--text)] panel-plate focus:border-[var(--accent)]"
      bind:value={draft.local.projects_path}
      spellcheck="false"
    />
    <span class="text-[11px] text-[var(--muted2)]"
      >Each immediate subdirectory becomes a discrete backup target.</span
    >
  </label>

  <label class="flex flex-col gap-2 text-[12px] text-[var(--muted)]">
    <span class="label-caps text-[var(--muted)]">Remote backup root</span>
    <input
      class="rounded-[5px] border border-[var(--border)] bg-[var(--bg4)] px-3 py-2 font-mono text-[13px] text-[var(--text)] panel-plate focus:border-[var(--accent)]"
      bind:value={draft.remote.backup_path}
      spellcheck="false"
    />
    <span class="text-[11px] text-[var(--muted2)]">
      Projects mirror as backup_root / project / snapshot folders on the receiver.
    </span>
  </label>

  <label class="flex flex-col gap-2 text-[12px] text-[var(--muted)]">
    <span class="label-caps text-[var(--muted)]">Automatic cadence (hours)</span>
    <input
      type="number"
      min="1"
      max="168"
      class="max-w-xs rounded-[5px] border border-[var(--border)] bg-[var(--bg4)] px-3 py-2 text-[var(--text)] panel-plate focus:border-[var(--accent)]"
      bind:value={draft.schedule.interval_hours}
    />
  </label>
</div>
