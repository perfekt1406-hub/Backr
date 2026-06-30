/*
 * Purpose: Static datasets for dev mock mode — realistic shapes matching serde DTOs.
 * Role: Consumed exclusively when `useDevMock()` is true (`src/lib/devMock/backend.ts`).
 */

import type { ActivityPoint } from "../../types/activity";
import type { Config } from "../../types/config";
import type { ProjectInfo } from "../../types/project";
import type { FileEntry, SnapshotEntry, SnapshotFileContents } from "../../types/snapshot";

/** Baseline configuration shown on first mock load (wizard pre-fill uses live mutable copy). */
export function createInitialMockConfig(): Config {
  const last = new Date(Date.now() - 3_600_000).toISOString();
  return {
    version: 1,
    remote: {
      host: "192.168.1.50",
      user: "backup",
      ssh_key: "~/.ssh/id_ed25519_mock",
      port: 22,
      backup_path: "/srv/backr",
    },
    local: {
      projects_path: "/home/dev/Projects",
    },
    schedule: {
      interval_hours: 3,
    },
    state: {
      last_backup_at: last,
    },
    update: {
      auto_update: false,
    },
  };
}

/**
 * Builds a dashboard `ProjectInfo` row.
 *
 * @param name           project folder name shown in the list
 * @param snapshotCount  number of snapshots indexed for the project
 * @param minutesAgo     minutes since the last backup, or `null` for never-backed-up
 */
function projectRow(
  name: string,
  snapshotCount: number,
  minutesAgo: number | null,
): ProjectInfo {
  return {
    name,
    snapshot_count: snapshotCount,
    last_backup_at:
      minutesAgo === null ? null : new Date(Date.now() - minutesAgo * 60_000).toISOString(),
  };
}

/** Dashboard rows — snapshot/file browsing is project-agnostic, so any name resolves. */
export const MOCK_PROJECT_ROWS: ProjectInfo[] = [
  projectRow("acme-api", 12, 120),
  projectRow("backr-ui", 28, 7),
  projectRow("legacy-monolith", 0, null),
  projectRow("payments-service", 34, 45),
  projectRow("auth-gateway", 18, 90),
  projectRow("notification-worker", 9, 15),
  projectRow("analytics-pipeline", 52, 1440),
  projectRow("mobile-app", 0, null),
  projectRow("web-storefront", 21, 300),
  projectRow("admin-dashboard", 14, 25),
  projectRow("search-indexer", 40, 720),
  projectRow("image-resizer", 7, 180),
  projectRow("billing-cron", 30, 60),
  projectRow("graphql-gateway", 16, 5),
  projectRow("user-profile-svc", 25, 2880),
  projectRow("inventory-db", 11, 8),
  projectRow("recommendation-engine", 19, 480),
  projectRow("email-templates", 0, null),
  projectRow("infra-terraform", 6, 4320),
  projectRow("docs-site", 3, 35),
  projectRow("ml-training", 47, 1080),
  projectRow("event-bus", 22, 12),
  projectRow("cache-proxy", 8, 200),
];

/** Snapshot IDs shared across projects for predictable URLs during manual QA. */
export const MOCK_SNAPSHOT_NAMES = ["2026-05-11_14-30-00", "2026-05-10_09-00-00"];

/** Sparse activity ledger seeded before any simulated backup completes. */
export function seedActivityPoints(): ActivityPoint[] {
  return [
    {
      ts_unix: Math.floor((Date.now() - 86_400_000) / 1000),
      label: "backup_complete",
    },
    {
      ts_unix: Math.floor((Date.now() - 43_200_000) / 1000),
      label: "backup_complete",
    },
  ];
}

function ts(): number {
  return Math.floor(Date.now() / 1000);
}

/**
 * Immediate children for `(project, snapshot, relativePath)` synthetic browsing.
 * Paths are normalized without leading slashes (`""`, `src`, `src/components`).
 */
export function mockChildrenAt(relativePath: string): FileEntry[] {
  const key = relativePath.trim().replace(/^\/+|\/+$/g, "");
  const tree: Record<string, FileEntry[]> = {
    "": [
      {
        name: "src",
        is_dir: true,
        size: 0,
        modified_unix: ts(),
      },
      {
        name: "tests",
        is_dir: true,
        size: 0,
        modified_unix: ts(),
      },
      {
        name: "README.md",
        is_dir: false,
        size: 1823,
        modified_unix: ts(),
      },
      {
        name: "Cargo.toml",
        is_dir: false,
        size: 604,
        modified_unix: ts(),
      },
    ],
    src: [
      {
        name: "main.rs",
        is_dir: false,
        size: 4421,
        modified_unix: ts(),
      },
      {
        name: "lib.rs",
        is_dir: false,
        size: 9088,
        modified_unix: ts(),
      },
      {
        name: "components",
        is_dir: true,
        size: 0,
        modified_unix: ts(),
      },
    ],
    "src/components": [
      {
        name: "App.svelte",
        is_dir: false,
        size: 2310,
        modified_unix: ts(),
      },
      {
        name: "routes.ts",
        is_dir: false,
        size: 884,
        modified_unix: ts(),
      },
    ],
    tests: [
      {
        name: "integration.rs",
        is_dir: false,
        size: 5600,
        modified_unix: ts(),
      },
    ],
  };
  return tree[key] ?? [];
}

/** Synthetic UTF-8 bodies keyed by normalized paths (`src/main.rs`). */
const MOCK_TEXT_BODIES: Record<string, string> = {
  "README.md":
    "# Demo crate\n\nSynthetic tree for **Backr** snapshot browsing.\n\n```bash\ncargo test\n```\n",
  "Cargo.toml":
    '[package]\nname = "demo"\nversion = "0.1.0"\nedition = "2021"\n\n[dependencies]\nserde = { version = "1", features = ["derive"] }\n',
  "src/main.rs": 'fn main() {\n    println!("mock snapshot");\n}\n',
  "src/lib.rs":
    "//! Library root\n\n/// Adds two integers.\npub fn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n",
  "src/components/App.svelte":
    '<script lang="ts">\n  let title = "Backr";\n</script>\n\n<main class="app">{title}</main>\n\n<style>\n  .app { font-family: system-ui; }\n</style>\n',
  "src/components/routes.ts":
    'export default {\n  "/": () => import("./Home.svelte"),\n  "/setup": () => import("./Setup.svelte"),\n};\n',
  "tests/integration.rs": '#[cfg(test)]\nmod tests {\n    #[test]\n    fn smoke() {\n        assert_eq!(2 + 2, 4);\n    }\n}\n',
};

/**
 * Resolves mock file contents for CodeMirror previews (`read_snapshot_file` stand-in).
 *
 * External: keyed lookup only — mirrors bounded SSH `head` semantics without binary payloads.
 */
export function mockSnapshotFileContents(relativePath: string): SnapshotFileContents {
  const key = relativePath.trim().replace(/^\/+|\/+$/g, "");
  const hit = MOCK_TEXT_BODIES[key];
  if (hit !== undefined) {
    return { text: hit, truncated: false };
  }
  return {
    text: `// No mock fixture for "${key}"\n// Remote mode streams UTF-8 via SSH + head -c.\n`,
    truncated: false,
  };
}

/** Synthetic snapshot rows for any project name (same timestamps for simpler QA). */
export function mockSnapshotsForProject(_project: string): SnapshotEntry[] {
  return MOCK_SNAPSHOT_NAMES.map((name) => ({ name }));
}
