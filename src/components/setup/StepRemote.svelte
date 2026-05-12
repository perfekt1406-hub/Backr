<!--
  Purpose: Collects SSH endpoint identity plus isolated key paths used by rsync wrappers.
  Role: Binds into wizard draft `[remote]` fields consumed by `test_connection`.
-->
<script lang="ts">
  import type { RemoteConfig } from "../../types/config";

  interface Props {
    remote: RemoteConfig;
  }

  let { remote = $bindable() }: Props = $props();
</script>

<div class="grid max-w-3xl gap-6">
  <div class="grid gap-4 md:grid-cols-2">
    <label class="flex flex-col gap-2 text-[12px] text-[var(--muted)]">
      <span class="label-caps text-[var(--muted)]">Host</span>
      <input
        class="rounded-[5px] border border-[var(--border)] bg-[var(--bg4)] px-3 py-2 text-[var(--text)] panel-plate focus:border-[var(--accent)]"
        bind:value={remote.host}
        autocomplete="off"
      />
    </label>
    <label class="flex flex-col gap-2 text-[12px] text-[var(--muted)]">
      <span class="label-caps text-[var(--muted)]">User</span>
      <input
        class="rounded-[5px] border border-[var(--border)] bg-[var(--bg4)] px-3 py-2 text-[var(--text)] panel-plate focus:border-[var(--accent)]"
        bind:value={remote.user}
        autocomplete="username"
      />
    </label>
  </div>

  <label class="flex flex-col gap-2 text-[12px] text-[var(--muted)]">
    <span class="label-caps text-[var(--muted)]">SSH private key path</span>
    <input
      class="rounded-[5px] border border-[var(--border)] bg-[var(--bg4)] px-3 py-2 font-mono text-[13px] text-[var(--text)] panel-plate focus:border-[var(--accent)]"
      bind:value={remote.ssh_key}
      spellcheck="false"
    />
  </label>

  <div class="grid gap-4 md:grid-cols-2">
    <label class="flex flex-col gap-2 text-[12px] text-[var(--muted)]">
      <span class="label-caps text-[var(--muted)]">SSH port</span>
      <input
        type="number"
        min="1"
        max="65535"
        class="rounded-[5px] border border-[var(--border)] bg-[var(--bg4)] px-3 py-2 text-[var(--text)] panel-plate focus:border-[var(--accent)]"
        bind:value={remote.port}
      />
    </label>
    <div class="rounded-[5px] border border-[var(--border)] bg-[var(--bg3)] px-4 py-3 text-[12px] text-[var(--muted2)] panel-plate">
      Keys stay on disk — Backr isolates `known_hosts` under `~/.config/backr/` per backend policy.
    </div>
  </div>
</div>
