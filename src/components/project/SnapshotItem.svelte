<!--
  Purpose: Snapshot timeline rows with restore affordances and browser deep links.
  Role: Delegates destructive restores through `restoreSnapshot` after explicit confirmation.
-->
<script lang="ts">
  import { Download, FolderSearch } from "lucide-svelte";
  import { link } from "svelte-spa-router";

  import * as commands from "../../lib/commands";
  import type { SnapshotEntry } from "../../types/snapshot";

  interface Props {
    project: string;
    snap: SnapshotEntry;
  }

  let { project, snap }: Props = $props();

  /** True while the restore rsync is in flight — disables the button and shows progress text. */
  let restoring = $state(false);

  const browseHref = $derived(
    `/project/${encodeURIComponent(project)}/${encodeURIComponent(snap.name)}`,
  );

  /** Copies an entire snapshot tree locally after explicit browser confirmation. */
  async function restore(): Promise<void> {
    const ok = window.confirm(
      `Restore snapshot "${snap.name}" into your home directory as ~/Projects-<name> (standard IDs match the snapshot folder name; unusual names get the current UTC time in the folder name). If that folder exists, Backr uses -1, -2, … Continue?`,
    );
    if (!ok) {
      return;
    }
    restoring = true;
    try {
      const dest = await commands.restoreSnapshot(project, snap.name);
      window.alert(`Restore completed:\n${dest}`);
    } catch (err) {
      window.alert(String(err));
    } finally {
      restoring = false;
    }
  }
</script>

<div
  class="flex flex-wrap items-center justify-between gap-4 rounded-[8px] border border-[var(--border)] bg-[var(--bg3)] px-4 py-3 panel-plate"
>
  <div>
    <div class="font-mono text-[14px] text-[var(--text)]">{snap.name}</div>
  </div>
  <div class="flex flex-wrap gap-2">
    <a
      href={browseHref}
      class="inline-flex items-center gap-2 rounded-[5px] border border-[var(--border)] px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--text)] hover:border-[var(--accent)]"
      use:link
    >
      <FolderSearch size={14} aria-hidden="true" />
      Browse
    </a>
    <button
      type="button"
      class="inline-flex items-center gap-2 rounded-[5px] border border-[var(--danger)] px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--danger)] hover:bg-[var(--bg4)] disabled:cursor-not-allowed disabled:opacity-50"
      disabled={restoring}
      onclick={() => void restore()}
    >
      <Download size={14} aria-hidden="true" />
      {restoring ? "Restoring…" : "Restore"}
    </button>
  </div>
</div>
