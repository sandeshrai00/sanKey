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
omarchy plugin add https://github.com/sandeshrai00/soraKey.git --enable
```

1. Open the Sorakey panel on your bar and click **Install Sorakey**. It installs
   `sorakey` and starts the service. The installer prefers the **prebuilt
   binary from GitHub Releases** (a few seconds): it verifies the release
   checksum, and when `gh` is logged in (or `GH_TOKEN` is set) also the GitHub
   attestation proving CI built it from the tagged commit. Without a `gh`
   login the checksum check alone is accepted. It only builds from source
   (needs Rust, a few minutes) when no release matches the current daemon
   source — e.g. you changed `daemon/` but haven't tagged a new version yet.
2. For **keyboard** sounds on Wayland your user needs the `input` group once.
   The installer never runs `sudo` itself — it only checks. If blocked,
   the panel shows **No keyboard access** with Copy/Rescan/Restart
   instead of silent `Playing`:

    ```
    sudo usermod -aG input $USER
    ```

    then log out and back in, then Restart sorakey from the panel.
    Verify with `groups` (must contain `input`) and
    `journalctl --user -u sorakey | grep evdev` (must show `Found N`,
    not `Found 0`).

## Usage

- **Left-click** the keyboard icon → panel: mute switch, per-pack volume
  slider, soundpack dropdown (search, delete), **Random**, **Import pack…**,
  **Open folder**, a live **Test typing** box, and Start/Stop/Restart.
- **Right-click** → toggle mute. **Scroll** on the icon → volume.
- **Ctrl+Alt+M** → global mute, from anywhere (system-wide hotkey).
- **Escape** closes the panel, **Tab** / **Shift+Tab** switches panels.
- **Settings** (gear in the panel header): bar icon position, audio output
  device, and **Export error logs** (saves a report of recent errors to a
  file via a GTK save dialog, or `~/Downloads` when run from a terminal).

The volume slider is **per soundpack**: each pack keeps its own level, and
clicking the percentage label resets it to that pack's recommended default.
Packs whose `config.json` sets `options.recommended_volume` start at that
level automatically.

## Output device

Settings → **AUDIO OUTPUT** lists the system's output devices (plus
**System default**). Pick one to route keyboard sounds there; **Rescan
devices** refreshes the list. The choice persists and is reported by
`status` as `audio_device`.

## Configure

```sh
omarchy bar move io.github.sandeshrai00.sorakey --section center
```

Mute, volume and soundpack persist across restarts
(`~/.local/share/sorakey/data/config.json`); the icon section persists across
disable/re-enable.

## Auto-start

The daemon is a `systemd` user service, enabled at install, so it starts
automatically at login. The panel's **Stop** halts it for the current session
(without disabling it) — it comes back at the next login. **Start** brings it
back immediately. To turn auto-start off entirely, disable the unit
(`systemctl --user disable sorakey`) or remove the plugin (see Remove).

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
omarchy restart shell
```

`uninstall.sh` moves `~/.local/share/sorakey` (packs **and** settings) to a
`.bak` timestamped folder; `uninstall.sh --purge` deletes it instead. The
final `omarchy restart shell` drops the bar icon.

## Import a soundpack

Click the keyboard icon on the bar → **Import pack…** → pick a `.zip`.
The pack is extracted to `~/.local/share/sorakey/soundpacks/keyboard/{id}/`
and appears in the keyboard dropdown on the next refresh. The ZIP must
contain a `config.json` (V2 format).

## Control API

`sorakey ctl '<json>'` speaks one JSON line in, one JSON line out, over
`$XDG_RUNTIME_DIR/sorakey.sock`:

- `status` — running, muted, volume, per-pack volume, active pack, audio device
- `mute {"muted": true|false}`
- `volume {"value": 0-100}` — global level (used when no pack is selected)
- `per_pack_volume {"id": "keyboard/...", "value": 0-100}`
- `reset_volume {"id": "keyboard/..."}` — back to the pack's recommended level
- `keyboard_pack {"id": "keyboard/..."}` — switch the active pack
- `packs` — list available packs
- `delete_pack {"id": "keyboard/..."}` — remove a pack (falls back to another)
- `audio_devices` — list output devices + current selection
- `select_device {"id": "..."}` — route output there (`null` = system default)
- `test_sound` — play one key sound (proves output works without typing)
- `input` / `input_rescan` — keyboard-capture health (`ok`, `total`, `keyboards`, `hint`); rescan re-probes `/dev/input` without restart
- `set_bar_section {"section": "left|center|right"}` / `get_bar_section`
- `diag` — memory (RSS/HWM), per-pack entry + cache sizes, active pack
- `export_logs` — recent error log as text, with a suggested filename

## Diagnostics & logs

- **No sound?** Check in order: panel status (`No keyboard access` = input
  permission — see Install step 2), **Test sound** button (proves output),
  mute/volume, selected pack, output device. `sorakey ctl '{"cmd":"status"}'`
  reports `input.ok` alongside audio state.
- **Privacy**: key capture is local-only via `/dev/input`. Key names never
  leave the machine; logs mask account names on export.

- **Memory / cache**: `sorakey ctl '{"cmd":"diag"}'` reports resident
  memory, the soundpack cache size, and per-pack volume entries — useful for
  spotting unbounded growth.
- **Error logs**: the daemon keeps a rolling buffer of recent errors.
  Settings → **Export error logs** opens a GTK save dialog (or run
  `scripts/sorakey-export-logs.py <name>` from a terminal to write straight to
  `~/Downloads`). The file is named `sorakey-log-<timestamp>.txt`.

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
| `scripts/uninstall.sh` | Removes daemon, unit file, binary (`--purge`: all data too) |
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