# Sorakey Cleanup Checklist

Run this command to check if all Sorakey files are removed:

```bash
find ~/.config ~/.local ~/.cache -name "*sorakey*" -o -name "*Sorakey*" -o -name "*SORAKEY*" 2>/dev/null
```

## Files/Locations to Check

| Path | Description |
|------|-------------|
| `~/.config/omarchy/plugins/*sorakey*` | Plugin files |
| `~/.local/bin/sorakey` | Binary |
| `~/.local/share/sorakey/` | Soundpacks |
| `~/.local/lib/sorakey/` | Build cache |
| `~/.cache/sorakey/` | Temp cache |
| `~/.config/systemd/user/sorakey.service` | Systemd service |

## Services to Check

```bash
systemctl --user list-units --all | grep sorakey
systemctl --user list-unit-files | grep sorakey
```

If any output appears, run:
```bash
systemctl --user disable --now sorakey
systemctl --user daemon-reload
```
