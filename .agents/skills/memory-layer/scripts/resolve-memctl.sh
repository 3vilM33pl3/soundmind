#!/usr/bin/env bash
set -euo pipefail

resolve_memctl_cmd() {
  if [[ -n "${MEMCTL_BIN:-}" ]]; then
    read -r -a MEMCTL_CMD <<< "$MEMCTL_BIN"
    return 0
  fi

  resolve_memory_layer_config

  if command -v memctl >/dev/null 2>&1; then
    MEMCTL_CMD=(memctl)
    return 0
  fi

  local script_dir skill_dir repo_root sibling_memory_root
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  skill_dir="$(cd "$script_dir/.." && pwd)"
  repo_root="$(cd "$skill_dir/../../.." && pwd)"
  sibling_memory_root="${MEMORY_WORKSPACE:-$repo_root/../memory}"

  local built_binary
  for built_binary in \
    "$sibling_memory_root/target/debug/mem-cli" \
    "$sibling_memory_root/target/release/mem-cli" \
    "$sibling_memory_root/target/debian/memory-layer/usr/bin/mem-cli"
  do
    if [[ -x "$built_binary" ]]; then
      MEMCTL_CMD=("$built_binary")
      return 0
    fi
  done

  if [[ -f "$sibling_memory_root/Cargo.toml" && -d "$sibling_memory_root/crates/mem-cli" ]]; then
    MEMCTL_CMD=(cargo run --quiet --manifest-path "$sibling_memory_root/Cargo.toml" --bin mem-cli --)
    return 0
  fi

  echo "Unable to resolve mem-cli. Set MEMCTL_BIN, install 'memctl', or set MEMORY_WORKSPACE to the memory repo." >&2
  return 1
}

resolve_memory_layer_config() {
  if [[ -n "${MEMORY_LAYER_CONFIG:-}" ]]; then
    return 0
  fi

  if [[ -f "$HOME/.config/memory-layer/memory-layer.toml" ]]; then
    export MEMORY_LAYER_CONFIG="$HOME/.config/memory-layer/memory-layer.toml"
    return 0
  fi

  if command -v systemctl >/dev/null 2>&1 \
    && systemctl is-active --quiet memory-layer.service \
    && [[ -f /etc/memory-layer/memory-layer.toml ]]; then
    export MEMORY_LAYER_CONFIG=/etc/memory-layer/memory-layer.toml
    return 0
  fi

  if [[ -f /etc/memory-layer/memory-layer.toml ]]; then
    export MEMORY_LAYER_CONFIG=/etc/memory-layer/memory-layer.toml
  fi
}
