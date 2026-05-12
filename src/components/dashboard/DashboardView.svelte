<!--
  Purpose: Dashboard listing projects under the configured root plus scheduler and rsync log output.
  Role: Polls backup mutex metadata while exposing rsync progress streamed from Tauri events.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { replace } from "svelte-spa-router";

  import { Layers } from "lucide-svelte";

  import ActivityStrip from "./ActivityStrip.svelte";
  import BackupNowButton from "./BackupNowButton.svelte";
  import ProjectListItem from "./ProjectListItem.svelte";
  import StatusBadge from "../shared/StatusBadge.svelte";
  import {
    backupStatus,
    clearProgressLog,
    progressLog,
    refreshBackupStatus,
  } from "../../stores/backup";
  import { refreshProjects, projects } from "../../stores/projects";
  import { shellKind } from "../../stores/shell";
  import * as commands from "../../lib/commands";
  import { relativeFromIso } from "../../lib/time";

  onMount(() => {
    if (get(shellKind) === "host") {
      replace("/host");
      return;
    }
    void refreshProjects();
    void refreshBackupStatus();
    const id = window.setInterval(() => void refreshBackupStatus(), 5000);
    return () => window.clearInterval(id);
  });

  const busy = $derived($backupStatus?.in_progress ?? false);

  let restoringAllProjects = $state(false);

  /**
   * Runs `restore_all_projects` after confirmation — sequential restores per project and snapshot.
   */
  async function restoreEveryProject(): Promise<void> {
    const n = $projects.length;
    const ok = window.confirm(
      `Restore all snapshots for every project folder (${n} under your configured root)?\n\nRuns many rsync jobs sequentially: each project gets every valid remote snapshot into ~/Projects-… folders (same rules as single-project restore all). Projects with no snapshots are skipped.`,
    );
    if (!ok) {
      return;
    }
    restoringAllProjects = true;
    try {
      const rows = await commands.restoreAllProjects();
      const totalFolders = rows.reduce((acc, r) => acc + r.destinations.length, 0);
      const detail = rows
        .map((r) => `${r.project}:\n${r.destinations.map((d) => `  ${d}`).join("\n")}`)
        .join("\n\n");
      window.alert(
        rows.length === 0
          ? "No snapshots were restored (no projects had remote snapshots)."
          : `Restore all projects completed (${rows.length} project(s), ${totalFolders} folder(s)):\n\n${detail}`,
      );
    } catch (err) {
      window.alert(String(err));
    } finally {
      restoringAllProjects = false;
    }
  }
</script>

<div class="flex min-h-0 flex-1 flex-col gap-8 px-10 py-10">
  <header class="flex flex-wrap items-start justify-between gap-6 border-b border-[var(--border)] pb-6">
    <div>
      <p class="label-caps mb-2 text-[var(--muted)]">Overview</p>
      <h1 class="text-2xl font-semibold tracking-tight text-[var(--text)]">Projects & backup</h1>
      <p class="mt-2 max-w-xl text-[13px] text-[var(--muted2)]">
        Snapshot interval is configurable; manual backups share the same in-process mutex as the tray and
        scheduler.
      </p>
    </div>
    <div class="flex flex-col items-end gap-3">
      <StatusBadge active={busy} detail={$backupStatus?.active_project ?? null} />
      <BackupNowButton {busy} />
      {#if $backupStatus}
        <div class="text-right text-[11px] uppercase tracking-[0.14em] text-[var(--muted)]">
          <div>Last run {relativeFromIso($backupStatus.last_backup_at)}</div>
          <div class="text-[var(--muted2)]">
            Next window {relativeFromIso($backupStatus.next_backup_at)}
          </div>
        </div>
      {/if}
    </div>
  </header>

  <div class="grid gap-6 lg:grid-cols-[1.15fr_0.85fr]">
    <section class="flex flex-col gap-4">
      <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
        <h2 class="label-caps shrink-0 text-[var(--muted)]">Projects</h2>
        <button
          type="button"
          disabled={restoringAllProjects || busy || $projects.length === 0}
          class="inline-flex shrink-0 items-center gap-2 rounded-[5px] border border-[var(--danger)] px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--danger)] hover:bg-[var(--bg4)] disabled:cursor-not-allowed disabled:opacity-40"
          onclick={() => void restoreEveryProject()}
        >
          <Layers size={14} aria-hidden="true" />
          {restoringAllProjects ? "Restoring…" : "Restore all projects"}
        </button>
        <button
          type="button"
          class="ml-auto shrink-0 text-[11px] uppercase tracking-[0.14em] text-[var(--accent)] hover:text-[var(--accent-hover)]"
          onclick={() => void refreshProjects()}
        >
          Refresh
        </button>
      </div>
      <div class="flex flex-col gap-3">
        {#if $projects.length === 0}
          <div
            class="rounded-[8px] border border-dashed border-[var(--border2)] px-4 py-8 text-center text-[13px] text-[var(--muted)]"
          >
            No immediate child directories detected — create folders under your configured projects root.
          </div>
        {:else}
          {#each $projects as row}
            <ProjectListItem {row} />
          {/each}
        {/if}
      </div>
    </section>

    <div class="flex min-h-0 flex-col gap-6">
      <ActivityStrip />

      <section
        class="flex min-h-[220px] flex-col rounded-[8px] border border-[var(--border)] bg-[var(--bg2)] panel-plate"
      >
        <div class="flex items-center justify-between border-b border-[var(--border)] px-4 py-3">
          <span class="label-caps text-[var(--muted)]">Rsync console</span>
          <button
            type="button"
            class="text-[11px] uppercase tracking-[0.14em] text-[var(--muted)] hover:text-[var(--accent)]"
            onclick={() => clearProgressLog()}
          >
            Clear
          </button>
        </div>
        <pre
          class="max-h-[320px] flex-1 overflow-auto whitespace-pre-wrap break-all px-4 py-3 font-mono text-[11px] leading-relaxed text-[var(--muted2)]"
        >{#if $progressLog.length === 0}<span class="text-[var(--muted)]"
            >Awaiting backup events…</span
          >{:else}{#each $progressLog as line}
{line}
{/each}{/if}</pre>
      </section>
    </div>
  </div>
</div>
