/*
 * Purpose: Optional callback slot so dev mock backup simulation can append rsync lines without importing `stores/backup`.
 * Role: Breaks a circular dependency (`commands` ↔ `stores/backup`).
 */

/** Buffered sink assigned once from `App.svelte` — invoked only when mock backup runs. */
let sink: ((line: string) => void) | null = null;

/**
 * Registers the console writer used by synthetic backup progress.
 *
 * External: typically wired to `appendProgressLine` from `stores/backup`.
 */
export function registerMockProgressAppender(fn: (line: string) => void): void {
  sink = fn;
}

/**
 * Pushes one rsync-style line when mock mode is exercising the dashboard console.
 *
 * External: no-op until `registerMockProgressAppender` runs during shell bootstrap.
 */
export function emitMockProgressLine(line: string): void {
  sink?.(line);
}
