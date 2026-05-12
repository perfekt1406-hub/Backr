<!--
  Purpose: Read-only browsing plane for historical snapshot directories plus UTF-8 file previews.
  Role: Breadcrumb navigation back to project list, lazy tree listing, and CodeMirror modal previews.
-->
<script lang="ts">
  import { link } from "svelte-spa-router";

  import FileTree from "./FileTree.svelte";
  import SnapshotFileReaderOverlay from "./SnapshotFileReaderOverlay.svelte";
  import * as commands from "../../lib/commands";
  import type { SnapshotFileContents } from "../../types/snapshot";

  interface Props {
    params?: { name?: string; snapshot?: string };
  }

  let { params = {} }: Props = $props();

  const project = $derived(decodeURIComponent(params.name ?? ""));
  const snapshot = $derived(decodeURIComponent(params.snapshot ?? ""));

  let previewPath = $state<string | null>(null);
  let previewLoading = $state(false);
  let previewError = $state<string | null>(null);
  let previewBody = $state<SnapshotFileContents | null>(null);

  /** Fetches bounded UTF-8 through IPC/mock — surfaces binary decode failures as toast strings. */
  async function openPreview(relPath: string): Promise<void> {
    previewPath = relPath;
    previewLoading = true;
    previewError = null;
    previewBody = null;
    try {
      previewBody = await commands.readSnapshotFile(project, snapshot, relPath);
    } catch (err) {
      previewError = String(err);
    } finally {
      previewLoading = false;
    }
  }

  /** Clears overlay state after Escape/backdrop or Close — avoids stale buffers while navigating. */
  function closePreview(): void {
    previewPath = null;
    previewLoading = false;
    previewError = null;
    previewBody = null;
  }
</script>

<div class="flex min-h-0 flex-1 flex-col px-10 py-10">
  <div class="mb-6 flex flex-wrap items-center gap-3 border-b border-[var(--border)] pb-6 text-[12px] uppercase tracking-[0.14em] text-[var(--muted)]">
    <a href="/" class="hover:text-[var(--accent)]" use:link>Projects</a>
    <span class="text-[var(--border2)]">/</span>
    <a
      href={`/project/${encodeURIComponent(project)}`}
      class="hover:text-[var(--accent)]"
      use:link>{project}</a>
    <span class="text-[var(--border2)]">/</span>
    <span class="break-all font-mono text-[var(--text)]">{snapshot}</span>
  </div>

  <section
    class="flex min-h-0 flex-1 flex-col rounded-[8px] border border-[var(--border)] bg-[var(--bg3)] px-5 py-4 panel-plate"
  >
    <header class="mb-4">
      <p class="label-caps text-[var(--muted)]">Snapshot tree</p>
      <p class="mt-2 text-[13px] text-[var(--muted2)]">
        Expand directories lazily via `list_files`. Click a file for a read-only preview (UTF-8,
        capped server-side).
      </p>
    </header>
    <div class="min-h-0 flex-1 overflow-auto">
      <FileTree {project} {snapshot} onPickFile={openPreview} />
    </div>
  </section>
</div>

{#if previewPath !== null}
  <SnapshotFileReaderOverlay
    relativePath={previewPath}
    loading={previewLoading}
    error={previewError}
    body={previewBody}
    onClose={closePreview}
  />
{/if}
