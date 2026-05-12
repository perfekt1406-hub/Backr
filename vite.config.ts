/*
 * Purpose: Vite build configuration for the Backr Svelte webview.
 * Role: Enables Tailwind CSS v4 via `@tailwindcss/vite` and wires `@sveltejs/vite-plugin-svelte`.
 */

import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";

/** Bundler entrypoint — hosts dev server on port 1420 for `tauri dev`. */
export default defineConfig({
  plugins: [tailwindcss(), svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
});
