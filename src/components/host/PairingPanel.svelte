<!--
  Purpose: Host "Add a laptop" panel — a button starts broadcasting a 6-digit
  pairing code (mDNS + listener); the window stays open until a laptop pairs (or
  the host stops it), then confirms.
  Role: Embedded in the Trust-keys view; uses startPairing / stopPairing / pairingStatus.
-->
<script lang="ts">
  import { onDestroy } from "svelte";
  import { Laptop } from "lucide-svelte";

  import * as commands from "../../lib/commands";

  /** When embedded in the host setup guide, drop the card's own heading/icon/intro. */
  let { embedded = false }: { embedded?: boolean } = $props();

  let broadcasting = $state(false);
  let code = $state<string | null>(null);
  let busy = $state(false);
  let err = $state<string | null>(null);
  let paired = $state(false);

  let poll: ReturnType<typeof setInterval> | undefined;

  function clearPoll(): void {
    if (poll) clearInterval(poll);
    poll = undefined;
  }

  /** Starts broadcasting a pairing code; ends when a laptop pairs (window closes host-side). */
  async function start(): Promise<void> {
    err = null;
    paired = false;
    busy = true;
    try {
      const started = await commands.startPairing();
      code = started.code;
      broadcasting = true;
      clearPoll();
      poll = setInterval(() => {
        void commands
          .pairingStatus()
          .then((open) => {
            if (!open && broadcasting) {
              paired = true;
              broadcasting = false;
              code = null;
              clearPoll();
            }
          })
          .catch(() => {});
      }, 1500);
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  /** Stops broadcasting without pairing. */
  async function stop(): Promise<void> {
    clearPoll();
    broadcasting = false;
    code = null;
    try {
      await commands.stopPairing();
    } catch {
      /* best-effort */
    }
  }

  onDestroy(() => {
    clearPoll();
    void commands.stopPairing().catch(() => {});
  });
</script>

<section class="rounded-[8px] border border-[var(--border)] bg-[var(--bg2)] px-5 py-5 panel-plate">
  <div class="flex items-start gap-3">
    {#if !embedded}
      <Laptop size={22} class="mt-0.5 text-[var(--accent)]" aria-hidden="true" />
    {/if}
    <div class="flex-1">
      {#if !embedded}
        <h2 class="text-[15px] font-semibold text-[var(--text)]">Add a laptop</h2>
        <p class="mt-1 text-[12px] leading-relaxed text-[var(--muted2)]">
          Start broadcasting, then on the laptop pick this host and enter the code. Pairing ends automatically once a laptop connects.
        </p>
      {/if}

      {#if broadcasting && code}
        <div class="mt-4 flex flex-wrap items-center gap-4">
          <span class="font-mono text-3xl font-bold tracking-[0.3em] text-[var(--accent)]">{code}</span>
          <span class="label-caps text-[var(--muted)]">broadcasting — waiting for a laptop…</span>
          <button
            type="button"
            class="rounded-[6px] border border-[var(--border)] px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.1em] text-[var(--muted)] hover:border-[var(--accent)] hover:text-[var(--accent)]"
            onclick={() => void stop()}
          >
            Stop
          </button>
        </div>
        <p class="mt-3 text-[12px] text-[var(--muted2)]">
          On the laptop: open Backr → pick this host → type the code above.
        </p>
      {:else if paired}
        <p class="mt-4 text-[13px] font-medium text-[var(--accent)]">
          ✓ Laptop paired — its key is trusted.
        </p>
        <button
          type="button"
          class="mt-3 rounded-[6px] bg-[var(--accent)] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--bg)] hover:bg-[var(--accent-hover)]"
          onclick={() => void start()}
        >
          Add another laptop
        </button>
      {:else}
        <button
          type="button"
          class="mt-4 rounded-[6px] bg-[var(--accent)] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--bg)] hover:bg-[var(--accent-hover)] disabled:opacity-50"
          disabled={busy}
          onclick={() => void start()}
        >
          {busy ? "Starting…" : "Start pairing"}
        </button>
      {/if}

      {#if err}
        <p class="mt-3 text-[12px] text-[var(--warn)]">{err}</p>
      {/if}
    </div>
  </div>
</section>
