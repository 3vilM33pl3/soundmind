#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./resolve-memctl.sh
source "$SCRIPT_DIR/resolve-memctl.sh"
resolve_memctl_cmd

exec "${MEMCTL_CMD[@]}" remember "$@"
