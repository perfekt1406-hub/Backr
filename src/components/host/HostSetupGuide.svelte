<!--
  Purpose: First-run onboarding shown in the host dashboard before any backups exist.
  Role: The single, in-order place to connect a laptop — install Backr on it, pair it
        (the 6-digit code is shown inline in step 3), then run the first backup. No need
        to navigate elsewhere. Replaces the empty state in HostDashboardView until the
        first snapshot arrives.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { CheckCircle2, Circle } from "lucide-svelte";

  import * as commands from "../../lib/commands";
  import type { HostTrustStatus } from "../../types/hostTrust";
  import PairingPanel from "./PairingPanel.svelte";

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
    <p class="label-caps mb-2 text-[var(--accent)]">Host ready — connect a laptop</p>
    <h2 class="text-xl font-semibold tracking-tight text-[var(--text)]">Connect your first laptop</h2>
    <p class="mt-2 max-w-2xl text-[13px] leading-relaxed text-[var(--muted2)]">
      Follow these steps in order. Project folders appear here once the first backup runs.
    </p>
  </div>

  <div class="flex flex-col gap-6">

    <!-- Step 1: Host ready -->
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

    <!-- Step 3: Pair — code shown inline here -->
    <div class="flex gap-4">
      {#if keysOk}
        <CheckCircle2 size={20} class="mt-0.5 shrink-0 text-[var(--accent)]" aria-hidden="true" />
      {:else}
        <Circle size={20} class="mt-0.5 shrink-0 text-[var(--muted)]" aria-hidden="true" />
      {/if}
      <div class="min-w-0 flex-1">
        <p class="font-medium text-[var(--text)]">3 · Pair the laptop</p>
        <p class="mt-1 max-w-xl text-[12px] leading-relaxed text-[var(--muted2)]">
          Click <strong class="font-medium text-[var(--text)]">Start pairing</strong>, then on the laptop open Backr,
          pick this host, and type the code shown below. That trusts the laptop's key automatically — no copying.
        </p>
        <div class="mt-3">
          <PairingPanel embedded />
        </div>
        <button
          type="button"
          class="mt-2 text-[11px] text-[var(--muted)] hover:text-[var(--accent)]"
          onclick={() => void push("/host/trust")}
        >
          Prefer to paste a key by hand? Trust keys →
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
        <p class="font-medium text-[var(--text)]">4 · Run the first backup from the laptop</p>
        <p class="mt-0.5 max-w-xl text-[12px] leading-relaxed text-[var(--muted2)]">
          After pairing, the laptop's setup is prefilled — click
          <strong class="font-medium text-[var(--text)]">Back Up Now</strong> there. Folders show up here after the first snapshot.
        </p>
      </div>
    </div>

  </div>
</div>
