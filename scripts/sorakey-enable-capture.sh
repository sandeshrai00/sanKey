#!/bin/bash
# Sorakey keyboard-access enabler — run from the panel's "Enable keyboard
# sounds" button. One GUI approval (via the shell's polkit agent), no
# terminal commands, no logout.
#
# Installs udev/70-sorakey-keyboard.rules to /etc/udev/rules.d/ (TAG+=uaccess
# for ID_INPUT_KEYBOARD devices), reloads rules and triggers them, then
# verifies the current user can read a keyboard event node.
#
# Exit codes: 0 = access works, 1 = hard error, 2 = approval not granted
# (panel stays truthful and offers Retry), 3 = no approval dialog exists
# on this box (panel offers the terminal route instead).
#
# Flags: --use-sudo  skip pkexec and use sudo directly (for terminal use,
# where a TTY exists for the password prompt).
set -u

USE_SUDO=0
if [[ "${1:-}" == "--use-sudo" ]]; then USE_SUDO=1; fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PLUGIN_DIR="$(dirname "$SCRIPT_DIR")"
SRC="$PLUGIN_DIR/udev/70-sorakey-keyboard.rules"
DST="/etc/udev/rules.d/70-sorakey-keyboard.rules"

step() { printf '\n\033[1m== %s ==\033[0m\n' "$*"; }
note() { printf '%s\n' "$*"; }

[[ -f "$SRC" ]] || { echo "sorakey-enable-capture: rule source missing: $SRC" >&2; exit 1; }

# A keyboard event node the current user should be able to read.
find_keyboard_node() {
  local d
  for d in /dev/input/event*; do
    [[ -e "$d" ]] || continue
    if udevadm info --query=property --name="$d" 2>/dev/null | grep -qx "ID_INPUT_KEYBOARD=1"; then
      printf '%s' "$d"
      return 0
    fi
  done
  return 1
}

user_can_read_keyboard() {
  local node
  node=$(find_keyboard_node) || return 1
  [[ -r "$node" ]]
}

run_privileged() {
  # $1 = shell snippet needing root. pkexec pops the shell's GUI dialog;
  # --use-sudo takes sudo (terminal provides the password prompt).
  # Prints the helper's stderr to $PK_ERR_FILE when set, for exit mapping.
  local snippet="$1" out=""
  if [[ "$USE_SUDO" -eq 0 ]] && command -v pkexec >/dev/null 2>&1; then
    out=$(pkexec bash -c "$snippet" 2>&1) && return 0
    printf '%s' "$out" > "${PK_ERR_FILE:-/dev/null}" 2>/dev/null || true
    note "(approval not granted — staying on the safe side)"
  elif command -v sudo >/dev/null 2>&1; then
    if sudo bash -c "$snippet"; then return 0; fi
  fi
  return 1
}

# Map a failed privilege attempt to an exit code from the helper's stderr:
# user dismissal -> 2, missing dialog/session plumbing -> 3, else 1.
map_priv_error() {
  local err="$1" low=""
  low=$(printf '%s' "$err" | tr '[:upper:]' '[:lower:]')
  case "$low" in
    *dismiss*|*cancel*) return 2 ;;
    *agent*|*authority*|*session*|*polkit*|*display*|*terminal*required*) return 3 ;;
    *) return 1 ;;
  esac
}

step "Checking keyboard access"
if user_can_read_keyboard; then
  note "keyboard access: OK ($(find_keyboard_node) readable)"
  if [[ -f "$DST" ]] && ! cmp -s "$SRC" "$DST"; then
    note "installed rule is outdated — refreshing on next approval"
  fi
  exit 0
fi
note "keyboard access: BLOCKED (no readable keyboard node)"

step "Enabling keyboard access (one approval)"
PK_ERR_FILE=$(mktemp)
if ! run_privileged "install -m 644 '$SRC' '$DST' && udevadm control --reload-rules && udevadm trigger --subsystem-match=input --action=change"; then
  err=$(cat "$PK_ERR_FILE" 2>/dev/null)
  rm -f "$PK_ERR_FILE"
  code=2
  if [[ -n "$err" ]]; then
    map_priv_error "$err"
    code=$?
  fi
  if [[ "$code" -eq 3 ]]; then
    echo "sorakey-enable-capture: no approval dialog on this system" >&2
    exit 3
  fi
  if [[ "$code" -eq 1 ]]; then
    printf '%s\n' "$err" >&2
    exit 1
  fi
  exit 2
fi
rm -f "$PK_ERR_FILE"

# udev applies ACLs asynchronously — poll briefly as the user.
step "Verifying access"
for _ in $(seq 1 10); do
  if user_can_read_keyboard; then
    note "keyboard access: OK ($(find_keyboard_node) readable, no logout needed)"
    exit 0
  fi
  sleep 1
done

echo "sorakey-enable-capture: rule installed but nodes still unreadable" >&2
exit 1
