/*
 * Purpose: Svelte compiler options shared by Vite and `svelte-check`.
 * Role: Applies `vitePreprocess()` so component `<style>` and TypeScript preprocess consistently.
 */

import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

/** Default export consumed by Vite and tooling for preprocessing pipelines. */
export default {
  preprocess: vitePreprocess(),
};
