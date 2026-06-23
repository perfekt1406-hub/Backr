<!--
  Purpose: Host "Add a laptop" panel — opens a time-boxed pairing window showing a
  6-digit code while the host advertises over mDNS, and confirms once a laptop pairs.
  Role: Embedded in the Trust-keys view; uses startPairing / stopPairing / pairingStatus.
-->
<script lang="ts">
  import { onDestroy } from "svelte";
  import { Laptop } from "lucide-svelte";

  import * as commands from "../../lib/commands";
  import type { PairingStarted } from "../../types/pairing";

  let started = $state<PairingStarted | null>(null);
  let remaining = $state(0);
  let busy = $state(false);
  let err = $state<string | null>(null);
  let paired = $state(false);

  let countdown: ReturnType<typeof setInterval> | undefined;
  let poll: ReturnType<typeof setInterval> | undefined;

  /** Stops both the countdown and the success poll. */
  function clearTimers(): void {
    if (countdown) clearInterval(countdown);
    if (poll) clearInterval(poll);
    countdown = undefined;
    poll = undefined;
  }

  /** Opens a pairing window and starts the countdown + success poll. */
  async function start(): Promise<void> {
    err = null;
    paired = false;
    busy = true;
    try {
      started = await commands.startPairing();
      const expiry = new Date(started.expires_at).getTime();
      const tick = (): void => {
        remaining = Math.max(0, Math.round((expiry - Date.now()) / 1000));
        if (remaining <= 0) void cancel();
      };
      tick();
      clearTimers();
      countdown = setInterval(tick, 1000);
      // A successful pair closes the window host-side, so pairing_status flips false.
      poll = setInterval(() => {
        void commands
          .pairingStatus()
          .then((open) => {
            if (!open && started) {
              paired = true;
              started = null;
              remaining = 0;
              clearTimers();
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

  /** Closes the pairing window. */
  async function cancel(): Promise<void> {
    clearTimers();
    started = null;
    remaining = 0;
    try {
      await commands.stopPairing();
    } catch {
      /* best-effort */
    }
  }

  /** Formats seconds as m:ss. */
  function fmt(total: number): string {
    const m = Math.floor(total / 60);
    const s = String(total % 60).padStart(2, "0");
    return `${m}:${s}`;
  }

  onDestroy(() => {
    clearTimers();
    void commands.stopPairing().catch(() => {});
  });
</script>

<section class="rounded-[8px] border border-[var(--border)] bg-[var(--bg2)] px-5 py-5 panel-plate">
  <div class="flex items-start gap-3">
    <Laptop size={22} class="mt-0.5 text-[var(--accent)]" aria-hidden="true" />
    <div class="flex-1">
      <h2 class="text-[15px] font-semibold text-[var(--text)]">Add a laptop</h2>
      <p class="mt-1 text-[12px] leading-relaxed text-[var(--muted2)]">
        The one-tap way to trust a laptop: open pairing, then on the laptop pick this host and enter the code.
      </p>

      {#if started}
        <div class="mt-4 flex flex-wrap items-center gap-4">
          <span class="font-mono text-3xl font-bold tracking-[0.3em] text-[var(--accent)]">{started.code}</span>
          <span class="label-caps text-[var(--muted)]">expires in {fmt(remaining)}</span>
          <button
            type="button"
            class="rounded-[6px] border border-[var(--border)] px-3 py-1.5 text-[11px] font-semibold uppercase tracking-[0.1em] text-[var(--muted)] hover:border-[var(--accent)] hover:text-[var(--accent)]"
            onclick={() => void cancel()}
          >
            Cancel
          </button>
        </div>
        <p class="mt-3 text-[12px] text-[var(--muted2)]">
          On the laptop: open Backr → pick this host → type the code above.
        </p>
      {:else if paired}
        <p class="mt-4 text-[13px] font-medium text-[var(--accent)]">
          ✓ Laptop paired — its key is trusted. Open pairing again to add another.
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
          {busy ? "Opening…" : "Add a laptop"}
        </button>
      {/if}

      {#if err}
        <p class="mt-3 text-[12px] text-[var(--warn)]">{err}</p>
      {/if}
    </div>
  </div>
</section>
