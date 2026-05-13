<!--
  Purpose: Backup-host «Trust keys» screen — paste a laptop pubkey into ~backup/.ssh/authorized_keys (or copy sudo fallback).
  Role: Mounted at `#/host/trust` when `shellKind === 'host'`; uses IPC [`host_trust_status`] / [`host_append_authorized_pubkey`].
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { KeyRound } from "lucide-svelte";

  import * as commands from "../../lib/commands";
  import type { HostTrustAppendResult, HostTrustStatus } from "../../types/hostTrust";

  let status = $state<HostTrustStatus | null>(null);
  let pubkeyPaste = $state("");
  let loadErr = $state<string | null>(null);
  let actionErr = $state<string | null>(null);
  let lastResult = $state<HostTrustAppendResult | null>(null);
  let busy = $state(false);

  /** Loads authorized_keys stats from the Rust side (`getent` + file read). */
  async function refreshStatus(): Promise<void> {
    loadErr = null;
    try {
      status = await commands.hostTrustStatus();
    } catch (e) {
      loadErr = e instanceof Error ? e.message : String(e);
      status = null;
    }
  }

  /** Submits the pasted pubkey line to [`host_append_authorized_pubkey`]. */
  async function submitPubkey(): Promise<void> {
    actionErr = null;
    lastResult = null;
    busy = true;
    try {
      lastResult = await commands.hostAppendAuthorizedPubkey(pubkeyPaste);
      if (lastResult.appended || lastResult.skipped_duplicate) {
        pubkeyPaste = "";
      }
      await refreshStatus();
    } catch (e) {
      actionErr = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  /** Copies helper text to the clipboard when [`navigator.clipboard`] exists. */
  async function copyText(label: string, text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      lastResult = {
        appended: false,
        skipped_duplicate: false,
        pubkey_line_count: status?.pubkey_line_count ?? 0,
        message: `${label} copied to clipboard.`,
      };
    } catch {
      actionErr = "Clipboard unavailable — select and copy manually.";
    }
  }

  onMount(() => {
    void refreshStatus();
  });
</script>

<div class="flex min-h-0 flex-1 flex-col gap-8 px-10 py-10">
  <header class="border-b border-[var(--border)] pb-6">
    <p class="label-caps mb-2 text-[var(--muted)]">Backup host</p>
    <h1 class="flex items-center gap-3 text-2xl font-semibold tracking-tight text-[var(--text)]">
      <KeyRound size={26} class="text-[var(--accent)]" aria-hidden="true" />
      Trust laptop SSH keys
    </h1>
    <p class="mt-3 max-w-2xl text-[13px] leading-relaxed text-[var(--muted2)]">
      Passwordless backups use SSH public keys for the backup UNIX account (shown below). On your <strong>laptop</strong>,
      copy <span class="font-mono text-[12px] text-[var(--text)]">~/.ssh/id_ed25519.pub</span> — one line starting with
      <span class="font-mono text-[12px]">ssh-ed25519</span> — then paste it here. If this app cannot write the file
      (common when running as a desktop user), copy the generated <strong>sudo</strong> commands instead.
    </p>
  </header>

  {#if loadErr}
    <p class="rounded-[6px] border border-[var(--warn)] bg-[var(--bg4)] px-4 py-3 text-[13px] text-[var(--warn)]">
      {loadErr}
    </p>
  {:else if status}
    <section
      class="rounded-[8px] border border-[var(--border)] bg-[var(--bg2)] px-5 py-4 text-[13px] text-[var(--text)] panel-plate"
      aria-label="authorized_keys status"
    >
      <div class="flex flex-wrap gap-x-8 gap-y-2">
        <div>
          <span class="label-caps text-[var(--muted)]">SSH account</span>
          <span class="ml-2 font-mono">{status.ssh_user}</span>
        </div>
        <div>
          <span class="label-caps text-[var(--muted)]">Pubkey lines</span>
          <span class="ml-2 tabular-nums font-semibold">{status.pubkey_line_count}</span>
        </div>
      </div>
      <p class="mt-3 break-all font-mono text-[11px] text-[var(--muted)]">{status.authorized_keys_path}</p>
    </section>
  {/if}

  <section class="flex max-w-2xl flex-col gap-3">
    <label for="pubkey-paste" class="label-caps text-[var(--muted)]">Paste public key line</label>
    <textarea
      id="pubkey-paste"
      bind:value={pubkeyPaste}
      rows={4}
      placeholder="ssh-ed25519 AAAA… backr-you@laptop"
      class="min-h-[96px] resize-y rounded-[6px] border border-[var(--border)] bg-[var(--bg3)] px-3 py-2 font-mono text-[12px] text-[var(--text)] outline-none focus:border-[var(--accent)]"
    ></textarea>
    <div class="flex flex-wrap gap-3">
      <button
        type="button"
        class="rounded-[6px] bg-[var(--accent)] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--bg)] hover:bg-[var(--accent-hover)] disabled:opacity-50"
        disabled={busy || !pubkeyPaste.trim()}
        onclick={() => void submitPubkey()}
      >
        Add key
      </button>
      <button
        type="button"
        class="rounded-[6px] border border-[var(--border)] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--muted)] hover:border-[var(--accent)] hover:text-[var(--accent)]"
        onclick={() => void refreshStatus()}
      >
        Refresh status
      </button>
    </div>
  </section>

  {#if actionErr}
    <p class="text-[13px] text-[var(--warn)]">{actionErr}</p>
  {/if}

  {#if lastResult}
    <section class="max-w-2xl rounded-[8px] border border-[var(--border)] bg-[var(--bg2)] px-5 py-4 text-[13px] text-[var(--text)]">
      <p class="font-medium text-[var(--accent)]">{lastResult.message}</p>
      {#if lastResult.sudo_script}
        <p class="mt-3 text-[12px] text-[var(--muted2)]">
          Run these commands on <strong>this</strong> machine’s terminal (they use sudo):
        </p>
        <pre
          class="mt-2 max-h-56 overflow-auto whitespace-pre-wrap break-all rounded-[6px] border border-[var(--border)] bg-[var(--bg3)] p-3 font-mono text-[11px] text-[var(--text)]">{lastResult.sudo_script}</pre>
        <button
          type="button"
          class="mt-3 text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--accent)] hover:text-[var(--accent-hover)]"
          onclick={() => void copyText("Sudo commands", lastResult?.sudo_script ?? "")}
        >
          Copy sudo commands
        </button>
      {/if}
    </section>
  {/if}
</div>
