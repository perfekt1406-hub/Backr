/*
 * Default rsync exclude patterns for backups.
 *
 * These cover regenerable build output, dependency directories, tool caches, and
 * OS/editor cruft across common toolchains. Excluding them keeps snapshots small and
 * fast (a Rust `target/` alone can dwarf the actual source by 1000x) without losing
 * anything irreplaceable — everything here is reproduced by a build, install, or the
 * OS itself.
 *
 * Safety principle: only unambiguous, regenerable paths belong here. Generic names
 * that frequently hold real source (`dist/`, `build/`, `out/`, `bin/`, `obj/`,
 * `vendor/`, `env/`, `.idea/`) are deliberately omitted so a backup never silently
 * drops wanted data. Add new entries as new tools are adopted — one entry per line,
 * grouped by ecosystem.
 *
 * Pattern semantics (rsync): a trailing `/` matches directories only; a name without
 * a leading slash matches at any depth in the tree (so `node_modules/` catches every
 * nested one). `*.ext` globs match files at any depth.
 */

/// Exclude patterns passed to `rsync` as `--exclude <pattern>` during backup.
/// Restore is intentionally unfiltered — we pull back exactly what was stored.
pub const BACKUP_EXCLUDES: &[&str] = &[
    // ── Rust / Cargo ──
    "target/",
    // ── Node / JavaScript / TypeScript ──
    "node_modules/",
    ".next/",        // Next.js
    ".nuxt/",        // Nuxt
    ".svelte-kit/",  // SvelteKit
    ".astro/",       // Astro
    ".vite/",        // Vite cache
    ".turbo/",       // Turborepo cache
    ".parcel-cache/",// Parcel
    ".angular/",     // Angular cache
    ".nyc_output/",  // nyc coverage
    "coverage/",     // test coverage reports
    // ── Python ──
    "__pycache__/",
    ".venv/",
    "venv/",
    ".mypy_cache/",
    ".pytest_cache/",
    ".ruff_cache/",
    ".tox/",
    ".ipynb_checkpoints/",
    "*.pyc",
    "*.pyo",
    // ── Mobile / native toolchains ──
    ".dart_tool/",   // Dart / Flutter
    "Pods/",         // CocoaPods (iOS)
    "DerivedData/",  // Xcode
    ".gradle/",      // Gradle (Java/Kotlin/Android)
    // ── Other ecosystems ──
    "_build/",       // Elixir / Erlang
    ".terraform/",   // Terraform provider cache
    ".cache/",       // generic tool cache dir
    // ── OS / editor cruft ──
    ".DS_Store",     // macOS
    "Thumbs.db",     // Windows
    "*.swp",         // vim swap
    "*.swo",
];
