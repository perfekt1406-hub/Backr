<!--
  Purpose: First-run onboarding shown in the host dashboard before any backups exist.
  Role: Guides the operator through connecting a laptop via one-tap pairing — install
        Backr on the laptop, pair it (which trusts its key), then the first backup.
        Replaces the empty state in HostDashboardView until the first snapshot arrives.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { CheckCircle2, Circle } from "lucide-svelte";

  import * as commands from "../../lib/commands";
  import type { HostTrustStatus } from "../../types/hostTrust";

  /** Number of snapshot projects already on disk — passed from HostDashboardView. */
  let { projectCount = 0 }: { projectCount: number } = $props();

  let trust = $state<HostTrustStatus | null>(null);

  /** True once at least one laptop has been paired/trusted. */
  const keysOk = $derived((trust?.pubkey_line_count ?? 0) > 0);

  onMount(async () => {
    try {
      trust = await commands.hostTrustStatus();
    } catch {
      /* non-fatal — steps render in pending state */
    }
  });
</script>

<div class="flex flex-col gap-7 rounded-[10px] border border-[var(--border)] bg-[var(--bg2)] px-8 py-8 panel-plate">

  <!-- Header -->
  <div>
    <p class="label-caps mb-2 text-[var(--accent)]">Host ready — connect your laptops</p>
    <h2 class="text-xl font-semibold tracking-tight text-[var(--text)]">Get started with Backr</h2>
    <p class="mt-2 max-w-2xl text-[13px] leading-relaxed text-[var(--muted2)]">
      This machine receives backups over SSH. Connect a laptop with one-tap pairing — its
      project folders appear here once the first backup runs.
    </p>
  </div>

  <div class="flex flex-col gap-6">

    <!-- Step 1: Host ready (always done) -->
    <div class="flex gap-4">
      <CheckCircle2 size={20} class="mt-0.5 shrink-0 text-[var(--accent)]" aria-hidden="true" />
      <div>
        <p class="font-medium text-[var(--text)]">Host is ready</p>
        <p class="mt-0.5 text-[12px] text-[var(--muted2)]">
          SSH server, <span class="font-mono text-[11px]">{trust?.ssh_user ?? "backr"}</span> account,
          and backup folder are configured.
        </p>
      </div>
    </div>

    <!-- Step 2: Install Backr on the laptop -->
    <div class="flex gap-4">
      <Circle size={20} class="mt-0.5 shrink-0 text-[var(--muted)]" aria-hidden="true" />
      <div class="min-w-0 flex-1">
        <p class="font-medium text-[var(--text)]">On the laptop — install Backr</p>
        <p class="mt-1 max-w-xl text-[12px] leading-relaxed text-[var(--muted2)]">
          Run one command (as your normal user, not sudo). It builds Backr and opens it.
        </p>
        <pre
          class="mt-3 select-all overflow-x-auto rounded-[6px] border border-[var(--border)] bg-[var(--bg3)] px-3 py-2.5 font-mono text-[11px] leading-relaxed text-[var(--text)]"
        >curl -fsSL https://raw.githubusercontent.com/perfekt1406-hub/Backr/main/scripts/setup-connecting-client.sh | bash</pre>
      </div>
    </div>

    <!-- Step 3: Pair the laptop -->
    <div class="flex gap-4">
      {#if keysOk}
        <CheckCircle2 size={20} class="mt-0.5 shrink-0 text-[var(--accent)]" aria-hidden="true" />
      {:else}
        <Circle size={20} class="mt-0.5 shrink-0 text-[var(--muted)]" aria-hidden="true" />
      {/if}
      <div class="flex-1">
        <p class="font-medium text-[var(--text)]">Pair the laptop</p>
        {#if keysOk}
          <p class="mt-0.5 text-[12px] text-[var(--muted2)]">
            {trust?.pubkey_line_count}
            {(trust?.pubkey_line_count ?? 0) === 1 ? "laptop" : "laptops"} paired — they can connect passwordlessly.
          </p>
        {:else}
          <p class="mt-0.5 max-w-xl text-[12px] leading-relaxed text-[var(--muted2)]">
            Click <strong class="font-medium text-[var(--text)]">Add a laptop</strong> to broadcast a
            6-digit code, then on the laptop pick this host and enter it — that trusts the laptop's key
            automatically. (Or paste a key by hand under Trust keys.)
          </p>
        {/if}
        <button
          type="button"
          class="mt-3 rounded-[6px] bg-[var(--accent)] px-4 py-1.5 text-[11px] font-semibold uppercase tracking-[0.1em] text-[var(--bg)] hover:bg-[var(--accent-hover)]"
          onclick={() => void push("/host/trust")}
        >
          {keysOk ? "Add another laptop →" : "Add a laptop →"}
        </button>
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
        <p class="font-medium text-[var(--text)]">First backup runs from the laptop</p>
        <p class="mt-0.5 max-w-xl text-[12px] leading-relaxed text-[var(--muted2)]">
          After pairing, the laptop's setup is prefilled — click
          <strong class="font-medium text-[var(--text)]">Back Up Now</strong> there. Project folders
          appear on this screen once the first snapshot arrives.
        </p>
      </div>
    </div>

  </div>
</div>
