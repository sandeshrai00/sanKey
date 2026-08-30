#!/bin/bash
set -euo pipefail
PLUGIN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DAEMON_DIR="$PLUGIN_DIR/daemon"
MANIFEST="$PLUGIN_DIR/manifest.json"
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/sankey"
TARGET_DIR="$CACHE_DIR/target"
LIB_DIR="$HOME/.local/lib/sankey"
BIN="$HOME/.local/bin/sankeyd"
REPO="sandeshrai00/sanKey"

mkdir -p "$CACHE_DIR" "$LIB_DIR"

version="$(python3 -c "import json;print(json.load(open('$MANIFEST'))['version'])" 2>/dev/null || echo "0.0.0")"
arch="$(uname -m)"
case "$arch" in x86_64|aarch64) ;; *) arch="x86_64";; esac
asset="sankeyd-${arch}"

# source id for staleness
source_id=""
if command -v sha256sum >/dev/null 2>&1; then
  source_id=$( { find "$DAEMON_DIR" -name "Cargo.toml" -o -name "Cargo.lock" -o -name "*.rs";
                 echo "$PLUGIN_DIR/rust-toolchain.toml"; } | sort | xargs cat 2>/dev/null | sha256sum | cut -d' ' -f1)
  if [[ -f "$LIB_DIR/source.sha256" ]] && [[ "$(cat "$LIB_DIR/source.sha256" 2>/dev/null)" == "$source_id" ]] && [[ -x "$BIN" ]]; then
    echo "sankeyd up to date (source $source_id)"
    exit 0
  fi
fi

# A release is only trusted for the exact tagged commit: if the daemon source
# moved past the tag, the prebuilt would be stale, so refuse it (same rule the
# Omarchy-Spotify plugin uses).
release_matches_source() {
  command -v git >/dev/null 2>&1 || return 1
  local dirty tag_commit
  dirty=$(git -C "$PLUGIN_DIR" status --porcelain --untracked-files=normal -- daemon rust-toolchain.toml manifest.json 2>/dev/null) || return 1
  [[ -z "$dirty" ]] || return 1
  tag_commit=$(git -C "$PLUGIN_DIR" rev-parse "refs/tags/v${version}^{commit}" 2>/dev/null) || return 1
  git -C "$PLUGIN_DIR" diff --quiet "$tag_commit" HEAD -- daemon rust-toolchain.toml manifest.json 2>/dev/null || return 1
}

# gh can verify attestations only when actually authenticated (git-over-SSH
# does not count).
gh_can_verify() {
  command -v gh >/dev/null 2>&1 || return 1
  [[ -n "${GH_TOKEN:-}" ]] && return 0
  GH_PROMPT_DISABLED=1 gh auth status --active >/dev/null 2>&1
}

try_download_prebuilt() {
  release_matches_source || return 1
  command -v curl >/dev/null 2>&1 || return 1
  command -v sha256sum >/dev/null 2>&1 || return 1
  local url="https://github.com/$REPO/releases/download/v${version}/${asset}"
  local sums="https://github.com/$REPO/releases/download/v${version}/SHA256SUMS"
  local tmp
  tmp=$(mktemp -d)
  echo "Trying verified prebuilt $url ..."
  if curl --proto '=https' --tlsv1.2 -fsSL --max-time 120 -o "$tmp/$asset" "$url" 2>/dev/null \
    && curl --proto '=https' --tlsv1.2 -fsSL --max-time 30 -o "$tmp/SHA256SUMS" "$sums" 2>/dev/null; then
    # SHA256SUMS may contain "dist/..." or bare asset names — normalize
    sed -i "s|dist/||g; s|\*${asset}|${asset}|g" "$tmp/SHA256SUMS" 2>/dev/null || true
    if (cd "$tmp" && sha256sum -c "${asset}" >/dev/null 2>&1 || (cd "$tmp" && sha256sum -c SHA256SUMS >/dev/null 2>&1)); then
      if gh_can_verify; then
        # Strongest path: prove GitHub CI built this file from the tagged commit.
        if GH_PROMPT_DISABLED=1 gh attestation verify "$tmp/$asset" --repo "$REPO" \
             --cert-identity-regex "https://github.com/$REPO/.github/workflows/release.*" \
             --deny-self-hosted-runners 2>/dev/null; then
          install -m 755 "$tmp/$asset" "$BIN"
          [[ -n "$source_id" ]] && echo "$source_id" > "$LIB_DIR/source.sha256"
          rm -rf "$tmp"
          echo "Installed verified prebuilt $version $arch (attested)"
          return 0
        fi
        # Attestation failed while it was possible: a real integrity signal,
        # so do NOT install the binary — fall back to the source build.
        echo "warning: attestation failed — building from source" >&2
        rm -rf "$tmp"
        return 1
      fi
      # No way to attestation-verify here (gh missing or not logged in):
      # the release checksum already passed, so install it. The checksum
      # comes from the same GitHub release, so this trusts the release page.
      install -m 755 "$tmp/$asset" "$BIN"
      [[ -n "$source_id" ]] && echo "$source_id" > "$LIB_DIR/source.sha256"
      rm -rf "$tmp"
      echo "Installed prebuilt $version $arch (release checksum verified; attestation skipped — gh not logged in, run 'gh auth login' for the attested path)"
      return 0
    fi
  fi
  rm -rf "$tmp" 2>/dev/null || true
  return 1
}

if [[ "${SANKEY_BUILD_FROM_SOURCE:-}" != "1" ]]; then
  if try_download_prebuilt; then exit 0; fi
  echo "No usable prebuilt for this source (no release yet, or source moved past the tag) — building from source"
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