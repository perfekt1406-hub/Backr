<!--
  Purpose: Read-only overview of snapshot directories stored locally on the backup host disk.
  Role: Mirrors client dashboard layout — project cards + storage strip — without backup controls or SSH restores.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { HardDrive, RefreshCw, Server } from "lucide-svelte";

  import type { HostProjectRow, HostVolumeSummary } from "../../types/hostDashboard";
  import * as commands from "../../lib/commands";
  import { relativeFromIso } from "../../lib/time";
  import { hostDashboardRoot, hostSshUser, shellKind } from "../../stores/shell";
  import { replace } from "svelte-spa-router";

  let rows = $state<HostProjectRow[]>([]);
  let vol = $state<HostVolumeSummary | null>(null);
  let loading = $state(true);
  let err = $state<string | null>(null);

  /**
   * Pulls project tree + df summary for the configured backup root.
   */
  async function refresh(): Promise<void> {
    const root = get(hostDashboardRoot);
    if (!root) {
      err = "Missing backup root.";
      loading = false;
      return;
    }
    loading = true;
    err = null;
    try {
      const [p, v] = await Promise.all([
        commands.hostListSnapshotProjects(root),
        commands.hostVolumeSummary(root),
      ]);
      rows = p;
      vol = v;
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  }

  /**
   * Human-readable byte counts for df output (base-10 like GNU df -B1).
   */
  function fmtBytes(n: number | null): string {
    if (n == null || Number.isNaN(n)) {
      return "—";
    }
    if (n < 1024) {
      return `${n} B`;
    }
    const gb = n / 1_000_000_000;
    if (gb >= 1) {
      return `${gb.toFixed(2)} GB`;
    }
    const mb = n / 1_000_000;
    if (mb >= 1) {
      return `${mb.toFixed(1)} MB`;
    }
    const kb = n / 1000;
    return `${kb.toFixed(0)} KB`;
  }

  onMount(() => {
    if (get(shellKind) !== "host") {
      replace("/");
      return;
    }
    void refresh();
    const id = window.setInterval(() => void refresh(), 15000);
    return () => window.clearInterval(id);
  });
</script>

<div class="flex min-h-0 flex-1 flex-col gap-8 px-10 py-10">
  <header class="flex flex-wrap items-start justify-between gap-6 border-b border-[var(--border)] pb-6">
    <div>
      <p class="label-caps mb-2 text-[var(--muted)]">Backup host</p>
      <h1 class="text-2xl font-semibold tracking-tight text-[var(--text)]">Snapshot storage</h1>
      <p class="mt-2 max-w-xl text-[13px] text-[var(--muted2)]">
        Local view of directories written by laptops over SSH/rsync — read-only; backups still originate from
        client machines running Backr.
      </p>
      {#if $hostDashboardRoot}
        <p class="mt-3 font-mono text-[11px] text-[var(--muted)]">
          {$hostDashboardRoot}
          {#if $hostSshUser}
            <span class="text-[var(--muted2)]"> · SSH user {$hostSshUser}</span>
          {/if}
        </p>
      {/if}
    </div>
    <div class="flex flex-col items-end gap-3">
      <div class="flex items-center gap-2 rounded-[5px] border border-[var(--border2)] bg-[var(--bg3)] px-3 py-2 text-[var(--accent)] panel-plate">
        <Server size={18} aria-hidden="true" />
        <span class="label-caps text-[11px] tracking-[0.16em] text-[var(--muted)]">Listen-only</span>
      </div>
      <button
        type="button"
        class="inline-flex items-center gap-2 text-[11px] uppercase tracking-[0.14em] text-[var(--accent)] hover:text-[var(--accent-hover)]"
        onclick={() => void refresh()}
      >
        <RefreshCw size={14} aria-hidden="true" />
        Refresh
      </button>
    </div>
  </header>

  {#if err}
    <div class="rounded-[8px] border border-[var(--danger)] bg-[var(--bg4)] px-4 py-3 text-[13px] text-[var(--danger)]">
      {err}
    </div>
  {/if}

  <div class="grid gap-6 lg:grid-cols-[1.15fr_0.85fr]">
    <section class="flex flex-col gap-4">
      <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
        <h2 class="label-caps shrink-0 text-[var(--muted)]">Projects on disk</h2>
      </div>
      <div class="flex flex-col gap-3">
        {#if loading && rows.length === 0}
          <div class="text-[13px] text-[var(--muted)]">Loading snapshot tree…</div>
        {:else if rows.length === 0}
          <div
            class="rounded-[8px] border border-dashed border-[var(--border2)] px-4 py-8 text-center text-[13px] text-[var(--muted)]"
          >
            No project folders yet — backups from clients will appear as subdirectories under your backup root.
          </div>
        {:else}
          {#each rows as row}
            <article
              class="flex flex-col gap-2 rounded-[8px] border border-[var(--border)] bg-[var(--bg2)] px-4 py-3 panel-plate"
            >
              <div class="flex flex-wrap items-baseline justify-between gap-2">
                <span class="font-mono text-[14px] font-semibold text-[var(--text)]">{row.name}</span>
                <span class="text-[11px] uppercase tracking-[0.14em] text-[var(--muted)]">
                  {row.snapshots.length} snapshot{row.snapshots.length === 1 ? "" : "s"}
                </span>
              </div>
              {#if row.snapshots.length > 0}
                <div class="flex flex-wrap gap-2">
                  {#each row.snapshots.slice(0, 6) as snap}
                    <span
                      class="rounded-[4px] border border-[var(--border2)] bg-[var(--bg3)] px-2 py-1 font-mono text-[11px] text-[var(--muted2)]"
                      title={snap.modified_iso ?? ""}
                    >
                      {snap.id}
                      {#if snap.modified_iso}
                        <span class="ml-1 text-[var(--muted)]">· {relativeFromIso(snap.modified_iso)}</span>
                      {/if}
                    </span>
                  {/each}
                  {#if row.snapshots.length > 6}
                    <span class="self-center text-[11px] text-[var(--muted)]"
                      >+{row.snapshots.length - 6} more</span
                    >
                  {/if}
                </div>
              {:else}
                <p class="text-[12px] text-[var(--muted)]">No snapshot folders inside this project yet.</p>
              {/if}
            </article>
          {/each}
        {/if}
      </div>
    </section>

    <aside class="flex min-h-0 flex-col gap-6">
      <section
        class="flex flex-col rounded-[8px] border border-[var(--border)] bg-[var(--bg2)] panel-plate"
      >
        <div class="flex items-center gap-2 border-b border-[var(--border)] px-4 py-3">
          <HardDrive size={16} class="text-[var(--accent)]" aria-hidden="true" />
          <span class="label-caps text-[var(--muted)]">Volume</span>
        </div>
        <div class="space-y-3 px-4 py-4 text-[13px] text-[var(--muted2)]">
          {#if vol}
            <div class="flex justify-between gap-4">
              <span class="text-[var(--muted)]">Filesystem size</span>
              <span class="font-mono text-[var(--text)]">{fmtBytes(vol.bytes_size)}</span>
            </div>
            <div class="flex justify-between gap-4">
              <span class="text-[var(--muted)]">Available</span>
              <span class="font-mono text-[var(--text)]">{fmtBytes(vol.bytes_avail)}</span>
            </div>
          {:else}
            <p class="text-[var(--muted)]">No volume stats (df unavailable).</p>
          {/if}
        </div>
      </section>

      <section class="rounded-[8px] border border-[var(--border)] bg-[var(--bg3)] px-4 py-4 text-[12px] leading-relaxed text-[var(--muted2)]">
        <p class="label-caps mb-2 text-[var(--muted)]">Tip</p>
        Install Backr on this machine from the same repo. With no laptop config present, the app opens this
        host dashboard automatically when <span class="font-mono text-[11px] text-[var(--text)]"
          >/etc/backr/host.toml</span
        >
        exists (written by <span class="font-mono text-[11px]">setup-backup-host.sh</span>).
      </section>
    </aside>
  </div>
</div>
