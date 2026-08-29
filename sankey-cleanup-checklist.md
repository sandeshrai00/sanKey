# Sankey Cleanup Checklist

Run this command to check if all Sankey files are removed:

```bash
find ~/.config ~/.local ~/.cache -name "*sankey*" -o -name "*Sankey*" -o -name "*SANKEY*" 2>/dev/null
```

## Files/Locations to Check

| Path | Description |
|------|-------------|
| `~/.config/omarchy/plugins/*sankey*` | Plugin files |
| `~/.local/bin/sankeyd` | Binary |
| `~/.local/share/sankey/` | Soundpacks |
| `~/.local/lib/sankey/` | Build cache |
| `~/.cache/sankey/` | Temp cache |
| `~/.config/systemd/user/sankey.service` | Systemd service |

## Services to Check

```bash
systemctl --user list-units --all | grep sankey
systemctl --user list-unit-files | grep sankey
```

If any output appears, run:
```bash
systemctl --user disable --now sankey
systemctl --user daemon-reload
```
