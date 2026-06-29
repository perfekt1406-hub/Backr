<!--
  Purpose: Recursive tree row handling directory expansion via memoized SSH listings.
  Role: Hydrates children only after explicit expansion using composite cache keys from `stores/snapshots`.
-->
<script lang="ts">
  /**
   * Namespace import avoids Tailwind v4’s scanner treating `File as …` inside `{ … }`
   * import lists as invalid CSS declarations (`@tailwindcss/vite`).
   */
  import * as Lucide from "lucide-svelte";
  import { get } from "svelte/store";

  import FileTreeNode from "./FileTreeNode.svelte";
  import * as commands from "../../lib/commands";
  import type { FileEntry } from "../../types/snapshot";
  import { filesCache, filesCacheKey } from "../../stores/snapshots";

  interface Props {
    project: string;
    snapshot: string;
    /** Relative directory containing this entry (`""` at snapshot root). */
    parentPath: string;
    entry: FileEntry;
    depth: number;
    /** Opens UTF-8 preview — omit when snapshots should stay browse-only. */
    onPickFile?: (relativePath: string) => void;
  }

  let { project, snapshot, parentPath, entry, depth, onPickFile }: Props = $props();

  let expanded = $state(false);
  let loading = $state(false);
  let children = $state<FileEntry[]>([]);
  let hydrated = $state(false);
  /** Non-null when listing this directory failed, so the row can surface it instead of silently showing nothing. */
  let error = $state<string | null>(null);

  /** Remote relative path passed into `list_files` for this node's directory payload. */
  const pathForListing = $derived(
    parentPath ? `${parentPath}/${entry.name}` : entry.name,
  );

  /** Sort directories ahead of files for scan readability. */
  function sortEntries(rows: FileEntry[]): FileEntry[] {
    return [...rows].sort((a, b) => {
      if (a.is_dir !== b.is_dir) {
        return a.is_dir ? -1 : 1;
      }
      return a.name.localeCompare(b.name);
    });
  }

  /** Loads immediate children once per expansion cycle with cross-node caching. */
  async function hydrate(): Promise<void> {
    loading = true;
    error = null;
    try {
      const key = filesCacheKey(project, snapshot, pathForListing);
      const cache = get(filesCache);
      const hit = cache.get(key);
      if (hit) {
        children = sortEntries(hit);
      } else {
        const rows = await commands.listFiles(project, snapshot, pathForListing);
        const sorted = sortEntries(rows);
        filesCache.update((m) => new Map(m).set(key, sorted));
        children = sorted;
      }
      hydrated = true;
    } catch (err) {
      // Surface the failure on this row; without this the chevron just opened to nothing.
      error = err instanceof Error ? err.message : String(err);
      hydrated = false;
    } finally {
      loading = false;
    }
  }

  /** Toggles expansion while respecting directory-only semantics. */
  async function toggle(): Promise<void> {
    if (!entry.is_dir) {
      return;
    }
    expanded = !expanded;
    if (expanded && !hydrated) {
      await hydrate();
    }
  }

  /** Directory rows expand/collapse; file rows invoke preview callback when wired. */
  async function onRowActivate(): Promise<void> {
    if (entry.is_dir) {
      await toggle();
    } else if (onPickFile) {
      onPickFile(pathForListing);
    }
  }
</script>

<div style={`padding-left:${depth * 14}px`}>
  <button
    type="button"
    class="flex w-full items-center gap-2 rounded-[5px] px-2 py-1 text-left transition hover:bg-[var(--bg4)]"
    class:cursor-pointer={entry.is_dir || !!onPickFile}
    class:cursor-default={!entry.is_dir && !onPickFile}
    onclick={() => void onRowActivate()}
  >
    {#if entry.is_dir}
      <Lucide.ChevronRight
        size={14}
        class={`shrink-0 text-[var(--muted)] transition ${expanded ? "rotate-90" : ""}`}
      />
      <Lucide.Folder size={14} class="shrink-0 text-[var(--accent)]" />
    {:else}
      <span class="inline-block w-[14px] shrink-0"></span>
      <Lucide.File size={14} class="shrink-0 text-[var(--muted2)]" />
    {/if}
    <span class="truncate text-[var(--text)]">{entry.name}</span>
    {#if !entry.is_dir && entry.size > 0}
      <span class="ml-auto shrink-0 text-[11px] text-[var(--muted)]">
        {entry.size.toLocaleString()} B
      </span>
    {/if}
    {#if entry.is_dir && loading}
      <span class="ml-auto text-[10px] uppercase tracking-[0.14em] text-[var(--muted)]">…</span>
    {/if}
  </button>

  {#if entry.is_dir && expanded && error}
    <p
      style={`padding-left:${(depth + 1) * 14 + 8}px`}
      class="py-1 text-[11px] text-[var(--warn)]"
    >
      {error}
    </p>
  {/if}

  {#if entry.is_dir && expanded && hydrated && children.length > 0}
    <ul class="mt-1 flex flex-col gap-1 border-l border-[var(--border)] pl-2">
      {#each children as child}
        <li>
          <FileTreeNode
            {project}
            {snapshot}
            parentPath={pathForListing}
            entry={child}
            depth={depth + 1}
            {onPickFile}
          />
        </li>
      {/each}
    </ul>
  {/if}
</div>
