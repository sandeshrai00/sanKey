# Sankey

Mechanical keyboard sounds for Omarchy, driven by a lean Rust
daemon. A keyboard icon on the bar opens a panel with live mute, volume,
and soundpack picker; right-click the icon to mute, scroll to
set volume.

## What it is

- **`sankeyd`** — a headless sound daemon forked from the [MechvibesDX](https://github.com/hainguyents13/mechvibes-dx)
  v0.8.2 audio core: same polyphonic engine, anti-click fades, resampler and
  V2 soundpack format, with all of the GUI, tray, telemetry and auto-updater
  removed. It runs as a `systemd` user service, idles at ~0% CPU and ~40 MB RAM,
  and is controlled over a Unix socket.
- **The Omarchy plugin** — a bar widget + panel that installs the daemon
  (one click) and controls it live.

## Install

```sh
omarchy plugin add https://github.com/sandeshrai00/sanKey.git --enable
```

1. Open the Sankey panel on your bar and click **Install Sankey**. It installs
   `sankeyd` (verified prebuilt from GitHub Releases when available, else built
   from `daemon/` source) and starts the service. A source build needs a Rust
   toolchain and takes a few minutes.
2. For **keyboard** sounds on Wayland your user needs the `input` group. The
   installer tells you when it is missing:

   ```
   sudo usermod -aG input $USER
   ```

   then log out and back in.

## Usage

- **Left-click** the keyboard icon → panel: mute switch, volume slider,
  soundpack dropdown, **Random**, **Import pack…**, **Open folder**,
  Start/Stop/Restart, and Settings (bar icon position).
- **Right-click** → toggle mute. **Scroll** on the icon → volume.
- **Escape** closes the panel, **Tab** / **Shift+Tab** switches panels.

## Configure

```sh
omarchy bar move io.github.sandeshrai00.sankey --section center
```

Mute, volume and soundpack persist across restarts
(`~/.local/share/sankey/data/config.json`); the icon section persists across
disable/re-enable.

## Update

```sh
omarchy plugin update io.github.sandeshrai00.sankey --yes
```

Updates the plugin code; the daemon binary is re-verified/rebuilt the next
time **Install Sankey** runs (or `scripts/build-sankeyd.sh` by hand).

## Remove

```sh
# stops the daemon, removes binary + service file, keeps soundpacks as .bak
~/.config/omarchy/plugins/io.github.sandeshrai00.sankey/scripts/uninstall.sh
omarchy plugin remove io.github.sandeshrai00.sankey --yes
```

`uninstall.sh --purge` deletes the soundpacks too.

## Import a soundpack

Click the keyboard icon on the bar → **Import pack…** → pick a `.zip`.
The pack is extracted to `~/.local/share/sankey/soundpacks/keyboard/{id}/`
and appears in the keyboard dropdown on the next refresh. The ZIP must
contain a `config.json` (V2 format).

## Control API

`sankeyd ctl '<json>'` speaks one JSON line in, one JSON line out, over
`$XDG_RUNTIME_DIR/sankey.sock`:

- `status` — running, muted, volume, active pack
- `mute {"muted": true|false}`
- `volume {"value": 0-100}`
- `keyboard_pack {"id": "keyboard/..."}`
- `packs` — list available packs

## Files

| Path | What |
|---|---|
| `manifest.json` | Omarchy plugin manifest (bar-widget + service) |
| `Panel.qml` | Bar icon + popup panel (with **Import pack…** button) |
| `Service.qml` | Headless service (daemon lifecycle + import flow) |
| `Model.js` | Status/pack parsing helpers |
| `scripts/sankey-setup` | One-click installer |
| `scripts/sankey-import-pack.py` | GTK4 file-picker + ZIP extractor |
| `scripts/build-sankeyd.sh` | Verified-prebuilt-or-source daemon build |
| `scripts/uninstall.sh` | Removes daemon, unit file, binary (`--purge`: packs too) |
| `daemon/` | The `sankeyd` Rust daemon (trimmed MechvibesDX core) |
| `daemon/soundpacks/` | Built-in V2 soundpacks |

## Layout

```
~/.local/bin/sankeyd                 binary
~/.local/share/sankey/soundpacks/    built-in + imported packs
~/.local/share/sankey/data/config.json   settings (persisted by ctl)
~/.local/share/sankey/bar-section    last bar section chosen in Settings
$XDG_RUNTIME_DIR/sankey.sock         control socket
```

## Build the daemon by hand

```sh
cargo build --release --manifest-path daemon/Cargo.toml
```

Requires `rustc` plus the native libs `alsa`, `libevdev`, `libx11`,
`pkg-config` (already present on an Omarchy box).

## License

MIT. The daemon is a fork of the MIT-licensed MechvibesDX core
(Copyright (c) 2026 Hải Nguyễn); see `LICENSE`.