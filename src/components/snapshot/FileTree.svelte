<!--
  Purpose: Lazy directory navigator rooted at a single immutable snapshot revision.
  Role: Boots immediate children through `list_files` before handing expansion to `FileTreeNode`.
-->
<script lang="ts">
  import { onMount } from "svelte";

  import FileTreeNode from "./FileTreeNode.svelte";
  import * as commands from "../../lib/commands";
  import type { FileEntry } from "../../types/snapshot";

  interface Props {
    project: string;
    snapshot: string;
    /** Invokes UTF-8 preview (`read_snapshot_file`) when a leaf row fires `onPickFile`. */
    onPickFile?: (relativePath: string) => void;
  }

  let { project, snapshot, onPickFile }: Props = $props();

  let entries = $state<FileEntry[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  /** Mirrors directory-first ordering used inside recursive nodes. */
  function sortEntries(rows: FileEntry[]): FileEntry[] {
    return [...rows].sort((a, b) => {
      if (a.is_dir !== b.is_dir) {
        return a.is_dir ? -1 : 1;
      }
      return a.name.localeCompare(b.name);
    });
  }

  /** Loads shallow listing at snapshot root (`path=""`). */
  async function loadRoot(): Promise<void> {
    loading = true;
    error = null;
    try {
      entries = sortEntries(await commands.listFiles(project, snapshot, ""));
    } catch (err) {
      error = String(err);
      entries = [];
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void loadRoot();
  });
</script>

{#if loading}
  <p class="label-caps tracking-[0.2em] text-[var(--muted)]">Indexing snapshot root…</p>
{:else if error}
  <p class="text-[13px] text-[var(--danger)]">{error}</p>
{:else if entries.length === 0}
  <p class="text-[13px] text-[var(--muted)]">Empty snapshot tree.</p>
{:else}
  <ul class="flex flex-col gap-1 font-mono text-[12px]">
    {#each entries as entry}
      <li>
        <FileTreeNode {project} {snapshot} parentPath="" {entry} depth={0} {onPickFile} />
      </li>
    {/each}
  </ul>
{/if}
