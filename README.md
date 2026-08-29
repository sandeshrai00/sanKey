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

Local-only repo (no GitHub remote). Copy the folder to `~/.config/omarchy/plugins/io.github.sanman.sankey/` and enable it in Omarchy.

1. Open the Sankey panel on your bar and click **Install Sankey**. It builds
   `sankeyd` from `daemon/`, installs it, and starts the service. First build
   needs a Rust toolchain (the installer offers to set one up) and takes a few
   minutes.

3. For **keyboard** sounds on Wayland your user needs the `input` group. The
   installer tells you when it is missing:

   ```
   sudo usermod -aG input $USER
   ```

    then log out and back in.

## Files

| Path | What |
|---|---|
| `manifest.json` | Omarchy plugin manifest (bar-widget, panel, service) |
| `Panel.qml` | Bar icon + popup panel (with **Import pack…** button) |
| `Service.qml` | Hidden background service (owns the import flow) |
| `Model.js` | Status/pack parsing helpers |
| `bin/sankey-setup` | One-click installer |
| `bin/sankey-import-pack.py` | GTK4 file-picker + ZIP extractor (called by Service) |
| `daemon/` | The `sankeyd` Rust daemon (trimmed MechvibesDX core) |
| `daemon/soundpacks/` | Built-in V2 soundpacks |

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

Mute is also the `Ctrl+Alt+M` hotkey, and persists.

## Layout

```
~/.local/bin/sankeyd                 binary
~/.local/share/sankey/soundpacks/    built-in packs
~/.local/share/sankey/data/config.json   settings (persisted by ctl)
$XDG_RUNTIME_DIR/sankey.sock         control socket
```

## Build the daemon by hand

```
cargo build --release --manifest-path daemon/Cargo.toml
```

Requires `rustc` plus the native libs `alsa`, `libevdev`, `libx11`,
`pkg-config` (already present on an Omarchy box).

## License

MIT. The daemon is a fork of the MIT-licensed MechvibesDX core
(Copyright (c) 2026 Hải Nguyễn); see `LICENSE`.