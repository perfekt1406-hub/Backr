<!--
  Purpose: First-run onboarding screen shown in the host dashboard before any backups exist.
  Role: Guides the operator through the three post-install steps: confirming the host is ready,
        trusting a laptop key, and running the first backup. Replaces the plain empty-state
        message in HostDashboardView until the first backup snapshot arrives.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { push } from "svelte-spa-router";
  import { CheckCircle2, Circle } from "lucide-svelte";

  import * as commands from "../../lib/commands";
  import type { HostTrustStatus } from "../../types/hostTrust";
  import type { SystemInfo } from "../../types/systemInfo";

  /** Number of snapshot projects already on disk — passed from HostDashboardView. */
  let { projectCount = 0 }: { projectCount: number } = $props();

  let trust = $state<HostTrustStatus | null>(null);
  let sysInfo = $state<SystemInfo | null>(null);

  /** True once at least one laptop key has been trusted (ssh-copy-id or manual paste). */
  const keysOk = $derived((trust?.pubkey_line_count ?? 0) > 0);

  onMount(async () => {
    try {
      [trust, sysInfo] = await Promise.all([
        commands.hostTrustStatus(),
        commands.getSystemInfo(),
      ]);
    } catch {
      /* non-fatal — steps render in pending state */
    }
  });
</script>

<div class="flex flex-col gap-7 rounded-[10px] border border-[var(--border)] bg-[var(--bg2)] px-8 py-8 panel-plate">

  <!-- Header -->
  <div>
    <p class="label-caps mb-2 text-[var(--accent)]">Host ready — connect your laptops</p>
    <h2 class="text-xl font-semibold tracking-tight text-[var(--text)]">
      Get started with Backr
    </h2>
    <p class="mt-2 max-w-2xl text-[13px] leading-relaxed text-[var(--muted2)]">
      This machine is set up to receive backups over SSH. Follow these steps on each
      laptop you want to back up — project folders appear here once the first backup runs.
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

    <!-- Step 2: Run client setup on each laptop -->
    <div class="flex gap-4">
      <Circle size={20} class="mt-0.5 shrink-0 text-[var(--muted)]" aria-hidden="true" />
      <div class="min-w-0 flex-1">
        <p class="font-medium text-[var(--text)]">On each laptop — run the client setup script</p>
        <p class="mt-1 max-w-xl text-[12px] leading-relaxed text-[var(--muted2)]">
          Clone the repo and run one command. The wizard asks for this machine's
          address{sysInfo?.hostname ? ` (${sysInfo.hostname})` : ""} and SSH port,
          installs the Backr app, and automatically trusts the laptop's key via
          <span class="font-mono text-[11px]">ssh-copy-id</span> (you type the
          <span class="font-mono text-[11px]">{trust?.ssh_user ?? "backr"}</span> password once).
        </p>
        <pre
          class="mt-3 select-all overflow-x-auto rounded-[6px] border border-[var(--border)] bg-[var(--bg3)] px-3 py-2.5 font-mono text-[11px] leading-relaxed text-[var(--text)]"
        >git clone https://github.com/perfekt1406-hub/Backr.git &amp;&amp; cd Backr
./scripts/setup-connecting-client.sh</pre>
      </div>
    </div>

    <!-- Step 3: Trust the laptop key -->
    <div class="flex gap-4">
      {#if keysOk}
        <CheckCircle2 size={20} class="mt-0.5 shrink-0 text-[var(--accent)]" aria-hidden="true" />
      {:else}
        <Circle size={20} class="mt-0.5 shrink-0 text-[var(--muted)]" aria-hidden="true" />
      {/if}
      <div class="flex-1">
        <p class="font-medium text-[var(--text)]">Trust the laptop's SSH key</p>
        {#if keysOk}
          <p class="mt-0.5 text-[12px] text-[var(--muted2)]">
            {trust?.pubkey_line_count}
            {(trust?.pubkey_line_count ?? 0) === 1 ? "key" : "keys"} trusted — laptops can connect passwordlessly.
          </p>
        {:else}
          <p class="mt-0.5 max-w-xl text-[12px] leading-relaxed text-[var(--muted2)]">
            The client script runs <span class="font-mono text-[11px]">ssh-copy-id</span> automatically.
            If that step was skipped, paste the laptop's
            <span class="font-mono text-[11px]">~/.ssh/id_ed25519.pub</span> here manually.
          </p>
          <button
            type="button"
            class="mt-3 rounded-[6px] bg-[var(--accent)] px-4 py-1.5 text-[11px] font-semibold uppercase tracking-[0.1em] text-[var(--bg)] hover:bg-[var(--accent-hover)]"
            onclick={() => void push("/host/trust")}
          >
            Open Trust Keys →
          </button>
        {/if}
      </div>
    </div>

    <!-- Step 4: Run the first backup -->
    <div class="flex gap-4">
      {#if projectCount > 0}
        <CheckCircle2 size={20} class="mt-0.5 shrink-0 text-[var(--accent)]" aria-hidden="true" />
      {:else}
        <Circle size={20} class="mt-0.5 shrink-0 text-[var(--muted)]" aria-hidden="true" />
      {/if}
      <div>
        <p class="font-medium text-[var(--text)]">Run the first backup from the laptop</p>
        <p class="mt-0.5 max-w-xl text-[12px] leading-relaxed text-[var(--muted2)]">
          Open Backr on the laptop, complete the in-app setup wizard, then click
          <strong class="font-medium text-[var(--text)]">Backup now</strong>. Project folders will appear on this
          screen once the first snapshot arrives.
        </p>
      </div>
    </div>

  </div>
</div>
