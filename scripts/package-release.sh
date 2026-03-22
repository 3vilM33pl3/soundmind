#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-$(awk '/^\[workspace.package\]/{flag=1; next} flag && /^version =/{gsub(/"/, "", $3); print $3; exit}' "$ROOT_DIR/Cargo.toml")}"
ARCH="${ARCH:-$(dpkg --print-architecture)}"
DIST_ROOT="$ROOT_DIR/dist"
TARBALL_DIR="$DIST_ROOT/soundmind-linux-${VERSION}"
DEB_STAGING_DIR="$DIST_ROOT/soundmind_${VERSION}_${ARCH}"
DEB_FILE="$DIST_ROOT/soundmind_${VERSION}_${ARCH}.deb"
TARBALL_FILE="$DIST_ROOT/soundmind-linux-${VERSION}.tar.gz"
CHECKSUM_FILE="$DIST_ROOT/soundmind_${VERSION}_SHA256SUMS"

rm -rf "$TARBALL_DIR" "$DEB_STAGING_DIR" "$DEB_FILE" "$TARBALL_FILE" "$CHECKSUM_FILE"
mkdir -p \
  "$TARBALL_DIR/bin" \
  "$TARBALL_DIR/config" \
  "$TARBALL_DIR/packaging/bin" \
  "$TARBALL_DIR/packaging/linux" \
  "$TARBALL_DIR/packaging/systemd" \
  "$DEB_STAGING_DIR/DEBIAN" \
  "$DEB_STAGING_DIR/usr/bin" \
  "$DEB_STAGING_DIR/usr/lib/soundmind" \
  "$DEB_STAGING_DIR/usr/lib/systemd/user" \
  "$DEB_STAGING_DIR/usr/share/applications" \
  "$DEB_STAGING_DIR/usr/share/doc/soundmind" \
  "$DEB_STAGING_DIR/usr/share/icons/hicolor/128x128/apps" \
  "$DEB_STAGING_DIR/usr/share/soundmind"

cd "$ROOT_DIR"
cargo build --release -p app_backend -p app_ui

install -m 0755 target/release/app_backend "$TARBALL_DIR/bin/app_backend"
install -m 0755 target/release/app_ui "$TARBALL_DIR/bin/app_ui"
install -m 0644 config.example.toml "$TARBALL_DIR/config/config.toml"
install -m 0755 packaging/bin/soundmind "$TARBALL_DIR/packaging/bin/soundmind"
install -m 0755 packaging/bin/soundmind-backend "$TARBALL_DIR/packaging/bin/soundmind-backend"
install -m 0755 packaging/bin/soundmind-setup-user "$TARBALL_DIR/packaging/bin/soundmind-setup-user"
install -m 0644 packaging/linux/soundmind.desktop.in "$TARBALL_DIR/packaging/linux/soundmind.desktop.in"
install -m 0644 packaging/systemd/soundmind-backend.service "$TARBALL_DIR/packaging/systemd/soundmind-backend.service"
install -m 0644 crates/app_ui/icons/icon.png "$TARBALL_DIR/packaging/linux/soundmind.png"
install -m 0755 scripts/install-user-service.sh "$TARBALL_DIR/install-user-service.sh"

tar -C "$DIST_ROOT" -czf "$TARBALL_FILE" "soundmind-linux-${VERSION}"

install -m 0755 target/release/app_backend "$DEB_STAGING_DIR/usr/lib/soundmind/app_backend"
install -m 0755 target/release/app_ui "$DEB_STAGING_DIR/usr/lib/soundmind/app_ui"
install -m 0755 packaging/bin/soundmind "$DEB_STAGING_DIR/usr/bin/soundmind"
install -m 0755 packaging/bin/soundmind-backend "$DEB_STAGING_DIR/usr/bin/soundmind-backend"
install -m 0755 packaging/bin/soundmind-setup-user "$DEB_STAGING_DIR/usr/bin/soundmind-setup-user"
install -m 0644 config.example.toml "$DEB_STAGING_DIR/usr/share/soundmind/config.example.toml"
install -m 0644 README.md "$DEB_STAGING_DIR/usr/share/doc/soundmind/README.md"
install -m 0644 docs/INSTALLATION.md "$DEB_STAGING_DIR/usr/share/doc/soundmind/INSTALLATION.md"
install -m 0644 docs/USER_GUIDE.md "$DEB_STAGING_DIR/usr/share/doc/soundmind/USER_GUIDE.md"
install -m 0644 crates/app_ui/icons/icon.png "$DEB_STAGING_DIR/usr/share/icons/hicolor/128x128/apps/soundmind.png"

sed \
  -e 's|@APP_BIN@|soundmind|g' \
  packaging/linux/soundmind.desktop.in \
  > "$DEB_STAGING_DIR/usr/share/applications/soundmind.desktop"

sed \
  -e 's|@BACKEND_BIN@|/usr/bin/soundmind-backend|g' \
  -e 's|@CONFIG_DIR@|%h/.config/soundmind|g' \
  -e 's|@WORKING_DIR@|/usr/lib/soundmind|g' \
  packaging/systemd/soundmind-backend.service \
  > "$DEB_STAGING_DIR/usr/lib/systemd/user/soundmind-backend.service"

cat > "$DEB_STAGING_DIR/DEBIAN/control" <<EOF
Package: soundmind
Version: ${VERSION}
Section: sound
Priority: optional
Architecture: ${ARCH}
Maintainer: Soundmind Maintainers <opensource@users.noreply.github.com>
Depends: bash, libgtk-3-0, libwebkit2gtk-4.1-0, pulseaudio-utils, systemd
Recommends: libayatana-appindicator3-1, poppler-utils
Homepage: https://github.com/3vilM33pl3/soundmind
Description: Ubuntu system-audio interview assistant
 Soundmind captures desktop audio, streams it for transcription, and gives
 live interview assistance with answers, summaries, commentary, session
 history, and priming documents.
EOF

dpkg-deb --build --root-owner-group "$DEB_STAGING_DIR" "$DEB_FILE"

(cd "$DIST_ROOT" && sha256sum "$(basename "$TARBALL_FILE")" "$(basename "$DEB_FILE")") > "$CHECKSUM_FILE"

cat <<EOF
Created release artifacts:
  ${TARBALL_FILE}
  ${DEB_FILE}
  ${CHECKSUM_FILE}
EOF
