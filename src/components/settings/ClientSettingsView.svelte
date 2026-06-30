<!--
  Purpose: Day-2 configuration editor for the laptop client — replaces re-entering the wizard.
  Role: Loads the persisted Config, lets the user edit connection, local path, and schedule fields
        inline, and saves via saveConfig. Test connection button reuses the wizard's SSH probe.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { CheckCircle2, Plug } from "lucide-svelte";

  import * as commands from "../../lib/commands";
  import type { Config } from "../../types/config";
  import type { UpdateStatus } from "../../types/update";

  /* ── State ───────────────────────────────────────────────────────────────── */

  let config = $state<Config | null>(null);
  let loadErr = $state<string | null>(null);
  let saveErr = $state<string | null>(null);
  let saveOk = $state(false);
  let saveBusy = $state(false);
  let testBusy = $state(false);
  let testResult = $state<{ ok: boolean; msg: string } | null>(null);

  /* Software updates */
  let updateStatus = $state<UpdateStatus | null>(null);
  let autoUpdate = $state(false);
  let updateBusy = $state(false);
  let updateMsg = $state<string | null>(null);

  /* ── Lifecycle ───────────────────────────────────────────────────────────── */

  onMount(async () => {
    try {
      config = await commands.getConfig();
      if (!config) {
        loadErr = "No configuration found. Complete setup first.";
      }
    } catch (e) {
      loadErr = e instanceof Error ? e.message : String(e);
    }

    // Update info is best-effort: a slow or failed release check must not block settings.
    try {
      updateStatus = await commands.getUpdateStatus();
      autoUpdate = await commands.getUpdateSettings();
    } catch {
      /* leave the version panel without availability info */
    }
  });

  /* ── Actions ─────────────────────────────────────────────────────────────── */

  /** Persists the edited config; restarts the scheduler on success. */
  async function save(): Promise<void> {
    if (!config) return;
    saveErr = null;
    saveOk = false;
    saveBusy = true;
    try {
      await commands.saveConfig(config);
      saveOk = true;
    } catch (e) {
      saveErr = e instanceof Error ? e.message : String(e);
    } finally {
      saveBusy = false;
    }
  }

  /** Probes SSH connectivity with the current (unsaved) connection fields. */
  async function testConnection(): Promise<void> {
    if (!config) return;
    testResult = null;
    testBusy = true;
    try {
      await commands.testConnection(
        config.remote.host,
        config.remote.user,
        config.remote.ssh_key,
        config.remote.port,
      );
      testResult = { ok: true, msg: "Connection successful." };
    } catch (e) {
      testResult = { ok: false, msg: e instanceof Error ? e.message : String(e) };
    } finally {
      testBusy = false;
    }
  }

  /** Triggers an update via the daemon (which relaunches the app after the swap). */
  async function applyNow(): Promise<void> {
    updateBusy = true;
    updateMsg = null;
    try {
      await commands.applyUpdate();
      updateMsg = "Update started — Backr will restart shortly.";
    } catch (e) {
      updateMsg = e instanceof Error ? e.message : String(e);
    } finally {
      updateBusy = false;
    }
  }

  /** Flips the automatic-updates preference and persists it. */
  async function toggleAuto(): Promise<void> {
    updateMsg = null;
    try {
      autoUpdate = await commands.setUpdateSettings(!autoUpdate);
    } catch (e) {
      updateMsg = e instanceof Error ? e.message : String(e);
    }
  }
</script>

<!-- ─────────────────────────────────────────────────────────────────────────── -->

<div class="flex min-h-0 flex-1 flex-col gap-8 px-10 py-10">

{#if loadErr}
  <p class="text-[13px] text-[var(--warn)]">{loadErr}</p>
{:else if !config}
  <p class="text-[13px] text-[var(--muted)]">Loading…</p>
{:else}
  <div class="flex max-w-3xl flex-col gap-6">

    <!-- ── Connection section ── -->
    <section class="overflow-hidden rounded-[10px] border border-[var(--border)] bg-[var(--bg2)] panel-plate">
      <div class="px-8 pt-7 pb-5">
        <p class="label-caps mb-1 text-[var(--accent)]">Connection</p>
        <h2 class="text-base font-semibold text-[var(--text)]">Remote backup host</h2>
        <p class="mt-1 text-[12px] text-[var(--muted2)]">SSH credentials and backup root on the host machine.</p>
      </div>
      <div class="mx-8 border-t border-[var(--border)]"></div>
      <div class="px-8 py-6 bg-[var(--bg3)]">
        <div class="grid max-w-2xl gap-5">

          <!-- Host + port row -->
          <div class="grid gap-4 sm:grid-cols-[1fr_120px]">
            <label class="flex flex-col gap-1.5">
              <span class="label-caps text-[var(--muted)]">Host / IP</span>
              <input
                bind:value={config.remote.host}
                class="rounded-[5px] border border-[var(--border)] bg-[var(--bg2)] px-3 py-2 text-[13px] text-[var(--text)] outline-none focus:border-[var(--accent)]"
                autocomplete="off"
                spellcheck="false"
              />
            </label>
            <label class="flex flex-col gap-1.5">
              <span class="label-caps text-[var(--muted)]">Port</span>
              <input
                type="number"
                min="1"
                max="65535"
                bind:value={config.remote.port}
                class="rounded-[5px] border border-[var(--border)] bg-[var(--bg2)] px-3 py-2 text-[13px] text-[var(--text)] outline-none focus:border-[var(--accent)]"
              />
            </label>
          </div>

          <!-- SSH user -->
          <label class="flex flex-col gap-1.5">
            <span class="label-caps text-[var(--muted)]">SSH user</span>
            <input
              bind:value={config.remote.user}
              class="rounded-[5px] border border-[var(--border)] bg-[var(--bg2)] px-3 py-2 text-[13px] text-[var(--text)] outline-none focus:border-[var(--accent)]"
              autocomplete="username"
              spellcheck="false"
            />
          </label>

          <!-- SSH key path -->
          <label class="flex flex-col gap-1.5">
            <span class="label-caps text-[var(--muted)]">SSH private key path</span>
            <input
              bind:value={config.remote.ssh_key}
              class="rounded-[5px] border border-[var(--border)] bg-[var(--bg2)] px-3 py-2 font-mono text-[12px] text-[var(--text)] outline-none focus:border-[var(--accent)]"
              spellcheck="false"
            />
          </label>

          <!-- Remote backup root -->
          <label class="flex flex-col gap-1.5">
            <span class="label-caps text-[var(--muted)]">Remote backup root</span>
            <input
              bind:value={config.remote.backup_path}
              class="rounded-[5px] border border-[var(--border)] bg-[var(--bg2)] px-3 py-2 font-mono text-[12px] text-[var(--text)] outline-none focus:border-[var(--accent)]"
              spellcheck="false"
            />
          </label>

          <!-- Test connection -->
          <div class="flex flex-wrap items-center gap-3">
            <button
              type="button"
              class="flex items-center gap-2 rounded-[6px] border border-[var(--border)] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--muted)] hover:border-[var(--accent)] hover:text-[var(--accent)] disabled:opacity-50"
              disabled={testBusy}
              onclick={() => void testConnection()}
            >
              <Plug size={13} aria-hidden="true" />
              {testBusy ? "Testing…" : "Test connection"}
            </button>
            {#if testResult}
              <span class="text-[12px] {testResult.ok ? 'text-[var(--accent)]' : 'text-[var(--warn)]'}">
                {testResult.msg}
              </span>
            {/if}
          </div>

        </div>
      </div>
    </section>

    <!-- ── Local path section ── -->
    <section class="overflow-hidden rounded-[10px] border border-[var(--border)] bg-[var(--bg2)] panel-plate">
      <div class="px-8 pt-7 pb-5">
        <p class="label-caps mb-1 text-[var(--accent)]">Local path</p>
        <h2 class="text-base font-semibold text-[var(--text)]">Projects directory</h2>
        <p class="mt-1 text-[12px] text-[var(--muted2)]">The local folder whose subdirectories are treated as projects.</p>
      </div>
      <div class="mx-8 border-t border-[var(--border)]"></div>
      <div class="px-8 py-6 bg-[var(--bg3)]">
        <label class="flex max-w-2xl flex-col gap-1.5">
          <span class="label-caps text-[var(--muted)]">Projects path</span>
          <input
            bind:value={config.local.projects_path}
            class="rounded-[5px] border border-[var(--border)] bg-[var(--bg2)] px-3 py-2 font-mono text-[12px] text-[var(--text)] outline-none focus:border-[var(--accent)]"
            spellcheck="false"
          />
        </label>
      </div>
    </section>

    <!-- ── Schedule section ── -->
    <section class="overflow-hidden rounded-[10px] border border-[var(--border)] bg-[var(--bg2)] panel-plate">
      <div class="px-8 pt-7 pb-5">
        <p class="label-caps mb-1 text-[var(--accent)]">Schedule</p>
        <h2 class="text-base font-semibold text-[var(--text)]">Backup interval</h2>
        <p class="mt-1 text-[12px] text-[var(--muted2)]">How often the background backup job runs.</p>
      </div>
      <div class="mx-8 border-t border-[var(--border)]"></div>
      <div class="px-8 py-6 bg-[var(--bg3)]">
        <label class="flex flex-col gap-1.5">
          <span class="label-caps text-[var(--muted)]">Interval (hours)</span>
          <div class="flex items-center gap-3">
            <input
              type="number"
              min="1"
              max="168"
              bind:value={config.schedule.interval_hours}
              class="w-24 rounded-[5px] border border-[var(--border)] bg-[var(--bg2)] px-3 py-2 text-[13px] text-[var(--text)] outline-none focus:border-[var(--accent)]"
            />
            <span class="text-[12px] text-[var(--muted2)]">hours between snapshots</span>
          </div>
        </label>
      </div>
    </section>

    <!-- ── Software updates section ── -->
    <section class="overflow-hidden rounded-[10px] border border-[var(--border)] bg-[var(--bg2)] panel-plate">
      <div class="px-8 pt-7 pb-5">
        <p class="label-caps mb-1 text-[var(--accent)]">Software updates</p>
        <h2 class="text-base font-semibold text-[var(--text)]">App version</h2>
        <p class="mt-1 text-[12px] text-[var(--muted2)]">Update Backr to the latest release. Your connection, schedule, and snapshots are preserved.</p>
      </div>
      <div class="mx-8 border-t border-[var(--border)]"></div>
      <div class="flex flex-col gap-5 bg-[var(--bg3)] px-8 py-6">

        <!-- Current version + availability + Update now -->
        <div class="flex flex-wrap items-center gap-3">
          <span class="text-[13px] text-[var(--text)]">
            Current version
            <span class="ml-1 font-mono text-[12px] text-[var(--muted)]">{updateStatus?.current_version ?? "—"}</span>
          </span>
          {#if updateStatus?.update_available}
            <span class="rounded-full border border-[var(--accent)] px-2.5 py-0.5 text-[11px] font-semibold text-[var(--accent)]">
              Update available: {updateStatus.latest_version}
            </span>
            <button
              type="button"
              class="rounded-[6px] bg-[var(--accent)] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--bg)] hover:bg-[var(--accent-hover)] disabled:opacity-50"
              disabled={updateBusy}
              onclick={() => void applyNow()}
            >
              {updateBusy ? "Updating…" : "Update now"}
            </button>
          {:else if updateStatus}
            <span class="text-[12px] text-[var(--muted2)]">Up to date</span>
          {/if}
        </div>

        <!-- Auto-update toggle -->
        <label class="flex cursor-pointer items-center gap-3">
          <input
            type="checkbox"
            checked={autoUpdate}
            onchange={() => void toggleAuto()}
            class="h-4 w-4 accent-[var(--accent)]"
          />
          <span class="text-[13px] text-[var(--text)]">Automatic updates</span>
          <span class="text-[12px] text-[var(--muted2)]">Apply new releases when the app opens or a command runs.</span>
        </label>

        {#if updateMsg}
          <span class="text-[12px] text-[var(--muted)]">{updateMsg}</span>
        {/if}
      </div>
    </section>

    <!-- ── Save bar ── -->
    <div class="flex items-center gap-4">
      <button
        type="button"
        class="rounded-[6px] bg-[var(--accent)] px-5 py-2.5 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--bg)] hover:bg-[var(--accent-hover)] disabled:opacity-50"
        disabled={saveBusy}
        onclick={() => void save()}
      >
        {saveBusy ? "Saving…" : "Save changes"}
      </button>

      {#if saveOk}
        <span class="flex items-center gap-1.5 text-[12px] text-[var(--accent)]">
          <CheckCircle2 size={14} aria-hidden="true" />
          Saved
        </span>
      {/if}

      {#if saveErr}
        <span class="text-[12px] text-[var(--warn)]">{saveErr}</span>
      {/if}
    </div>

  </div>
{/if}

</div>
