<!--
  Purpose: Global transient diagnostics anchored above routed screens.
  Role: Subscribes to `toastMessage` store populated after failed invokes.
-->
<script lang="ts">
  import { toastMessage } from "../../stores/ui";
</script>

{#if $toastMessage}
  <div
    class="fixed left-[272px] right-4 top-4 z-50 max-w-xl rounded-[8px] border border-[var(--danger)] bg-[var(--bg3)] px-4 py-3 text-[13px] text-[var(--danger)] shadow-lg panel-plate"
    role="status"
  >
    {$toastMessage}
  </div>
{/if}
