#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./resolve-memctl.sh
source "$SCRIPT_DIR/resolve-memctl.sh"

PROJECT="${1:-${MEMORY_LAYER_PROJECT:-$(basename "$PWD")}}"
resolve_memctl_cmd

exec "${MEMCTL_CMD[@]}" curate --project "$PROJECT"
