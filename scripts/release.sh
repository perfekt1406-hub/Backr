#!/usr/bin/env bash
#
# Purpose: Cut a Backr release. Bumps the single [workspace.package] version,
#          commits it, and creates a vX.Y.Z tag. Pushing that tag triggers
#          .github/workflows/release.yml, which builds and uploads the Linux
#          binaries the self-update client downloads.
# Role: One-command versioning helper. It does NOT push — you push when ready,
#       so cutting a tag is always a deliberate step.
#
# Usage:
#   scripts/release.sh X.Y.Z      (e.g. scripts/release.sh 0.4.2)
#   then: git push && git push origin vX.Y.Z
#
set -euo pipefail

ver="${1:-}"
if [[ ! "$ver" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: scripts/release.sh X.Y.Z   (e.g. scripts/release.sh 0.4.2)" >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
cd "$repo_root"

if [[ -n "$(git status --porcelain --untracked-files=no)" ]]; then
  echo "error: working tree has uncommitted tracked changes — commit or stash first." >&2
  exit 1
fi

if git rev-parse -q --verify "refs/tags/v${ver}" >/dev/null; then
  echo "error: tag v${ver} already exists." >&2
  exit 1
fi

# Bump the one workspace version (crates inherit it via version.workspace = true).
# Only the first `version = "X.Y.Z"` line (under [workspace.package]) is rewritten.
sed -i.bak -E "0,/^version = \"[0-9]+\.[0-9]+\.[0-9]+\"/s//version = \"${ver}\"/" Cargo.toml
rm -f Cargo.toml.bak

# Refresh the lockfile entries for the workspace crates.
cargo update --workspace >/dev/null 2>&1 || true

git add Cargo.toml Cargo.lock
git commit -m "release: v${ver}"
git tag "v${ver}"

echo
echo "Tagged v${ver}. Push to trigger the release build:"
echo "    git push && git push origin v${ver}"
