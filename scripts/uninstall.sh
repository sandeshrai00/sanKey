#!/bin/bash
set -euo pipefail
PURGE=0
if [[ "${1:-}" == "--purge" ]]; then PURGE=1; fi
UDEV_RULE="/etc/udev/rules.d/70-sorakey-keyboard.rules"
echo "== Sorakey uninstall =="
systemctl --user disable --now sorakey 2>/dev/null || true
systemctl --user daemon-reload 2>/dev/null || true
pkill -x sorakey 2>/dev/null || true
# remove the keyboard-access rule installed via the panel (GUI-approved);
# needs one approval, like the install did
if [[ -f "$UDEV_RULE" ]]; then
  if command -v pkexec >/dev/null 2>&1; then
    pkexec bash -c "rm -f '$UDEV_RULE' && udevadm control --reload-rules && udevadm trigger --subsystem-match=input --action=change" 2>/dev/null \
      && echo "removed keyboard-access rule" \
      || echo "kept $UDEV_RULE (approval declined) — remove with: pkexec rm $UDEV_RULE"
  else
    echo "kept $UDEV_RULE (no pkexec) — remove with: sudo rm $UDEV_RULE"
  fi
fi
if [[ $PURGE -eq 1 ]]; then
  rm -rf ~/.local/share/sorakey ~/.local/bin/sorakey ~/.config/systemd/user/sorakey.service ~/.cache/sorakey ~/.local/lib/sorakey
  echo "purged data + binary"
else
  # keep packs as .bak
  if [[ -d ~/.local/share/sorakey ]]; then
    mv ~/.local/share/sorakey ~/.local/share/sorakey.bak.$(date +%s) 2>/dev/null || true
    echo "moved packs to .bak (use --purge to delete)"
  fi
  rm -f ~/.local/bin/sorakey ~/.config/systemd/user/sorakey.service
  systemctl --user daemon-reload 2>/dev/null || true
fi
# remove plugin with: omarchy plugin remove io.github.sandeshrai00.sorakey --yes
echo "run: omarchy plugin remove io.github.sandeshrai00.sorakey --yes ; omarchy restart shell"
