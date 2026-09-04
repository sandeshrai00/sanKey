#!/usr/bin/env bash
set -euo pipefail
# dev-sync — copy dev repo to installed plugin for testing

DEV_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLUGIN_ID="io.github.sandeshrai00.sorakey"
INSTALLED="$HOME/.config/omarchy/plugins/$PLUGIN_ID"

NO_RESTART=0
NO_VALIDATE=0
for a in "$@"; do
  case "$a" in
    --no-restart) NO_RESTART=1 ;;
    --no-validate) NO_VALIDATE=1 ;;
    -h|--help) echo "Usage: $0 [--no-restart] [--no-validate]"; exit 0 ;;
    *) echo "unknown arg: $a" >&2; exit 1 ;;
  esac
done

[[ -f "$DEV_DIR/manifest.json" ]] || { echo "no manifest at $DEV_DIR" >&2; exit 1; }

mkdir -p "$INSTALLED"

# rsync if available, else cp
if command -v rsync >/dev/null 2>&1; then
  # process docs are --delete-excluded: an excluded file survives --delete as
  # a stale copy unless it's purged from the target too
  rsync -a --delete --delete-excluded \
    --exclude=".git" \
    --exclude="target" \
    --exclude="docs/dev" \
    --exclude="CLEANUP.md" \
    --exclude="admin-scripts" \
    --exclude=".github" \
    --exclude="__pycache__/" \
    --exclude="*.pyc" \
    "$DEV_DIR"/ "$INSTALLED"/
  # keep git worktree
  if [[ -d "$DEV_DIR/.git" ]]; then
    rsync -a --delete "$DEV_DIR/.git"/ "$INSTALLED/.git"/
  fi
else
  cp -a "$DEV_DIR"/. "$INSTALLED"/
  # same junk list as the rsync excludes above (cp has no --exclude)
  rm -rf "$INSTALLED/docs/dev" "$INSTALLED/CLEANUP.md" "$INSTALLED/admin-scripts" "$INSTALLED/.github"
  find "$INSTALLED" -name "__pycache__" -type d -prune -exec rm -rf {} + 2>/dev/null || true
  find "$INSTALLED" -name "*.pyc" -delete 2>/dev/null || true
fi

echo "synced $DEV_DIR -> $INSTALLED"

if [[ $NO_VALIDATE -eq 0 ]]; then
  if command -v omarchy >/dev/null 2>&1; then
    omarchy plugin validate "$INSTALLED" || { echo "validate failed" >&2; exit 1; }
  else
    echo "skip validate (omarchy not on PATH)"
  fi
fi

if [[ $NO_RESTART -eq 0 ]]; then
  if command -v omarchy >/dev/null 2>&1; then
    omarchy restart shell 2>/dev/null || omarchy restart --shell 2>/dev/null || echo "restart shell manually: omarchy restart shell"
    sleep 1
  fi
fi

# health check
systemctl --user is-active sorakey >/dev/null 2>&1 && echo "sorakey: active" || echo "sorakey: inactive (enable plugin or check journalctl --user -u sorakey)"
