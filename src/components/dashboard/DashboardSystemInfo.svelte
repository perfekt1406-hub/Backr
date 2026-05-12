<!--
  Purpose: Shows hostname, distro/OS, kernel, arch, user, timezone, and live local time on the dashboard.
  Role: Loads static facts via [`getSystemInfo`] once; clock refreshes client-side every second.
-->
<script lang="ts">
  import { onMount } from "svelte";

  import { Cpu } from "lucide-svelte";

  import * as commands from "../../lib/commands";
  import type { SystemInfo } from "../../types/systemInfo";

  let info = $state<SystemInfo | null>(null);
  let loadErr = $state<string | null>(null);
  /** Human-readable local instant updated on an interval. */
  let localClock = $state("");

  /** Renders `when` using locale medium date + long time. */
  function formatLiveClock(when: Date): string {
    return new Intl.DateTimeFormat(undefined, {
      dateStyle: "medium",
      timeStyle: "long",
    }).format(when);
  }

  /** Schedules [`localClock`] refresh — external browser `Intl` API for timezone-aware formatting. */
  function tickClock(): void {
    localClock = formatLiveClock(new Date());
  }

  /** Pulls Rust-side snapshot then clears errors on success. */
  async function pull(): Promise<void> {
    try {
      info = await commands.getSystemInfo();
      loadErr = null;
    } catch (err) {
      loadErr = String(err);
      info = null;
    }
  }

  /** Placeholder for blank backend fields. */
  function dash(value: string | null | undefined): string {
    const t = value?.trim();
    return t ? t : "—";
  }

  /** IANA zone from the runtime (browser or webview). */
  const timeZone =
    typeof Intl !== "undefined"
      ? Intl.DateTimeFormat().resolvedOptions().timeZone ?? "—"
      : "—";

  /** UI language string when available. */
  const locale =
    typeof navigator !== "undefined" ? navigator.language || "—" : "—";

  onMount(() => {
    void pull();
    tickClock();
    const id = window.setInterval(() => tickClock(), 1000);
    return () => window.clearInterval(id);
  });
</script>

<section
  class="rounded-[8px] border border-[var(--border)] bg-[var(--bg2)] px-4 py-3 panel-plate"
  aria-label="System information"
>
  <div class="mb-3 flex items-center gap-2">
    <Cpu size={16} class="text-[var(--accent)]" aria-hidden="true" />
    <span class="label-caps text-[var(--muted)]">System information</span>
  </div>

  {#if loadErr}
    <p class="text-[12px] text-[var(--danger)]">{loadErr}</p>
  {:else if !info}
    <p class="text-[13px] text-[var(--muted2)]">Loading…</p>
  {:else}
    <dl class="grid grid-cols-[minmax(0,auto)_minmax(0,1fr)] gap-x-4 gap-y-2 text-[13px]">
      <dt class="text-[var(--muted)]">Host name</dt>
      <dd class="min-w-0 break-all font-mono text-[var(--text)]">{dash(info.hostname)}</dd>

      <dt class="text-[var(--muted)]">OS</dt>
      <dd class="min-w-0 leading-snug text-[var(--text)]">{dash(info.os_pretty)}</dd>

      <dt class="text-[var(--muted)]">Kernel</dt>
      <dd class="min-w-0 font-mono text-[12px] text-[var(--text)]">{dash(info.kernel_release)}</dd>

      <dt class="text-[var(--muted)]">Architecture</dt>
      <dd class="font-mono text-[var(--text)]">{dash(info.arch)}</dd>

      <dt class="text-[var(--muted)]">User</dt>
      <dd class="font-mono text-[var(--text)]">{dash(info.user)}</dd>

      <dt class="text-[var(--muted)]">Time zone</dt>
      <dd class="min-w-0 break-all font-mono text-[12px] text-[var(--text)]">{timeZone}</dd>

      <dt class="text-[var(--muted)]">Locale</dt>
      <dd class="font-mono text-[12px] text-[var(--text)]">{locale}</dd>

      <dt class="text-[var(--muted)]">Local time</dt>
      <dd class="leading-snug text-[var(--text)]">{localClock || "—"}</dd>
    </dl>
    <p class="mt-3 border-t border-[var(--border)] pt-2 text-[10px] text-[var(--muted2)]">
      Snapshot from shell · {info.sampled_at_rfc3339}
    </p>
  {/if}
</section>
