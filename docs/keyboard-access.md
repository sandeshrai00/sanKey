# Why Sorakey asks for keyboard access

Sorakey plays sounds as you type. To do that, it needs to hear your keys.

## Why approval is needed

Linux locks keyboard input so that only your login session may hear keys.
Sorakey runs in the background, so the system asks you — once — to grant
it access. This is the same kind of approval every app install asks for,
and it works with password or fingerprint.

## What tapping "Enable keyboard sounds" does

1. Your system's approval dialog appears (not ours — we never see
   what you type into it).
2. On approval, one access rule is installed for **keyboards only**:
   `/etc/udev/rules.d/70-sorakey-keyboard.rules`
3. Sounds start within seconds. No logout, no restart, no terminal.

## Privacy

Keys are heard in order to play sounds — that is all:

- never recorded,
- never logged (the sound engine is even tested to stay silent in code),
- never sent anywhere (everything stays on your machine).

## Removal

Removing the plugin removes the rule too (`uninstall.sh` handles it with
the same one-time approval). No residue, no silent leftovers.

## FAQ

**I tapped Cancel — now what?**
Nothing breaks. The panel keeps showing the honest state with a Retry
button. Approve whenever you're ready.

**Will I be asked again?**
No. The approval persists across reboots and updates.

**Does it slow down typing or drain battery?**
No. Key detection is event-driven (zero polling), and each keystroke
reuses precomputed audio — no work per key beyond playback.

**I use a different layout / multiple keyboards.**
Access covers all keyboards on the system, and key detection is
layout-independent.
