<!--
  Purpose: Dual-state badge for scheduler mutex alignment with tray semantics.
  Role: Encodes `in_progress` vs idle without playful illustration metaphors.
-->
<script lang="ts">
  interface Props {
    active: boolean;
    detail?: string | null;
  }

  let { active, detail = null }: Props = $props();
</script>

{#if active}
  <span
    class="inline-flex items-center gap-2 rounded-[5px] border border-[var(--warn)] bg-[var(--bg4)] px-2 py-0.5 text-[11px] font-semibold uppercase tracking-[0.14em] text-[var(--warn)]"
  >
    <span class="relative flex h-2 w-2">
      <span
        class="absolute inline-flex h-full w-full animate-ping rounded-full bg-[var(--warn)] opacity-40"
      ></span>
      <span class="relative inline-flex h-2 w-2 rounded-full bg-[var(--warn)]"></span>
    </span>
    RUNNING {#if detail}<span class="normal-case text-[var(--muted2)]">{detail}</span>{/if}
  </span>
{:else}
  <span
    class="inline-flex items-center gap-2 rounded-[5px] border border-[var(--border)] px-2 py-0.5 text-[11px] font-semibold uppercase tracking-[0.14em] text-[var(--muted)]"
  >
    IDLE
  </span>
{/if}
