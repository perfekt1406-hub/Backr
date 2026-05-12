/*
 * Purpose: Maps filenames to CodeMirror language extensions for read-only preview panes.
 * Role: Keeps `@codemirror/lang-*` imports out of `.svelte` files for Tailwind’s extractor compatibility.
 */

import { css } from "@codemirror/lang-css";
import { html } from "@codemirror/lang-html";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { markdown } from "@codemirror/lang-markdown";
import { python } from "@codemirror/lang-python";
import { rust } from "@codemirror/lang-rust";
import { xml } from "@codemirror/lang-xml";
import { yaml } from "@codemirror/lang-yaml";
import type { Extension } from "@codemirror/state";

/**
 * Chooses syntax-highlighting support based on the file basename’s extension.
 *
 * External: each `@codemirror/lang-*` factory yields `LanguageSupport` merged into `EditorState`.
 */
export function extensionsForFilename(filename: string): Extension[] {
  const dot = filename.lastIndexOf(".");
  const ext = dot >= 0 ? filename.slice(dot + 1).toLowerCase() : "";

  switch (ext) {
    case "js":
    case "mjs":
    case "cjs":
      return [javascript()];
    case "jsx":
      return [javascript({ jsx: true })];
    case "ts":
      return [javascript({ typescript: true })];
    case "tsx":
      return [javascript({ jsx: true, typescript: true })];
    case "rs":
      return [rust()];
    case "json":
      return [json()];
    case "md":
      return [markdown()];
    case "toml":
      return [markdown()];
    case "css":
      return [css()];
    case "html":
    case "htm":
      return [html()];
    case "svg":
    case "xml":
      return [xml()];
    case "py":
      return [python()];
    case "yml":
    case "yaml":
      return [yaml()];
    case "svelte":
      return [html()];
    default:
      return [];
  }
}
