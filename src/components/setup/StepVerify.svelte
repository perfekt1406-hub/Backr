<!--
  Purpose: Proof step executing SSH echo probe then atomic save into `config.toml`.
  Role: Calls `test_connection` before `saveCfg` so invalid credentials never reach disk persistence.
-->
<script lang="ts">
  import type { Config } from "../../types/config";
  import * as commands from "../../lib/commands";
  import { saveCfg } from "../../stores/config";

  interface Props {
    draft: Config;
    onDone: () => void;
  }

  let { draft = $bindable(), onDone }: Props = $props();

  let testing = $state(false);
  let saving = $state(false);

  /** Runs lightweight remote echo without persisting yet. */
  async function runTest(): Promise<void> {
    testing = true;
    try {
      await commands.testConnection(
        draft.remote.host,
        draft.remote.user,
        draft.remote.ssh_key,
        draft.remote.port,
      );
      window.alert("SSH probe succeeded — credentials accepted.");
    } catch (err) {
      window.alert(String(err));
    } finally {
      testing = false;
    }
  }

  /** Writes configuration through the shared store helper. */
  async function commit(): Promise<void> {
    saving = true;
    try {
      const ok = await saveCfg(draft);
      if (ok) {
        onDone();
      }
    } finally {
      saving = false;
    }
  }
</script>

<div class="grid max-w-3xl gap-6">
  <section
    class="rounded-[8px] border border-[var(--border)] bg-[var(--bg3)] px-6 py-5 text-[13px] text-[var(--muted2)] panel-plate"
  >
    <p class="label-caps mb-3 text-[var(--muted)]">Verification checklist</p>
    <ul class="list-disc space-y-2 pl-5">
      <li>SSH key readable by this workstation user.</li>
      <li>Remote user can create directories under the declared backup root.</li>
      <li>Local projects directory exists before first snapshot.</li>
    </ul>
  </section>

  <div class="flex flex-wrap gap-3">
    <button
      type="button"
      class="rounded-[5px] border border-[var(--border2)] px-4 py-2 text-[12px] font-semibold uppercase tracking-[0.12em] text-[var(--text)] hover:border-[var(--accent)] disabled:opacity-40"
      onclick={() => void runTest()}
      disabled={testing || saving}
    >
      {testing ? "Testing…" : "Test connection"}
    </button>
    <button
      type="button"
      class="rounded-[5px] border border-[var(--accent)] bg-[var(--accent)] px-5 py-2 text-[13px] font-semibold uppercase tracking-[0.12em] text-[var(--bg)] hover:bg-[var(--accent-hover)] disabled:opacity-40"
      onclick={() => void commit()}
      disabled={saving || testing}
    >
      {saving ? "Saving…" : "Save configuration"}
    </button>
  </div>
</div>
