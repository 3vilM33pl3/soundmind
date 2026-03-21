#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALL_PREFIX="${INSTALL_PREFIX:-$HOME/.local}"
CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"

BIN_DIR="$INSTALL_PREFIX/bin"
LIB_DIR="$INSTALL_PREFIX/lib/soundmind"
CONFIG_DIR="$CONFIG_HOME/soundmind"
SYSTEMD_DIR="$CONFIG_HOME/systemd/user"
APPLICATIONS_DIR="$DATA_HOME/applications"
ICON_DIR="$DATA_HOME/icons/hicolor/128x128/apps"

mkdir -p "$BIN_DIR" "$LIB_DIR" "$CONFIG_DIR" "$SYSTEMD_DIR" "$APPLICATIONS_DIR" "$ICON_DIR"

cd "$ROOT_DIR"
cargo build --release -p app_backend -p app_ui

install -m 0755 target/release/app_backend "$LIB_DIR/app_backend"
install -m 0755 target/release/app_ui "$LIB_DIR/app_ui"
install -m 0644 crates/app_ui/icons/icon.png "$ICON_DIR/soundmind.png"

cat > "$BIN_DIR/soundmind" <<EOF
#!/usr/bin/env bash
set -euo pipefail
export SOUNDMIND_CONFIG="${CONFIG_DIR}/config.toml"
export SOUNDMIND_KEYS_ENV="${CONFIG_DIR}/keys.env"
exec "${LIB_DIR}/app_ui" "\$@"
EOF

cat > "$BIN_DIR/soundmind-backend" <<EOF
#!/usr/bin/env bash
set -euo pipefail
export SOUNDMIND_CONFIG="${CONFIG_DIR}/config.toml"
export SOUNDMIND_KEYS_ENV="${CONFIG_DIR}/keys.env"
cd "${LIB_DIR}"
exec "${LIB_DIR}/app_backend" "\$@"
EOF

chmod 0755 "$BIN_DIR/soundmind" "$BIN_DIR/soundmind-backend"

if [[ -f "$ROOT_DIR/config.toml" ]]; then
  cp "$ROOT_DIR/config.toml" "$CONFIG_DIR/config.toml"
elif [[ ! -f "$CONFIG_DIR/config.toml" ]]; then
  cp "$ROOT_DIR/config.example.toml" "$CONFIG_DIR/config.toml"
fi

if [[ -f "$ROOT_DIR/keys.env" && ! -f "$CONFIG_DIR/keys.env" ]]; then
  cp "$ROOT_DIR/keys.env" "$CONFIG_DIR/keys.env"
fi

sed \
  -e "s|@APP_BIN@|${BIN_DIR}/soundmind|g" \
  "$ROOT_DIR/packaging/linux/soundmind.desktop.in" \
  > "$APPLICATIONS_DIR/soundmind.desktop"

sed \
  -e "s|@BACKEND_BIN@|${BIN_DIR}/soundmind-backend|g" \
  -e "s|@CONFIG_DIR@|${CONFIG_DIR}|g" \
  -e "s|@WORKING_DIR@|${LIB_DIR}|g" \
  "$ROOT_DIR/packaging/systemd/soundmind-backend.service" \
  > "$SYSTEMD_DIR/soundmind-backend.service"

systemctl --user daemon-reload
systemctl --user enable --now soundmind-backend.service

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPLICATIONS_DIR" >/dev/null 2>&1 || true
fi

cat <<EOF
Installed Soundmind to:
  UI wrapper:        ${BIN_DIR}/soundmind
  Backend wrapper:   ${BIN_DIR}/soundmind-backend
  Config directory:  ${CONFIG_DIR}
  User service:      ${SYSTEMD_DIR}/soundmind-backend.service

The backend service is enabled and started for the current user.
If you copied a fresh config, review ${CONFIG_DIR}/config.toml and ${CONFIG_DIR}/keys.env.
EOF
