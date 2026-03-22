#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-$(awk '/^\[workspace.package\]/{flag=1; next} flag && /^version =/{gsub(/"/, "", $3); print $3; exit}' "$ROOT_DIR/Cargo.toml")}"
ARCH="${ARCH:-$(dpkg --print-architecture)}"
TAG="v${VERSION}"
TARBALL_FILE="$ROOT_DIR/dist/soundmind-linux-${VERSION}.tar.gz"
DEB_FILE="$ROOT_DIR/dist/soundmind_${VERSION}_${ARCH}.deb"
CHECKSUM_FILE="$ROOT_DIR/dist/soundmind_${VERSION}_SHA256SUMS"

cd "$ROOT_DIR"

if [[ -n "$(git status --short)" ]]; then
  echo "git worktree is not clean; commit or stash changes before releasing" >&2
  exit 1
fi

"$ROOT_DIR/scripts/package-release.sh" "$VERSION"

if ! git rev-parse -q --verify "refs/tags/${TAG}" >/dev/null; then
  git tag -a "$TAG" -m "Soundmind ${TAG}"
fi

git push origin HEAD
git push origin "$TAG"

gh release create "$TAG" \
  "$TARBALL_FILE" \
  "$DEB_FILE" \
  "$CHECKSUM_FILE" \
  --repo 3vilM33pl3/soundmind \
  --title "Soundmind ${TAG}" \
  --generate-notes
