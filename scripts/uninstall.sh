#!/usr/bin/env bash
set -euo pipefail
PURGE=0
if [[ "${1:-}" == "--purge" ]]; then PURGE=1; fi
echo "== Sorakey uninstall =="
systemctl --user disable --now sorakey 2>/dev/null || true
systemctl --user daemon-reload 2>/dev/null || true
pkill -x sorakey 2>/dev/null || true
if [[ $PURGE -eq 1 ]]; then
  rm -rf ~/.local/share/sorakey ~/.local/bin/sorakey ~/.config/systemd/user/sorakey.service
  echo "purged data + binary"
else
  # keep packs by moving to .bak
  if [[ -d ~/.local/share/sorakey ]]; then
    mv ~/.local/share/sorakey ~/.local/share/sorakey.bak.$(date +%s) 2>/dev/null || true
    echo "moved packs to .bak (use --purge to delete)"
  fi
  rm -f ~/.local/bin/sorakey ~/.config/systemd/user/sorakey.service
  systemctl --user daemon-reload 2>/dev/null || true
fi
# plugin folder removal is done by: omarchy plugin remove io.github.sandeshrai00.sorakey --yes
echo "run: omarchy plugin remove io.github.sandeshrai00.sorakey --yes ; omarchy restart shell"
