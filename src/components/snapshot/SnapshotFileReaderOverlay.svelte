<!--
  Purpose: Modal shell hosting lazy-loaded UTF-8 snapshot previews plus Escape dismissal.
  Role: Connects IPC/mock payloads with `CodeMirrorReadonly` for monospace syntax highlighting.
-->
<script lang="ts">
  import { onMount } from "svelte";

  import * as Lucide from "lucide-svelte";

  import CodeMirrorReadonly from "./CodeMirrorReadonly.svelte";
  import type { SnapshotFileContents } from "../../types/snapshot";

  interface Props {
    relativePath: string;
    loading: boolean;
    error: string | null;
    body: SnapshotFileContents | null;
    /** Clears preview state from parent routes when Escape/backdrop fires. */
    onClose: () => void;
  }

  let { relativePath, loading, error, body, onClose }: Props = $props();

  /** Filename suffix forwarded into CM language packs. */
  const basename = $derived(relativePath.split("/").pop() ?? relativePath);

  onMount(() => {
    /** Trap Escape while overlay mounted — avoids leaving orphaned fullscreen shells. */
    function esc(ev: KeyboardEvent): void {
      if (ev.key === "Escape") {
        onClose();
      }
    }
    window.addEventListener("keydown", esc);
    return () => window.removeEventListener("keydown", esc);
  });
</script>

<div
  class="fixed inset-0 z-[60] flex items-center justify-center bg-black/55 px-4 py-8"
  role="presentation"
  onclick={onClose}
>
  <div
    class="flex max-h-[90vh] w-full max-w-5xl flex-col overflow-hidden rounded-[8px] border border-[var(--border)] bg-[var(--bg3)] shadow-xl panel-plate"
    role="dialog"
    aria-modal="true"
    aria-labelledby="snapshot-preview-title"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
  >
    <header
      class="flex flex-wrap items-start justify-between gap-4 border-b border-[var(--border)] px-4 py-3"
    >
      <div class="min-w-0">
        <p id="snapshot-preview-title" class="label-caps text-[var(--muted)]">
          Read-only preview
        </p>
        <p class="mt-1 break-all font-mono text-[13px] text-[var(--text)]">{relativePath}</p>
        {#if body?.truncated}
          <p class="mt-2 text-[11px] uppercase tracking-[0.12em] text-[var(--warn)]">
            Truncated at server byte cap (512 KiB)
          </p>
        {/if}
      </div>
      <button
        type="button"
        class="inline-flex shrink-0 items-center gap-2 rounded-[5px] border border-[var(--border)] px-3 py-2 text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--muted)] hover:border-[var(--accent)] hover:text-[var(--accent)]"
        onclick={onClose}
        aria-label="Close preview"
      >
        <Lucide.X size={16} aria-hidden="true" />
        Close
      </button>
    </header>
    <div class="min-h-0 flex-1 overflow-auto px-4 py-4">
      {#if loading}
        <p class="text-[13px] text-[var(--muted)]">Loading…</p>
      {:else if error}
        <p class="text-[13px] text-[var(--danger)]">{error}</p>
      {:else if body}
        <CodeMirrorReadonly content={body.text} filename={basename} />
      {/if}
    </div>
  </div>
</div>
