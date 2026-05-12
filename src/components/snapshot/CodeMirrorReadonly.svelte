<!--
  Purpose: Non-editable CodeMirror 6 viewport for UTF-8 snapshot previews.
  Role: Mounts `EditorView` read-only; recreates the editor when `content` or `filename` changes.
-->
<script lang="ts">
  import { defaultKeymap } from "@codemirror/commands";
  import {
    bracketMatching,
    foldGutter,
    syntaxHighlighting,
    defaultHighlightStyle,
  } from "@codemirror/language";
  import { EditorState } from "@codemirror/state";
  import {
    EditorView,
    highlightActiveLineGutter,
    keymap,
    lineNumbers,
  } from "@codemirror/view";
  import { oneDark } from "@codemirror/theme-one-dark";

  import { extensionsForFilename } from "../../lib/codemirror/extensions";

  interface Props {
    /** Raw decoded UTF-8 from `read_snapshot_file`. */
    content: string;
    /** Basename or suffix — drives `@codemirror/lang-*` selection only. */
    filename: string;
  }

  let { content, filename }: Props = $props();

  let host = $state<HTMLDivElement | null>(null);

  /** Builds CM state/extensions — teardown destroys prior views bound to `host`. */
  $effect(() => {
    const el = host;
    const doc = content;
    const name = filename;
    if (!el) {
      return;
    }

    const extensions = [
      oneDark,
      lineNumbers(),
      highlightActiveLineGutter(),
      foldGutter(),
      bracketMatching(),
      syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
      EditorState.readOnly.of(true),
      EditorView.editable.of(false),
      EditorView.lineWrapping,
      keymap.of(defaultKeymap),
      EditorView.theme({
        "&": {
          height: "100%",
          maxHeight: "min(70vh, 560px)",
        },
        ".cm-scroller": {
          fontFamily: '"IBM Plex Mono", ui-monospace, monospace',
          fontSize: "13px",
          maxHeight: "min(70vh, 560px)",
        },
      }),
      ...extensionsForFilename(name),
    ];

    const state = EditorState.create({ doc, extensions });
    const view = new EditorView({ state, parent: el });
    return () => {
      view.destroy();
    };
  });
</script>

<div bind:this={host} class="min-h-[240px] rounded-[5px] border border-[var(--border)]"></div>
