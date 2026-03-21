#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-$(awk '/^\[workspace.package\]/{flag=1; next} flag && /^version =/{gsub(/"/, "", $3); print $3; exit}' "$ROOT_DIR/Cargo.toml")}"
DIST_DIR="$ROOT_DIR/dist/soundmind-linux-${VERSION}"

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/bin" "$DIST_DIR/config" "$DIST_DIR/packaging/linux" "$DIST_DIR/packaging/systemd"

cd "$ROOT_DIR"
cargo build --release -p app_backend -p app_ui

install -m 0755 target/release/app_backend "$DIST_DIR/bin/app_backend"
install -m 0755 target/release/app_ui "$DIST_DIR/bin/app_ui"
install -m 0644 config.example.toml "$DIST_DIR/config/config.toml"
install -m 0644 packaging/linux/soundmind.desktop.in "$DIST_DIR/packaging/linux/soundmind.desktop.in"
install -m 0644 packaging/systemd/soundmind-backend.service "$DIST_DIR/packaging/systemd/soundmind-backend.service"
install -m 0644 crates/app_ui/icons/icon.png "$DIST_DIR/packaging/linux/soundmind.png"
install -m 0755 scripts/install-user-service.sh "$DIST_DIR/install-user-service.sh"

tar -C "$ROOT_DIR/dist" -czf "$ROOT_DIR/dist/soundmind-linux-${VERSION}.tar.gz" "soundmind-linux-${VERSION}"

cat <<EOF
Created release bundle:
  ${DIST_DIR}
  ${ROOT_DIR}/dist/soundmind-linux-${VERSION}.tar.gz
EOF
