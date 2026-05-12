<!--
  Purpose: Newest-first snapshot stack for a single local project directory.
  Role: Hydrates via `list_snapshots` after decoding router params safely for arbitrary folder names.
-->
<script lang="ts">
  import { onMount } from "svelte";

  import { Layers } from "lucide-svelte";

  import SnapshotItem from "./SnapshotItem.svelte";
  import type { SnapshotEntry } from "../../types/snapshot";
  import * as commands from "../../lib/commands";

  interface Props {
    params?: { name?: string };
  }

  let { params = {} }: Props = $props();

  const project = $derived(decodeURIComponent(params.name ?? ""));

  let rows = $state<SnapshotEntry[]>([]);
  let loading = $state(true);
  let restoringAll = $state(false);

  onMount(() => {
    void (async () => {
      loading = true;
      try {
        rows = await commands.listSnapshots(project);
      } catch {
        rows = [];
      } finally {
        loading = false;
      }
    })();
  });

  /**
   * Runs `restore_all_snapshots` after confirmation — one rsync per snapshot under ~/Projects-….
   */
  async function restoreAll(): Promise<void> {
    const ok = window.confirm(
      `Restore all ${rows.length} snapshots for "${project}"?\n\nEach snapshot will be copied into your home directory as ~/Projects-<name> (standard snapshot IDs use that id; other names include the current UTC time). If a folder already exists, Backr adds -1, -2, … This runs ${rows.length} separate restores.`,
    );
    if (!ok) {
      return;
    }
    restoringAll = true;
    try {
      const paths = await commands.restoreAllSnapshots(project);
      window.alert(`Restore all completed (${paths.length} folders):\n\n${paths.join("\n")}`);
    } catch (err) {
      window.alert(String(err));
    } finally {
      restoringAll = false;
    }
  }
</script>

<div class="flex flex-col gap-4 px-10 pb-12">
  {#if loading}
    <div class="label-caps tracking-[0.2em] text-[var(--muted)]">Fetching remote index…</div>
  {:else if rows.length === 0}
    <div
      class="rounded-[8px] border border-dashed border-[var(--border2)] px-4 py-10 text-center text-[13px] text-[var(--muted)]"
    >
      No snapshots indexed yet — run a backup for this project first.
    </div>
  {:else}
    <div class="flex flex-col gap-4">
      <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
        <div class="shrink-0 text-[12px] text-[var(--muted)]">
          {rows.length} snapshot{rows.length === 1 ? "" : "s"} — newest first
        </div>
        <button
          type="button"
          disabled={restoringAll}
          class="inline-flex shrink-0 items-center gap-2 rounded-[5px] border border-[var(--danger)] px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--danger)] hover:bg-[var(--bg4)] disabled:cursor-not-allowed disabled:opacity-50"
          onclick={() => void restoreAll()}
        >
          <Layers size={14} aria-hidden="true" />
          {restoringAll ? "Restoring…" : "Restore all snapshots"}
        </button>
      </div>
      <div class="flex flex-col gap-3">
        {#each rows as snap}
          <SnapshotItem {project} {snap} />
        {/each}
      </div>
    </div>
  {/if}
</div>
