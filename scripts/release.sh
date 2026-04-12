#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

usage() {
  cat <<'EOF'
Usage: scripts/release.sh <version>

Example:
  scripts/release.sh 1.0.1

This script will:
  1. update Cargo.toml to the requested version
  2. refresh Cargo.lock
  3. run cargo test
  4. run cargo clippy --all-targets -- -D warnings
  5. commit the version bump
  6. create tag v<version>
  7. push the commit and tag to origin
EOF
}

if [[ $# -ne 1 ]]; then
  usage
  exit 1
fi

VERSION="$1"
TAG="v$VERSION"

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Error: version must look like 1.2.3"
  exit 1
fi

if [[ -n "$(git status --short)" ]]; then
  echo "Error: working tree is not clean. Commit or stash your changes first."
  exit 1
fi

CURRENT_VERSION="$(grep '^version = ' Cargo.toml | head -n1 | sed -E 's/version = "(.*)"/\1/')"

if [[ "$CURRENT_VERSION" == "$VERSION" ]]; then
  echo "Error: Cargo.toml is already at version $VERSION"
  exit 1
fi

if git rev-parse "$TAG" >/dev/null 2>&1; then
  echo "Error: git tag $TAG already exists locally"
  exit 1
fi

if git ls-remote --tags origin "refs/tags/$TAG" | grep -q "$TAG"; then
  echo "Error: git tag $TAG already exists on origin"
  exit 1
fi

python3 - <<'PY' "$VERSION"
from pathlib import Path
import re
import sys

version = sys.argv[1]
path = Path("Cargo.toml")
text = path.read_text()
updated, count = re.subn(
    r'(?m)^version = ".*"$',
    f'version = "{version}"',
    text,
    count=1,
)
if count != 1:
    raise SystemExit("Failed to update version in Cargo.toml")
path.write_text(updated)
PY

echo "Updated Cargo.toml: $CURRENT_VERSION -> $VERSION"

cargo check
cargo update --workspace
cargo test
cargo clippy --all-targets -- -D warnings

git add Cargo.toml Cargo.lock
git commit -m "Bump version to $VERSION"
git tag "$TAG"

git push origin main
git push origin "$TAG"

echo
echo "Release prepared and pushed:"
echo "  version: $VERSION"
echo "  tag:     $TAG"
echo
echo "GitHub Actions will now publish the crate and create the release."
