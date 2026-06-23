<!--
  Purpose: Client setup entry — discover Backr hosts on the LAN and pair with a
  6-digit code, prefilling the wizard. Falls back to manual entry.
  Role: Shown first in SetupWizard; uses discoverHosts / pairWithHost.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { Wifi } from "lucide-svelte";

  import * as commands from "../../lib/commands";
  import type { Config } from "../../types/config";
  import type { DiscoveredHost } from "../../types/pairing";

  interface Props {
    /** Called with the prefilled config draft after a successful pair. */
    onPaired: (cfg: Config) => void;
    /** Called when the user opts to type connection details by hand. */
    onManual: () => void;
  }
  let { onPaired, onManual }: Props = $props();

  let hosts = $state<DiscoveredHost[]>([]);
  let scanning = $state(false);
  let selected = $state<DiscoveredHost | null>(null);
  let code = $state("");
  let pairing = $state(false);
  let err = $state<string | null>(null);

  /** Browses the LAN for hosts in pairing mode. */
  async function scan(): Promise<void> {
    scanning = true;
    err = null;
    try {
      hosts = await commands.discoverHosts();
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      scanning = false;
    }
  }

  /** Pairs with the selected host using the entered code, then hands back the draft. */
  async function pair(): Promise<void> {
    if (!selected || code.trim().length !== 6) return;
    pairing = true;
    err = null;
    try {
      const cfg = await commands.pairWithHost(selected.address, code.trim());
      onPaired(cfg);
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      pairing = false;
    }
  }

  onMount(() => {
    void scan();
  });
</script>

<div class="flex flex-1 flex-col gap-8 px-10 py-10">
  <header class="border-b border-[var(--border)] pb-6">
    <p class="label-caps mb-2 text-[var(--muted)]">Initial setup</p>
    <h1 class="flex items-center gap-3 text-2xl font-semibold tracking-tight text-[var(--text)]">
      <Wifi size={26} class="text-[var(--accent)]" aria-hidden="true" />
      Find your backup host
    </h1>
    <p class="mt-2 max-w-2xl text-[13px] text-[var(--muted2)]">
      Backr scans your network for a host in pairing mode. On the host, open Backr →
      <strong>Add a laptop</strong> to get a 6-digit code.
    </p>
  </header>

  <section class="flex max-w-2xl flex-col gap-4">
    <div class="flex flex-wrap items-center gap-3">
      <button
        type="button"
        class="rounded-[6px] border border-[var(--border)] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--muted)] hover:border-[var(--accent)] hover:text-[var(--accent)] disabled:opacity-50"
        disabled={scanning}
        onclick={() => void scan()}
      >
        {scanning ? "Scanning…" : "Rescan"}
      </button>
      <button
        type="button"
        class="text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--accent)] hover:text-[var(--accent-hover)]"
        onclick={onManual}
      >
        Enter details manually
      </button>
    </div>

    {#if hosts.length === 0 && !scanning}
      <p class="text-[13px] text-[var(--muted2)]">
        No hosts found yet. Make sure the host has “Add a laptop” open, then rescan — or enter details manually.
      </p>
    {/if}

    {#each hosts as h (h.address)}
      <label
        class="flex cursor-pointer items-center gap-3 rounded-[6px] border px-4 py-3 panel-plate"
        class:border-[var(--accent)]={selected?.address === h.address}
        class:border-[var(--border)]={selected?.address !== h.address}
      >
        <input
          type="radio"
          name="host"
          checked={selected?.address === h.address}
          onchange={() => (selected = h)}
        />
        <span class="font-medium text-[var(--text)]">{h.hostname}</span>
        <span class="ml-auto font-mono text-[12px] text-[var(--muted)]">{h.address}</span>
      </label>
    {/each}

    {#if selected}
      <div class="flex flex-col gap-2">
        <label for="pair-code" class="label-caps text-[var(--muted)]">6-digit code shown on the host</label>
        <input
          id="pair-code"
          bind:value={code}
          inputmode="numeric"
          maxlength={6}
          placeholder="123456"
          class="max-w-[200px] rounded-[6px] border border-[var(--border)] bg-[var(--bg4)] px-3 py-2 font-mono text-xl tracking-[0.3em] text-[var(--text)] outline-none focus:border-[var(--accent)]"
        />
        <button
          type="button"
          class="mt-1 max-w-[200px] rounded-[6px] bg-[var(--accent)] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--bg)] hover:bg-[var(--accent-hover)] disabled:opacity-50"
          disabled={pairing || code.trim().length !== 6}
          onclick={() => void pair()}
        >
          {pairing ? "Pairing…" : "Pair"}
        </button>
      </div>
    {/if}

    {#if err}
      <p class="text-[13px] text-[var(--warn)]">{err}</p>
    {/if}
  </section>
</div>
