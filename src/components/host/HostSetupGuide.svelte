<!--
  Purpose: First-run onboarding shown in the host dashboard before any backups exist.
  Role: A four-step status checklist (steps 1-2 auto-complete, step 3 has the inline
        pairing panel + manual key-paste toggle, step 4 is post-pairing).
        Replaces the empty state in HostDashboardView until the first snapshot arrives.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { CheckCircle2, Circle } from "lucide-svelte";

  import * as commands from "../../lib/commands";
  import type { HostTrustAppendResult, HostTrustStatus } from "../../types/hostTrust";
  import PairingPanel from "./PairingPanel.svelte";

  /** Number of snapshot projects already on disk — passed from HostDashboardView. */
  let { projectCount = 0 }: { projectCount: number } = $props();

  let trust = $state<HostTrustStatus | null>(null);

  /** True once at least one laptop has been paired/trusted. */
  const keysOk = $derived((trust?.pubkey_line_count ?? 0) > 0);

  /* ── Manual trust form state ──────────────────────────────────────────── */

  /** Which panel is shown in the action zone. */
  let actionZone = $state<"pair" | "manual">("pair");

  let pubkeyPaste = $state("");
  let submitBusy = $state(false);
  let actionErr = $state<string | null>(null);
  let lastResult = $state<HostTrustAppendResult | null>(null);

  async function refreshTrust(): Promise<void> {
    try {
      trust = await commands.hostTrustStatus();
    } catch {
      /* non-fatal */
    }
  }

  /** Appends the pasted pubkey line via IPC. */
  async function submitPubkey(): Promise<void> {
    actionErr = null;
    lastResult = null;
    submitBusy = true;
    try {
      lastResult = await commands.hostAppendAuthorizedPubkey(pubkeyPaste);
      if (lastResult.appended || lastResult.skipped_duplicate) {
        pubkeyPaste = "";
      }
      await refreshTrust();
    } catch (e) {
      actionErr = e instanceof Error ? e.message : String(e);
    } finally {
      submitBusy = false;
    }
  }

  onMount(() => void refreshTrust());
</script>

<div class="flex flex-col rounded-[10px] border border-[var(--border)] bg-[var(--bg2)] panel-plate overflow-hidden">

  <!-- ── Header ── -->
  <div class="px-8 pt-8 pb-6">
    <p class="label-caps mb-2 text-[var(--accent)]">Host ready — connect a laptop</p>
    <h2 class="text-xl font-semibold tracking-tight text-[var(--text)]">Connect your first laptop</h2>
    <p class="mt-2 max-w-2xl text-[13px] leading-relaxed text-[var(--muted2)]">
      Follow these steps in order. Project folders appear here once the first backup runs.
    </p>
  </div>

  <!-- ── Step checklist ── -->
  <div class="flex flex-col gap-5 px-8 pb-8">

    <!-- Step 1: Host ready (always done) -->
    <div class="flex gap-4">
      <CheckCircle2 size={20} class="mt-0.5 shrink-0 text-[var(--accent)]" aria-hidden="true" />
      <div>
        <p class="font-medium text-[var(--text)]">1 · This host is ready</p>
        <p class="mt-0.5 text-[12px] text-[var(--muted2)]">
          SSH server, <span class="font-mono text-[11px]">{trust?.ssh_user ?? "backr"}</span> account, and backup folder are set up.
        </p>
      </div>
    </div>

    <!-- Step 2: Install on the laptop -->
    <div class="flex gap-4">
      <Circle size={20} class="mt-0.5 shrink-0 text-[var(--muted)]" aria-hidden="true" />
      <div class="min-w-0 flex-1">
        <p class="font-medium text-[var(--text)]">2 · On the laptop, install Backr</p>
        <p class="mt-1 max-w-xl text-[12px] leading-relaxed text-[var(--muted2)]">
          Run this on the laptop (your normal user, not sudo). Backr builds and opens itself.
        </p>
        <pre
          class="mt-3 select-all overflow-x-auto rounded-[6px] border border-[var(--border)] bg-[var(--bg3)] px-3 py-2.5 font-mono text-[11px] leading-relaxed text-[var(--text)]"
        >curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-connecting-client.sh | bash</pre>
      </div>
    </div>

    <!-- Step 3: Pair — inline pairing panel + manual fallback -->
    <div class="flex gap-4">
      {#if keysOk}
        <CheckCircle2 size={20} class="mt-0.5 shrink-0 text-[var(--accent)]" aria-hidden="true" />
      {:else}
        <Circle size={20} class="mt-0.5 shrink-0 text-[var(--muted)]" aria-hidden="true" />
      {/if}
      <div class="min-w-0 flex-1">
        <p class="font-medium text-[var(--text)]">3 · Pair the laptop</p>
        <p class="mt-0.5 max-w-xl text-[12px] leading-relaxed text-[var(--muted2)]">
          Click <strong class="font-medium text-[var(--text)]">Start pairing</strong>, then on the laptop open Backr,
          pick this host, and type the code shown. That trusts the laptop's key automatically.
        </p>

        {#if actionZone === "pair"}
          <div class="mt-4">
            <PairingPanel mode="inline" />
          </div>
          <button
            type="button"
            class="mt-4 text-[11px] text-[var(--muted)] hover:text-[var(--accent)]"
            onclick={() => { actionZone = "manual"; actionErr = null; lastResult = null; }}
          >
            Prefer to paste a key by hand? Trust keys →
          </button>

        {:else}
          <!-- Manual key-paste form -->
          <div class="mt-4">
            <div class="flex items-center justify-between mb-3">
              <p class="label-caps text-[var(--muted)]">Paste public key</p>
              <button
                type="button"
                class="text-[11px] text-[var(--muted)] hover:text-[var(--accent)]"
                onclick={() => { actionZone = "pair"; actionErr = null; lastResult = null; }}
              >
                ← Back to pairing
              </button>
            </div>

            {#if trust}
              <div class="mb-3 flex flex-wrap gap-x-6 gap-y-1 text-[12px] text-[var(--muted2)]">
                <span>SSH user <span class="font-mono text-[var(--text)]">{trust.ssh_user}</span></span>
                <span>Trusted keys <span class="font-semibold tabular-nums text-[var(--text)]">{trust.pubkey_line_count}</span></span>
                <span class="break-all font-mono text-[11px]">{trust.authorized_keys_path}</span>
              </div>
            {/if}

            <label for="guide-pubkey-paste" class="label-caps mb-2 block text-[var(--muted)]">
              Paste the laptop's public key line
            </label>
            <textarea
              id="guide-pubkey-paste"
              bind:value={pubkeyPaste}
              rows={3}
              placeholder="ssh-ed25519 AAAA… you@laptop"
              class="w-full max-w-xl resize-y rounded-[6px] border border-[var(--border)] bg-[var(--bg2)] px-3 py-2 font-mono text-[12px] text-[var(--text)] outline-none focus:border-[var(--accent)]"
            ></textarea>
            <div class="mt-3 flex flex-wrap gap-3">
              <button
                type="button"
                class="rounded-[6px] bg-[var(--accent)] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--bg)] hover:bg-[var(--accent-hover)] disabled:opacity-50"
                disabled={submitBusy || !pubkeyPaste.trim()}
                onclick={() => void submitPubkey()}
              >
                {submitBusy ? "Adding…" : "Add key"}
              </button>
              <button
                type="button"
                class="rounded-[6px] border border-[var(--border)] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--muted)] hover:border-[var(--accent)] hover:text-[var(--accent)]"
                onclick={() => void refreshTrust()}
              >
                Refresh
              </button>
            </div>

            {#if actionErr}
              <p class="mt-3 text-[12px] text-[var(--warn)]">{actionErr}</p>
            {/if}

            {#if lastResult}
              <p class="mt-3 text-[12px] font-medium text-[var(--accent)]">{lastResult.message}</p>
              {#if lastResult.sudo_script}
                <p class="mt-2 text-[11px] text-[var(--muted2)]">Run on this machine's terminal:</p>
                <pre class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap break-all rounded-[6px] border border-[var(--border)] bg-[var(--bg2)] p-3 font-mono text-[11px] text-[var(--text)]">{lastResult.sudo_script}</pre>
              {/if}
            {/if}
          </div>
        {/if}
      </div>
    </div>

    <!-- Step 4: First backup -->
    <div class="flex gap-4">
      {#if projectCount > 0}
        <CheckCircle2 size={20} class="mt-0.5 shrink-0 text-[var(--accent)]" aria-hidden="true" />
      {:else}
        <Circle size={20} class="mt-0.5 shrink-0 text-[var(--muted)]" aria-hidden="true" />
      {/if}
      <div>
        <p class="font-medium text-[var(--text)]">4 · Run the first backup from the laptop</p>
        <p class="mt-0.5 max-w-xl text-[12px] leading-relaxed text-[var(--muted2)]">
          After pairing, the laptop's setup is prefilled — click
          <strong class="font-medium text-[var(--text)]">Back Up Now</strong> there. Folders show up here after the first snapshot.
        </p>
      </div>
    </div>

  </div>


</div>
