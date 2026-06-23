<!--
  Purpose: Three-step onboarding aligning SSH targets with local project roots before saving disk config.
  Role: Orchestrates `StepRemote`, `StepPaths`, and `StepVerify` with deterministic navigation guards.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { replace } from "svelte-spa-router";

  import type { Config } from "../../types/config";
  import { shellKind } from "../../stores/shell";
  import PairHost from "./PairHost.svelte";
  import StepPaths from "./StepPaths.svelte";
  import StepRemote from "./StepRemote.svelte";
  import StepVerify from "./StepVerify.svelte";

  let step = $state(0);
  let mode = $state<"discover" | "wizard">("discover");

  /** Seeds wizard fields until persistence succeeds. */
  function emptyConfig(): Config {
    return {
      remote: {
        host: "",
        user: "",
        ssh_key: "",
        port: 22,
        backup_path: "",
      },
      local: { projects_path: "" },
      schedule: { interval_hours: 3 },
      state: { last_backup_at: null },
    };
  }

  let draft = $state<Config>(emptyConfig());

  /** Adopts a paired host's prefilled config and jumps to the verify step. */
  function adoptPairedConfig(cfg: Config): void {
    draft = cfg;
    mode = "wizard";
    step = 2;
  }

  /** Switches from discovery to manual entry with a blank draft. */
  function enterManual(): void {
    mode = "wizard";
    step = 0;
  }

  onMount(() => {
    if (get(shellKind) === "host") {
      replace("/host");
    }
  });

  /** Advances linearly while blocking empty mandatory remote fields. */
  function nextFromRemote(): void {
    const r = draft.remote;
    if (!r.host.trim() || !r.user.trim() || !r.ssh_key.trim()) {
      window.alert("Complete SSH host, user, and key path before continuing.");
      return;
    }
    step = 1;
  }

  /** Validates roots before SSH verification. */
  function nextFromPaths(): void {
    if (!draft.local.projects_path.trim() || !draft.remote.backup_path.trim()) {
      window.alert("Projects path and remote backup root are mandatory.");
      return;
    }
    step = 2;
  }

  /** Navigates to the dashboard after configuration persists. */
  function finish(): void {
    replace("/");
  }
</script>

{#if mode === "discover"}
  <PairHost onPaired={adoptPairedConfig} onManual={enterManual} />
{:else}
<div class="flex flex-1 flex-col gap-8 px-10 py-10">
  <header class="border-b border-[var(--border)] pb-6">
    <p class="label-caps mb-2 text-[var(--muted)]">Initial setup</p>
    <h1 class="text-2xl font-semibold tracking-tight text-[var(--text)]">
      Configure Backr
    </h1>
    <p class="mt-2 max-w-2xl text-[13px] text-[var(--muted2)]">
      Set SSH access, local project root, remote backup path, and backup interval. Test connectivity before
      saving—scheduled rsync snapshots start after a valid config is written.
    </p>
    <div class="mt-6 flex gap-2">
      {#each ["Remote", "Roots", "Verify"] as label, i}
        <button
          type="button"
          class="rounded-[5px] border px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.14em] transition-colors"
          class:border-[var(--accent)]={step === i}
          class:bg-[var(--bg3)]={step === i}
          class:text-[var(--accent)]={step === i}
          class:border-[var(--border)]={step !== i}
          class:text-[var(--muted)]={step !== i}
          onclick={() => {
            step = i;
          }}
        >
          {String(i).padStart(2, "0")} · {label}
        </button>
      {/each}
    </div>
  </header>

  {#if step === 0}
    <StepRemote bind:remote={draft.remote} />
    <div class="flex justify-end gap-3">
      <button
        type="button"
        class="rounded-[5px] border border-[var(--accent)] bg-[var(--accent)] px-5 py-2 text-[13px] font-semibold uppercase tracking-[0.12em] text-[var(--bg)] transition hover:bg-[var(--accent-hover)] active:bg-[var(--accent-pressed)]"
        onclick={nextFromRemote}
      >
        Continue
      </button>
    </div>
  {:else if step === 1}
    <StepPaths bind:draft />
    <div class="flex justify-between gap-3">
      <button
        type="button"
        class="rounded-[5px] border border-[var(--border)] px-4 py-2 text-[12px] uppercase tracking-[0.12em] text-[var(--muted)] hover:border-[var(--border-glow)]"
        onclick={() => {
          step = 0;
        }}
      >
        Back
      </button>
      <button
        type="button"
        class="rounded-[5px] border border-[var(--accent)] bg-[var(--accent)] px-5 py-2 text-[13px] font-semibold uppercase tracking-[0.12em] text-[var(--bg)] transition hover:bg-[var(--accent-hover)]"
        onclick={nextFromPaths}
      >
        Continue
      </button>
    </div>
  {:else}
    <StepVerify bind:draft onDone={finish} />
    <div class="flex justify-between gap-3">
      <button
        type="button"
        class="rounded-[5px] border border-[var(--border)] px-4 py-2 text-[12px] uppercase tracking-[0.12em] text-[var(--muted)] hover:border-[var(--border-glow)]"
        onclick={() => {
          step = 1;
        }}
      >
        Back
      </button>
    </div>
  {/if}
</div>
{/if}
