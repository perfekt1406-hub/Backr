/*
 * Purpose: Ambient TypeScript declarations for Vite-powered Svelte modules.
 * Role: Ensures `import.meta.env` and `.svelte` imports type-check under strict mode.
 */

/// <reference types="svelte" />
/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_BACKR_MOCK?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
