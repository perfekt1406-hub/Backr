<!--
  Purpose: Day-2 management screen for the backup host — trusted key revocation and host info.
  Role: Lists every pubkey in authorized_keys with per-key Remove buttons, inline paste-to-add form,
        and the PairingPanel for one-tap pairing. Read-only host info section at the top.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { KeyRound, Trash2 } from "lucide-svelte";

  import * as commands from "../../lib/commands";
  import type { AuthorizedPubkeyEntry } from "../../types/hostTrust";
  import type { HostTrustStatus } from "../../types/hostTrust";
  import type { HostTrustAppendResult } from "../../types/hostTrust";
  import { hostDashboardRoot, hostSshUser } from "../../stores/shell";
  import PairingPanel from "./PairingPanel.svelte";

  /* ── State ───────────────────────────────────────────────────────────────── */

  let trustStatus = $state<HostTrustStatus | null>(null);
  let pubkeys = $state<AuthorizedPubkeyEntry[]>([]);
  let loadErr = $state<string | null>(null);

  /** Which action zone is shown below the key list. */
  let actionZone = $state<"pair" | "add">("pair");

  /* Add key form */
  let pubkeyPaste = $state("");
  let addBusy = $state(false);
  let addErr = $state<string | null>(null);
  let addResult = $state<HostTrustAppendResult | null>(null);

  /** Track which key is currently being removed to disable its button. */
  let removingLine = $state<string | null>(null);
  let removeErr = $state<string | null>(null);

  /* ── Derived ─────────────────────────────────────────────────────────────── */

  const backupRoot = $derived($hostDashboardRoot ?? "—");
  const sshUser = $derived($hostSshUser ?? trustStatus?.ssh_user ?? "—");
  const authKeysPath = $derived(trustStatus?.authorized_keys_path ?? "—");

  /* ── Lifecycle ───────────────────────────────────────────────────────────── */

  async function refresh(): Promise<void> {
    try {
      const [status, keys] = await Promise.all([
        commands.hostTrustStatus(),
        commands.hostListAuthorizedPubkeys(),
      ]);
      trustStatus = status;
      pubkeys = keys;
    } catch (e) {
      loadErr = e instanceof Error ? e.message : String(e);
    }
  }

  onMount(() => void refresh());

  /* ── Actions ─────────────────────────────────────────────────────────────── */

  /** Removes the exact raw_line from authorized_keys and refreshes the list. */
  async function removeKey(entry: AuthorizedPubkeyEntry): Promise<void> {
    removeErr = null;
    removingLine = entry.raw_line;
    try {
      await commands.hostRemoveAuthorizedPubkey(entry.raw_line);
      await refresh();
    } catch (e) {
      removeErr = e instanceof Error ? e.message : String(e);
    } finally {
      removingLine = null;
    }
  }

  /** Appends a pasted pubkey line and refreshes the list. */
  async function addKey(): Promise<void> {
    addErr = null;
    addResult = null;
    addBusy = true;
    try {
      addResult = await commands.hostAppendAuthorizedPubkey(pubkeyPaste);
      if (addResult.appended || addResult.skipped_duplicate) {
        pubkeyPaste = "";
      }
      await refresh();
    } catch (e) {
      addErr = e instanceof Error ? e.message : String(e);
    } finally {
      addBusy = false;
    }
  }
</script>

<!-- ─────────────────────────────────────────────────────────────────────────── -->

<div class="flex max-w-3xl flex-col gap-6">

  <!-- ── Host info (read-only) ── -->
  <section class="overflow-hidden rounded-[10px] border border-[var(--border)] bg-[var(--bg2)] panel-plate">
    <div class="px-8 pt-7 pb-5">
      <p class="label-caps mb-1 text-[var(--accent)]">Host info</p>
      <h2 class="text-base font-semibold text-[var(--text)]">This machine</h2>
      <p class="mt-1 text-[12px] text-[var(--muted2)]">Set by the install script — read-only.</p>
    </div>
    <div class="mx-8 border-t border-[var(--border)]"></div>
    <div class="px-8 py-6 bg-[var(--bg3)]">
      <dl class="grid gap-3 text-[13px]">
        <div class="flex flex-wrap gap-x-4 gap-y-0.5">
          <dt class="label-caps w-32 shrink-0 text-[var(--muted)]">Backup root</dt>
          <dd class="font-mono text-[12px] text-[var(--text)] break-all">{backupRoot}</dd>
        </div>
        <div class="flex flex-wrap gap-x-4 gap-y-0.5">
          <dt class="label-caps w-32 shrink-0 text-[var(--muted)]">SSH user</dt>
          <dd class="font-mono text-[12px] text-[var(--text)]">{sshUser}</dd>
        </div>
        <div class="flex flex-wrap gap-x-4 gap-y-0.5">
          <dt class="label-caps w-32 shrink-0 text-[var(--muted)]">Authorized keys</dt>
          <dd class="font-mono text-[12px] text-[var(--text)] break-all">{authKeysPath}</dd>
        </div>
      </dl>
    </div>
  </section>

  <!-- ── Trusted keys ── -->
  <section class="overflow-hidden rounded-[10px] border border-[var(--border)] bg-[var(--bg2)] panel-plate">
    <div class="px-8 pt-7 pb-5">
      <p class="label-caps mb-1 text-[var(--accent)]">Trusted keys</p>
      <h2 class="text-base font-semibold text-[var(--text)]">Authorized laptops</h2>
      <p class="mt-1 text-[12px] text-[var(--muted2)]">
        Each entry below is one laptop's SSH public key in <span class="font-mono text-[11px]">authorized_keys</span>.
      </p>
    </div>

    <!-- Key list -->
    {#if loadErr}
      <div class="px-8 pb-5">
        <p class="text-[12px] text-[var(--warn)]">{loadErr}</p>
      </div>
    {:else if pubkeys.length === 0}
      <div class="px-8 pb-6">
        <div class="flex items-center gap-3 rounded-[7px] border border-[var(--border)] bg-[var(--bg3)] px-4 py-3">
          <KeyRound size={16} class="shrink-0 text-[var(--muted)]" aria-hidden="true" />
          <p class="text-[12px] text-[var(--muted2)]">No laptops trusted yet. Add one below.</p>
        </div>
      </div>
    {:else}
      <div class="mx-8 mb-5 overflow-hidden rounded-[7px] border border-[var(--border)]">
        {#each pubkeys as entry, i (entry.raw_line)}
          <div
            class="flex items-center gap-3 px-4 py-3 text-[12px] {i < pubkeys.length - 1 ? 'border-b border-[var(--border)]' : ''}"
          >
            <KeyRound size={14} class="shrink-0 text-[var(--muted)]" aria-hidden="true" />
            <div class="min-w-0 flex-1">
              <p class="truncate font-medium text-[var(--text)]">{entry.comment || "(no comment)"}</p>
              <p class="mt-0.5 text-[11px] text-[var(--muted2)]">{entry.key_type}</p>
            </div>
            <button
              type="button"
              class="flex shrink-0 items-center gap-1 rounded-[5px] border border-[var(--border)] px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.08em] text-[var(--muted)] hover:border-[var(--warn)] hover:text-[var(--warn)] disabled:opacity-40"
              disabled={removingLine === entry.raw_line}
              onclick={() => void removeKey(entry)}
            >
              <Trash2 size={11} aria-hidden="true" />
              {removingLine === entry.raw_line ? "Removing…" : "Remove"}
            </button>
          </div>
        {/each}
      </div>
    {/if}

    {#if removeErr}
      <p class="mx-8 mb-4 text-[12px] text-[var(--warn)]">{removeErr}</p>
    {/if}

    <div class="mx-8 border-t border-[var(--border)]"></div>

    <!-- ── Action zone ── -->
    <div class="px-8 py-6 bg-[var(--bg3)]">

      <!-- Zone toggle -->
      <div class="mb-5 flex gap-1 rounded-[6px] border border-[var(--border)] bg-[var(--bg2)] p-0.5 w-fit">
        <button
          type="button"
          class="rounded-[4px] px-4 py-1.5 text-[11px] font-semibold uppercase tracking-[0.1em] transition-colors"
          class:bg-[var(--bg4)]={actionZone === "pair"}
          class:text-[var(--text)]={actionZone === "pair"}
          class:text-[var(--muted)]={actionZone !== "pair"}
          onclick={() => { actionZone = "pair"; }}
        >
          Pair a laptop
        </button>
        <button
          type="button"
          class="rounded-[4px] px-4 py-1.5 text-[11px] font-semibold uppercase tracking-[0.1em] transition-colors"
          class:bg-[var(--bg4)]={actionZone === "add"}
          class:text-[var(--text)]={actionZone === "add"}
          class:text-[var(--muted)]={actionZone !== "add"}
          onclick={() => { actionZone = "add"; addErr = null; addResult = null; }}
        >
          Paste key
        </button>
      </div>

      {#if actionZone === "pair"}
        <PairingPanel mode="inline" />

      {:else}
        <!-- Manual paste form -->
        <label for="settings-pubkey-paste" class="label-caps mb-2 block text-[var(--muted)]">
          Paste the laptop's public key line
        </label>
        <textarea
          id="settings-pubkey-paste"
          bind:value={pubkeyPaste}
          rows={3}
          placeholder="ssh-ed25519 AAAA… you@laptop"
          class="w-full max-w-xl resize-y rounded-[6px] border border-[var(--border)] bg-[var(--bg2)] px-3 py-2 font-mono text-[12px] text-[var(--text)] outline-none focus:border-[var(--accent)]"
        ></textarea>
        <div class="mt-3 flex flex-wrap gap-3">
          <button
            type="button"
            class="rounded-[6px] bg-[var(--accent)] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--bg)] hover:bg-[var(--accent-hover)] disabled:opacity-50"
            disabled={addBusy || !pubkeyPaste.trim()}
            onclick={() => void addKey()}
          >
            {addBusy ? "Adding…" : "Add key"}
          </button>
          <button
            type="button"
            class="rounded-[6px] border border-[var(--border)] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--muted)] hover:border-[var(--accent)] hover:text-[var(--accent)]"
            onclick={() => void refresh()}
          >
            Refresh
          </button>
        </div>

        {#if addErr}
          <p class="mt-3 text-[12px] text-[var(--warn)]">{addErr}</p>
        {/if}

        {#if addResult}
          <p class="mt-3 text-[12px] font-medium text-[var(--accent)]">{addResult.message}</p>
          {#if addResult.sudo_script}
            <p class="mt-2 text-[11px] text-[var(--muted2)]">Run on this machine's terminal:</p>
            <pre class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-[6px] border border-[var(--border)] bg-[var(--bg2)] p-3 font-mono text-[11px] text-[var(--text)]">{addResult.sudo_script}</pre>
          {/if}
        {/if}
      {/if}

    </div>
  </section>

</div>
