<!--
  Purpose: Host "Add a laptop" panel — a button starts broadcasting a 6-digit
  pairing code (mDNS + listener); the window stays open until a laptop pairs (or
  the host stops it), then confirms.
  Role: Used standalone in the Trust-keys view ("card" mode) and inline in the
        HostSetupGuide action zone ("inline" mode, no outer wrapper).
-->
<script lang="ts">
  import { onDestroy } from "svelte";
  import { Laptop } from "lucide-svelte";

  import * as commands from "../../lib/commands";

  /**
   * Display mode:
   *   "card"   — default; renders a full bordered card with heading and intro text.
   *   "inline" — flat/borderless; content only, no card wrapper (for embedding inside
   *              another card's action zone).
   *
   * The legacy `embedded` boolean prop is preserved as an alias for mode="inline".
   */
  let {
    mode = "card",
    embedded = false,
  }: { mode?: "card" | "inline"; embedded?: boolean } = $props();

  /** Resolve effective mode — `embedded` prop acts as shorthand for mode="inline". */
  const isInline = $derived(mode === "inline" || embedded);

  let broadcasting = $state(false);
  let code = $state<string | null>(null);
  let busy = $state(false);
  let err = $state<string | null>(null);
  let paired = $state(false);

  let poll: ReturnType<typeof setInterval> | undefined;

  /** Clears the polling interval. */
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

<!--
  Outer wrapper: full card in "card" mode, transparent/flat in "inline" mode.
  Inner content is identical between modes so behaviour is always the same.
-->
{#if isInline}
  <!-- Inline mode: no border, no background — just the action content. -->
  <div class="flex flex-col gap-3">
    {@render content()}
  </div>
{:else}
  <!-- Card mode: standalone bordered panel with heading and intro. -->
  <section class="rounded-[8px] border border-[var(--border)] bg-[var(--bg2)] px-5 py-5 panel-plate">
    <div class="flex items-start gap-3">
      <Laptop size={22} class="mt-0.5 text-[var(--accent)]" aria-hidden="true" />
      <div class="flex-1">
        <h2 class="text-[15px] font-semibold text-[var(--text)]">Add a laptop</h2>
        <p class="mt-1 text-[12px] leading-relaxed text-[var(--muted2)]">
          Start broadcasting, then on the laptop pick this host and enter the code. Pairing ends automatically once a laptop connects.
        </p>
        {@render content()}
      </div>
    </div>
  </section>
{/if}

<!--
  Shared interactive content: Start button → broadcasting code display → paired confirmation.
  Rendered the same way regardless of card vs inline mode.
-->
{#snippet content()}
  {#if broadcasting && code}
    <!-- Broadcasting state: large code + status + stop button -->
    <div class="flex flex-wrap items-center gap-4 {isInline ? '' : 'mt-4'}">
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
    <!-- Paired state: confirmation + option to add another -->
    <p class="{isInline ? '' : 'mt-4'} text-[13px] font-medium text-[var(--accent)]">
      ✓ Laptop paired — its key is trusted.
    </p>
    <button
      type="button"
      class="mt-3 self-start rounded-[6px] bg-[var(--accent)] px-5 py-2.5 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--bg)] hover:bg-[var(--accent-hover)]"
      onclick={() => void start()}
    >
      Add another laptop
    </button>
  {:else}
    <!-- Idle state: Start pairing button -->
    <button
      type="button"
      class="{isInline ? 'self-start' : 'mt-4'} rounded-[6px] bg-[var(--accent)] px-5 py-2.5 text-[12px] font-semibold uppercase tracking-[0.1em] text-[var(--bg)] hover:bg-[var(--accent-hover)] disabled:opacity-50"
      disabled={busy}
      onclick={() => void start()}
    >
      {busy ? "Starting…" : "Start pairing"}
    </button>
  {/if}

  {#if err}
    <p class="mt-3 text-[12px] text-[var(--warn)]">{err}</p>
  {/if}
{/snippet}
