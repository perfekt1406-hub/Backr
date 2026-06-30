<!--
  Purpose: Dashboard listing projects under the configured root plus scheduler and rsync log output.
  Role: Polls backup mutex metadata while exposing rsync progress streamed from Tauri events.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { replace } from "svelte-spa-router";

  import { Layers } from "lucide-svelte";

  import ActivityLineChart from "./ActivityLineChart.svelte";
  import ActivityStrip from "./ActivityStrip.svelte";
  import BackupNowButton from "./BackupNowButton.svelte";
  import DashboardSummaryCards from "./DashboardSummaryCards.svelte";
  import DashboardSystemInfo from "./DashboardSystemInfo.svelte";
  import ProjectListItem from "./ProjectListItem.svelte";
  import ScrollArea from "../shared/ScrollArea.svelte";
  import StatusBadge from "../shared/StatusBadge.svelte";
  import {
    backupStatus,
    clearProgressLog,
    progressLog,
    refreshBackupStatus,
  } from "../../stores/backup";
  import { loadConfig } from "../../stores/config";
  import { refreshProjects, refreshProjectsRemote, projects } from "../../stores/projects";
  import { shellKind } from "../../stores/shell";
  import * as commands from "../../lib/commands";
  import { relativeFromIso } from "../../lib/time";

  onMount(() => {
    if (get(shellKind) === "host") {
      replace("/host");
      return;
    }
    void loadConfig();
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
  <header class="border-b border-[var(--border)] pb-6">
    <!-- One row: title stack (left) · summary tiles (center) · status + backup (right). -->
    <div
      class="flex min-w-0 w-full flex-nowrap items-stretch gap-4 overflow-x-auto md:gap-6 xl:gap-8"
    >
      <div class="min-w-0 flex-1">
        <p class="label-caps mb-2 text-[var(--muted)] md:mb-3">Overview</p>
        <h1
          class="max-w-[22rem] text-3xl font-semibold leading-[1.12] tracking-tight text-[var(--text)] md:max-w-xl md:text-4xl lg:text-5xl lg:leading-[1.08]"
        >
          Projects backup
        </h1>
      </div>

      <div class="flex shrink-0 self-stretch">
        <DashboardSummaryCards />
      </div>

      <div class="flex min-w-[11rem] shrink-0 flex-col items-end gap-3 self-start">
        <div class="flex flex-wrap items-center justify-end gap-3">
          <StatusBadge active={busy} detail={$backupStatus?.active_project ?? null} />
          <BackupNowButton {busy} />
        </div>
        {#if $backupStatus}
          <div class="text-right text-[11px] uppercase tracking-[0.14em] text-[var(--muted)]">
            <div>Last run {relativeFromIso($backupStatus.last_backup_at)}</div>
            <div class="text-[var(--muted2)]">
              Next window {relativeFromIso($backupStatus.next_backup_at)}
            </div>
          </div>
        {/if}
      </div>
    </div>
  </header>

  <div class="grid min-h-0 flex-1 gap-6 lg:grid-cols-[1.15fr_0.85fr]">
    <section class="flex min-h-0 flex-col gap-4">
      <div class="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2">
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
        <div class="ml-auto flex flex-wrap items-center gap-x-3 gap-y-2">
          <button
            type="button"
            class="shrink-0 text-[11px] uppercase tracking-[0.14em] text-[var(--muted)] hover:text-[var(--accent)]"
            onclick={() => void refreshProjects()}
          >
            Reload (cached)
          </button>
          <button
            type="button"
            class="shrink-0 text-[11px] uppercase tracking-[0.14em] text-[var(--accent)] hover:text-[var(--accent-hover)]"
            onclick={() => void refreshProjectsRemote()}
            title="Requires SSH to your backup host"
          >
            Sync from backup server
          </button>
        </div>
      </div>
      <!-- Scroll pane: only the project list scrolls (the page stays put with many
           projects). On lg the viewport is absolutely positioned so its height
           can't inflate the grid row. ScrollArea hides the unstyleable native bar
           and overlays a custom one matching the panel aesthetic. -->
      <ScrollArea
        class="min-h-0 flex-1"
        viewportClass="flex flex-col gap-3 lg:absolute lg:inset-0 lg:overflow-y-auto lg:pr-3"
      >
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
      </ScrollArea>
    </section>

    <div class="flex min-h-0 flex-col gap-6 lg:min-h-[min(80vh,720px)]">
      <ActivityStrip />

      <section
        class="flex min-h-[200px] shrink-0 flex-col rounded-[8px] border border-[var(--border)] bg-[var(--bg3)] panel-plate lg:max-h-[min(40vh,320px)] lg:min-h-0 lg:flex-1"
      >
        <div class="flex shrink-0 items-center justify-between border-b border-[var(--border)] px-4 py-3">
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
          class="min-h-[140px] flex-1 overflow-auto whitespace-pre-wrap break-all px-4 py-3 font-mono text-[11px] leading-relaxed text-[var(--muted2)]"
        >{#if $progressLog.length === 0}<span class="text-[var(--muted)]"
            >Awaiting backup events…</span
          >{:else}{#each $progressLog as line}
{line}
{/each}{/if}</pre>
      </section>

      <DashboardSystemInfo />

      <ActivityLineChart />
    </div>
  </div>
</div>
