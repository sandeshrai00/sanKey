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
  if curl --proto '=https' --tlsv1.2 -fsSL --max-filesize 33554432 -o "$tmp/sankeyd-x86_64" "$url" 2>/dev/null \
    && curl --proto '=https' --tlsv1.2 -fsSL --max-filesize 1048576 -o "$tmp/SHA256SUMS" "$sums" 2>/dev/null; then
    # SHA256SUMS may contain "dist/sankeyd-x86_64" path — normalize
    sed -i 's|dist/||g' "$tmp/SHA256SUMS" 2>/dev/null || true
    (cd "$tmp" && sha256sum -c SHA256SUMS 2>/dev/null) || { rm -rf "$tmp"; return 1; }
    # Hybrid: gh attestation if available (strict), else sha256sum-only with warning (Omarchy spec: safe install, validates listings not security)
    if command -v gh >/dev/null 2>&1; then
      if gh attestation verify "$tmp/sankeyd-x86_64" --repo sandeshrai00/sanKey --cert-identity-regex "https://github.com/sandeshrai00/sanKey/.github/workflows/release.*" --deny-self-hosted-runners 2>/dev/null; then
        install -m 755 "$tmp/sankeyd-x86_64" "$BIN"
        [[ -n "$source_id" ]] && echo "$source_id" > "$LIB_DIR/source.sha256"
        rm -rf "$tmp"
        echo "Installed verified prebuilt $version $arch (attested)"
        return 0
      else
        echo "warning: gh attestation failed — falling back to source build" >&2
        rm -rf "$tmp"
        return 1
      fi
    else
      echo "warning: gh missing, using sha256sum-only prebuilt" >&2
      install -m 755 "$tmp/sankeyd-x86_64" "$BIN"
      [[ -n "$source_id" ]] && echo "$source_id" > "$LIB_DIR/source.sha256"
      rm -rf "$tmp"
      echo "Installed verified prebuilt $version $arch (sha256sum)"
      return 0
    fi
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
export CARGO_TERM_QUIET=true
cargo build --locked --release --manifest-path "$DAEMON_DIR/Cargo.toml" --target-dir "$TARGET_DIR"
install -m 755 "$TARGET_DIR/release/sankeyd" "$BIN"
[[ -n "$source_id" ]] && echo "$source_id" > "$LIB_DIR/source.sha256"
echo "Built and installed $BIN"
