# Sankey — Local Install (for friends, no GitHub yet)

> This plugin is not published. Share it as a zip/folder.

## 1. Get the folder
From the author you receive a folder/zip named `sankey` containing `manifest.json`, `Panel.qml`, `daemon/`, `bin/` etc.
If it's a zip:
```bash
unzip sankey.zip
```

## 2. Install (Omarchy / Arch + Hyprland)

**Prereqs** — already on Omarchy: `hyprland`, `quickshell`, `alsa`, `libevdev`, `libx11`, `pkg-config`, `gtk4`, `xdg-desktop-portal`. Rust only needed if no verified prebuilt (auto-fallback).

```bash
# 1. Copy to Omarchy plugins (exact id matters)
mkdir -p ~/.config/omarchy/plugins
cp -r sankey ~/.config/omarchy/plugins/io.github.sanman.sankey

# 2. Run one-click installer (verified prebuilt if releases exist, else builds Rust daemon)
~/.config/omarchy/plugins/io.github.sanman.sankey/bin/sankey-setup
# first build ~40s (cached) or instant if prebuilt

# 3. Keyboard capture needs `input` group (installer will tell you)
sudo usermod -aG input $USER
# then log out and back in (or `newgrp input` to test)
```

Then enable the widget: **Omarchy menu → Plugins** (or `omarchy plugin` settings) → enable **Sankey** → it appears on the right side of the bar (keyboard icon). `sankey-setup` already enables it — no extra `pkill` needed (single rescan, no flash).

## 3. Use
- **Click** keyboard icon → panel with Mute, Volume slider, Soundpack dropdown
- **Right-click** icon → toggle mute
- **Scroll** on icon → volume
- **Hotkey** `Ctrl+Alt+M` → mute (persists)
- **Import pack…** → pick a `.zip` (must contain `config.json` V2). Installed to `~/.local/share/sankey/soundpacks/keyboard/{id}/`
- **Open folder** → opens `~/.local/share/sankey/soundpacks` in file manager

## 4. Check it's running
```bash
systemctl --user is-active sankey          # should be `active`
~/.local/bin/sankeyd ctl '{"cmd":"status"}'  # {"running":true,"muted":false,"volume":47,...}
journalctl --user -u sankey -n 30          # logs
```

## 5. Update (new zip from author)
```bash
rm -rf ~/.config/omarchy/plugins/io.github.sanman.sankey
cp -r sankey ~/.config/omarchy/plugins/io.github.sanman.sankey
~/.config/omarchy/plugins/io.github.sanman.sankey/bin/sankey-setup
# no pkill needed — enable triggers rescan
```

## 6. Uninstall
```bash
# Panel → Uninstall does: systemctl disable --now sankey + omarchy plugin remove --yes
# Or manually:
~/.config/omarchy/plugins/io.github.sanman.sankey/scripts/uninstall.sh        # keep packs .bak
~/.config/omarchy/plugins/io.github.sanman.sankey/scripts/uninstall.sh --purge  # delete all
omarchy plugin remove io.github.sanman.sankey --yes; omarchy restart shell
```

## 7. Troubleshooting
- **No sound:** `groups` must contain `input`; `systemctl --user status sankey`; check `~/.local/share/sankey/data/config.json` has `keyboard_soundpack`
- **No bar icon:** `omarchy plugin validate ~/.config/omarchy/plugins/io.github.sanman.sankey` should exit 0; `journalctl --user -u omarchy-shell -n 30` for QML errors; `pkill -x quickshell` again
- **Import fails:** ZIP must have `config.json` at top level; try `bin/sankey-import-pack.py your.zip` manually

## For the author
Local-only repo (`~/Work/sankey` has no `.git`). Zip to share:
```bash
cd ~/Work && zip -r sankey.zip sankey -x "sankey/daemon/target/*" -x "sankey/.git/*"
```
