# Sorakey

Mechanical keyboard sounds for Omarchy, driven by a lean Rust
daemon. A keyboard icon on the bar opens a panel with live mute, volume,
and soundpack picker; right-click the icon to mute, scroll to
set volume.

## What it is

- **`sorakey`** — a headless sound daemon forked from the [MechvibesDX](https://github.com/hainguyents13/mechvibes-dx)
  v0.8.2 audio core: same polyphonic engine, anti-click fades, resampler and
  V2 soundpack format, with all of the GUI, tray, telemetry and auto-updater
  removed. It runs as a `systemd` user service, idles at ~0% CPU and ~40 MB RAM,
  and is controlled over a Unix socket.
- **The Omarchy plugin** — a bar widget + panel that installs the daemon
  (one click) and controls it live.

## Install

```sh
omarchy plugin add https://github.com/sandeshrai00/sorakey.git --enable
```

1. Open the Sorakey panel on your bar and click **Install Sorakey**. It installs
   `sorakey` and starts the service. The installer prefers the **prebuilt
   binary from GitHub Releases** (a few seconds): it verifies the release
   checksum, and when `gh` is logged in (or `GH_TOKEN` is set) also the GitHub
   attestation proving CI built it from the tagged commit. Without a `gh`
   login the checksum check alone is accepted. It only builds from source
   (needs Rust, a few minutes) when no release matches the current daemon
   source — e.g. you changed `daemon/` but haven't tagged a new version yet.
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
omarchy bar move io.github.sandeshrai00.sorakey --section center
```

Mute, volume and soundpack persist across restarts
(`~/.local/share/sorakey/data/config.json`); the icon section persists across
disable/re-enable.

## Update

```sh
omarchy plugin update io.github.sandeshrai00.sorakey --yes
```

The QML updates in place. If the daemon source changed, the next shell start
(or plugin reload) re-runs the installer — verified prebuilt when one matches
the tagged source, else a source build — and restarts the daemon with the new
binary automatically.

Updates the plugin code. The daemon binary follows automatically: on the next
shell start (or re-enable) the plugin re-checks the installed binary against
the daemon source and swaps in the matching release prebuilt — or rebuilds if
the source moved past the last tag — then restarts the daemon if it changed.

## Remove

```sh
# stops the daemon, removes binary + service file, keeps soundpacks as .bak
~/.config/omarchy/plugins/io.github.sandeshrai00.sorakey/scripts/uninstall.sh
omarchy plugin remove io.github.sandeshrai00.sorakey --yes
```

`uninstall.sh --purge` deletes the soundpacks too.

## Import a soundpack

Click the keyboard icon on the bar → **Import pack…** → pick a `.zip`.
The pack is extracted to `~/.local/share/sorakey/soundpacks/keyboard/{id}/`
and appears in the keyboard dropdown on the next refresh. The ZIP must
contain a `config.json` (V2 format).

## Control API

`sorakey ctl '<json>'` speaks one JSON line in, one JSON line out, over
`$XDG_RUNTIME_DIR/sorakey.sock`:

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
| `scripts/sorakey-setup` | One-click installer |
| `scripts/sorakey-import-pack.py` | GTK4 file-picker + ZIP extractor |
| `scripts/build-sorakey.sh` | Verified-prebuilt-or-source daemon build |
| `scripts/uninstall.sh` | Removes daemon, unit file, binary (`--purge`: packs too) |
| `daemon/` | The `sorakey` Rust daemon (trimmed MechvibesDX core) |
| `daemon/soundpacks/` | Built-in V2 soundpacks |

## Layout

```
~/.local/bin/sorakey                 binary
~/.local/share/sorakey/soundpacks/    built-in + imported packs
~/.local/share/sorakey/data/config.json   settings (persisted by ctl)
~/.local/share/sorakey/bar-section    last bar section chosen in Settings
$XDG_RUNTIME_DIR/sorakey.sock         control socket
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