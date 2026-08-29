#!/usr/bin/env bash
set -euo pipefail
PURGE=0
if [[ "${1:-}" == "--purge" ]]; then PURGE=1; fi
echo "== Sankey uninstall =="
systemctl --user disable --now sankey 2>/dev/null || true
systemctl --user daemon-reload 2>/dev/null || true
pkill -x sankeyd 2>/dev/null || true
if [[ $PURGE -eq 1 ]]; then
  rm -rf ~/.local/share/sankey ~/.local/bin/sankeyd ~/.config/systemd/user/sankey.service
  echo "purged data + binary"
else
  # keep packs by moving to .bak
  if [[ -d ~/.local/share/sankey ]]; then
    mv ~/.local/share/sankey ~/.local/share/sankey.bak.$(date +%s) 2>/dev/null || true
    echo "moved packs to .bak (use --purge to delete)"
  fi
  rm -f ~/.local/bin/sankeyd ~/.config/systemd/user/sankey.service
  systemctl --user daemon-reload 2>/dev/null || true
fi
# plugin folder removal is done by: omarchy plugin remove io.github.sanman.sankey --yes
echo "run: omarchy plugin remove io.github.sanman.sankey --yes ; omarchy restart shell"
