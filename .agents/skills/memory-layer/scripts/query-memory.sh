#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./resolve-memctl.sh
source "$SCRIPT_DIR/resolve-memctl.sh"

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 \"<question>\" [project-slug]" >&2
  exit 2
fi

QUESTION="$1"
PROJECT="${2:-${MEMORY_LAYER_PROJECT:-$(basename "$PWD")}}"
resolve_memctl_cmd

exec "${MEMCTL_CMD[@]}" query --project "$PROJECT" --question "$QUESTION"
