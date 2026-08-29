#!/usr/bin/env bash
set -euo pipefail
PLUGIN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DAEMON_DIR="$PLUGIN_DIR/daemon"
MANIFEST="$PLUGIN_DIR/manifest.json"
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/sankey"
TARGET_DIR="$CACHE_DIR/target"
LIB_DIR="$HOME/.local/lib/sankey"
BIN="$HOME/.local/bin/sankeyd"

mkdir -p "$CACHE_DIR" "$LIB_DIR"

version="$(python3 -c "import json;print(json.load(open('$MANIFEST'))['version'])" 2>/dev/null || echo "0.0.0")"
arch="$(uname -m)"
case "$arch" in x86_64|aarch64) ;; *) arch="x86_64";; esac

# source id for staleness
source_id=""
if command -v sha256sum >/dev/null 2>&1; then
  source_id=$(find "$DAEMON_DIR" -name "Cargo.toml" -o -name "Cargo.lock" -o -name "*.rs" | sort | xargs cat 2>/dev/null | sha256sum | cut -d' ' -f1)
  if [[ -f "$LIB_DIR/source.sha256" ]] && [[ "$(cat "$LIB_DIR/source.sha256" 2>/dev/null)" == "$source_id" ]] && [[ -x "$BIN" ]]; then
    echo "sankeyd up to date (source $source_id)"
    exit 0
  fi
fi

# Try verified prebuilt (when releases exist)
try_download_verified() {
  local url="https://github.com/sandeshrai00/sanKey/releases/download/v${version}/sankeyd-${arch}"
  local sums="https://github.com/sandeshrai00/sanKey/releases/download/v${version}/SHA256SUMS"
  command -v curl >/dev/null 2>&1 || return 1
  command -v sha256sum >/dev/null 2>&1 || return 1
  local tmp=$(mktemp -d)
  echo "Trying verified prebuilt $url ..."
  if curl --proto '=https' --tlsv1.2 -fsSL --max-filesize 33554432 -o "$tmp/sankeyd" "$url" 2>/dev/null \
    && curl --proto '=https' --tlsv1.2 -fsSL --max-filesize 1048576 -o "$tmp/SHA256SUMS" "$sums" 2>/dev/null; then
    (cd "$tmp" && sha256sum -c --ignore-missing SHA256SUMS 2>/dev/null) || { rm -rf "$tmp"; return 1; }
    if command -v gh >/dev/null 2>&1 && gh attestation verify "$tmp/sankeyd" --repo sandeshrai00/sanKey --cert-identity-regex "https://github.com/sandeshrai00/sanKey/.github/workflows/release.*" --deny-self-hosted-runners 2>/dev/null; then
      install -m 755 "$tmp/sankeyd" "$BIN"
      [[ -n "$source_id" ]] && echo "$source_id" > "$LIB_DIR/source.sha256"
      rm -rf "$tmp"
      echo "Installed verified prebuilt $version $arch"
      return 0
    fi
    rm -rf "$tmp"
  else
    rm -rf "$tmp" 2>/dev/null || true
  fi
  return 1
}

if [[ "${SANKEY_BUILD_FROM_SOURCE:-}" != "1" ]]; then
  if try_download_verified; then exit 0; fi
  echo "No attested prebuilt (or gh missing) — building from source"
fi

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found — install rustup (pacman -S rustup or https://rustup.rs) then re-run" >&2
  exit 1
fi

# Deterministic source build outside plugin dir (avoids watcher thrash)
export SOURCE_DATE_EPOCH=$(git -C "$PLUGIN_DIR" log -1 --format=%ct 2>/dev/null || date +%s)
export CARGO_INCREMENTAL=0
export CARGO_TERM_QUIET=1
cargo build --locked --release --manifest-path "$DAEMON_DIR/Cargo.toml" --target-dir "$TARGET_DIR"
install -m 755 "$TARGET_DIR/release/sankeyd" "$BIN"
[[ -n "$source_id" ]] && echo "$source_id" > "$LIB_DIR/source.sha256"
echo "Built and installed $BIN"
