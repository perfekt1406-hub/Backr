<!--
  Purpose: Trigger on-demand snapshot sync for all projects or one directory.
  Role: Delegates to `requestBackup` while honoring mutex guardrails from Rust state.
-->
<script lang="ts">
  import { Zap } from "lucide-svelte";

  import { requestBackup } from "../../stores/backup";

  interface Props {
    busy: boolean;
    /** Optional subdirectory name under the configured projects root. */
    project?: string;
    /** Wide variant for dashboard hero placement. */
    variant?: "primary" | "ghost";
  }

  let { busy, project, variant = "primary" }: Props = $props();

  /** Dispatches IPC job after toggling disabled chrome upstream. */
  async function click(): Promise<void> {
    await requestBackup(project);
  }
</script>

<button
  type="button"
  class="inline-flex items-center gap-2 rounded-[5px] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.14em] transition disabled:cursor-not-allowed disabled:opacity-35"
  class:border={variant === "ghost"}
  class:border-solid={variant === "ghost"}
  class:border-[var(--border)]={variant === "ghost"}
  class:bg-transparent={variant === "ghost"}
  class:bg-[var(--accent)]={variant === "primary"}
  class:border-[var(--accent)]={variant === "primary"}
  class:text-[var(--bg)]={variant === "primary"}
  class:text-[var(--text)]={variant === "ghost"}
  class:hover:bg-[var(--accent-hover)]={variant === "primary"}
  class:hover:border-[var(--accent)]={variant === "ghost"}
  disabled={busy}
  onclick={() => void click()}
>
  <Zap size={16} aria-hidden="true" />
  {#if project}
    Backup {project}
  {:else}
    Backup all projects
  {/if}
</button>
