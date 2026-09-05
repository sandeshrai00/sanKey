# Rename List — soraKey

> How to use: fill the `New name` column with what you want.
> Leave blank = keep as-is. Don't rename files marked `DO NOT RENAME`.
> After you fill it, I will do the `git mv` + update all usages (`manifest.json`, imports, script paths, docs).

## 1. Plugin root (QML / JS) — main rename candidates

| # | Current path | What it is / where used | New name (you fill) |
|---|---|---|---|
| 1 | `Panel.qml` | Bar icon + popup panel (1707 lines). `manifest.json` -> `barWidget`. Should be `BarWidget.qml` per Omarchy docs. |  |
| 2 | `Service.qml` | Headless service. `manifest.json` -> `service`. Omarchy convention is `Service.qml` — keep recommended. |  |
| 3 | `Model.js` | Helpers `prettyPackName`, `packOptions`, `parseStatus`, `parsePacks`. Imported in `Panel.qml:8`, `SearchablePackDropdown.qml:5`. |  |
| 4 | `SoraDropdown.qml` | Themed dropdown fork. Used in `Panel.qml:1100,1174`. |  |
| 5 | `SoraTextField.qml` | Themed text input fork. Used in `Panel.qml:1689`, `SearchablePackDropdown.qml:198`. |  |
| 6 | `SearchablePackDropdown.qml` | Soundpack picker with search + delete. Used in `Panel.qml:1583`. |  |
| 7 | `manifest.json` | **DO NOT RENAME** — required by Omarchy. | — |
| 8 | `README.md` | **DO NOT RENAME** — required for publish. | — |
| 9 | `LICENSE` | **DO NOT RENAME** | — |
| 10 | `rust-toolchain.toml` | **DO NOT RENAME** — referenced by `scripts/build-sorakey.sh:29`. | — |
| 11 | `CLEANUP.md` | Dev note. Safe to rename/remove. |  |

## 2. Scripts — all renameable (must update `Panel.qml` + `Service.qml` paths)

| # | Current path | Used in | New name (you fill) |
|---|---|---|---|
| 12 | `scripts/sorakey-setup` (no ext) | `Panel.qml:20 setupPath`, `scripts/sorakey-setup:20` calls `build-sorakey.sh` |  |
| 13 | `scripts/build-sorakey.sh` | `Service.qml:240 freshnessCheck`, `scripts/sorakey-setup:20`, `admin-scripts/DEV_SYNC.md:58` |  |
| 14 | `scripts/sorakey-enable-capture.sh` | `Panel.qml:220,233`, `docs/keyboard-access.md:22,33,153` |  |
| 15 | `scripts/sorakey-detached-run` (no ext) | `Service.qml:67`, comment `Service.qml:37` |  |
| 16 | `scripts/sorakey-import-pack.py` | `Service.qml:73 startPick`, `admin-scripts/README.md:13`, `daemon/src/utils/config_converter.rs` docs |  |
| 17 | `scripts/sorakey-export-logs.py` | `Service.qml:74 startPick`, `README.md:138` |  |
| 18 | `scripts/_v1_shared.py` | Imported by `sorakey-import-pack.py:90` via filename, mirrored in `daemon/src/utils/config_converter.rs:786,864` |  |
| 19 | `scripts/uninstall.sh` | `Panel.qml:495`, `README.md:96,101`, `docs/keyboard-access.md:77,81`, `CLEANUP.md:24` |  |

## 3. Assets (referenced via `Qt.resolvedUrl` in `Panel.qml:28,980-981`)

| # | Current path | Used in | New name (you fill) |
|---|---|---|---|
| 20 | `assets/icon-bar-dark.svg` | `Panel.qml:28 barLogoSource` |  |
| 21 | `assets/icon-bar-light.svg` | `Panel.qml:28 barLogoSource` |  |
| 22 | `assets/icon-hero-dark.svg` | `Panel.qml:980 heroIcon` |  |
| 23 | `assets/icon-hero-light.svg` | `Panel.qml:981 heroIcon` |  |

## 4. udev + docs + admin-scripts

| # | Current path | Note | New name (you fill) |
|---|---|---|---|
| 24 | `udev/70-sorakey-keyboard.rules` | Keep `NN-*.rules` shape (udev convention). Ref: `docs/keyboard-access.md:22`, `scripts/sorakey-enable-capture.sh` |  |
| 25 | `docs/keyboard-access.md` | Linked from `Panel.qml:210 whyLearnMoreUrl` (GitHub URL) + README |  |
| 26 | `docs/dev/note.md` | Dev note |  |
| 27 | `docs/dev/plan.md` | Dev note |  |
| 28 | `docs/dev/relse.md` | Dev note (typo: `relse` -> `release`?) |  |
| 29 | `admin-scripts/dev-sync.sh` | Dev helper |  |
| 30 | `admin-scripts/DEV_SYNC.md` | Dev doc |  |
| 31 | `admin-scripts/IOHOOK_KEYCODES.md` | Dev doc |  |
| 32 | `admin-scripts/README.md` | Dev doc |  |
| 33 | `.github/workflows/release.yml` | **DO NOT RENAME** — GitHub path fixed | — |

## 5. Daemon `daemon/src/` — usually keep (Rust `mod.rs` convention)

Only rename if you want deeper cleanup. Renaming needs `mod.rs` updates.

| # | Current path | Note | New name (you fill) |
|---|---|---|---|
| 34 | `daemon/src/main.rs` | **DO NOT RENAME** — Rust entry | — |
| 35 | `daemon/src/control.rs` | Socket `ctl` handler |  |
| 36 | `daemon/src/libs/mod.rs` | **DO NOT RENAME** — Rust module file | — |
| 37 | `daemon/src/libs/bootstrap.rs` | Startup |  |
| 38 | `daemon/src/libs/cli_args.rs` | CLI args |  |
| 39 | `daemon/src/libs/device_manager.rs` | Audio devices |  |
| 40 | `daemon/src/libs/evdev_input_listener.rs` | Keyboard listener |  |
| 41 | `daemon/src/libs/audio/engine.rs` | Audio engine |  |
| 42 | `daemon/src/libs/audio/mod.rs` | **DO NOT RENAME** | — |
| 43 | `daemon/src/libs/audio/resampler.rs` | Resampler |  |
| 44 | `daemon/src/libs/audio/soundpack_loader.rs` | Pack loader |  |
| 45 | `daemon/src/state/mod.rs` | **DO NOT RENAME** | — |
| 46 | `daemon/src/state/config.rs` | Config |  |
| 47 | `daemon/src/state/config_writer.rs` | Config writer |  |
| 48 | `daemon/src/state/health.rs` | Health |  |
| 49 | `daemon/src/state/paths.rs` | Paths |  |
| 50 | `daemon/src/state/soundpack.rs` | State soundpack |  |
| 51 | `daemon/src/utils/mod.rs` | **DO NOT RENAME** | — |
| 52 | `daemon/src/utils/auto_startup.rs` | Autostart |  |
| 53 | `daemon/src/utils/config_converter.rs` | V1->V2 converter |  |
| 54 | `daemon/src/utils/constants.rs` | Constants |  |
| 55 | `daemon/src/utils/data.rs` | Data |  |
| 56 | `daemon/src/utils/keymap.rs` | Keymap |  |
| 57 | `daemon/src/utils/log_buffer.rs` | Log buffer |  |
| 58 | `daemon/src/utils/logger.rs` | Logger |  |
| 59 | `daemon/src/utils/path.rs` | Path helper |  |
| 60 | `daemon/src/utils/soundpack.rs` | Util soundpack |  |
| 61 | `daemon/src/utils/soundpack_validator.rs` | Validator |  |
| 62 | `daemon/src/utils/symphonia.rs` | Audio decode |  |
| 63 | `daemon/tests/version_sync.rs` | Version test |  |
| 64 | `daemon/Cargo.toml` | **DO NOT RENAME** | — |
| 65 | `daemon/Cargo.lock` | **DO NOT RENAME** | — |

## 6. Soundpacks — DO NOT RENAME (pack IDs = folder names, used by `ctl packs`)

`daemon/soundpacks/keyboard/sankey-*/*.ogg|jpg|config.json` — keep all as-is.

---

### Next step for you

Fill `New name` above, save, and tell me `done`. I will then `git mv` + patch usages + run `omarchy plugin validate` + `qmllint`.

## Quick fill — only renameable files (fill after `=`)

Panel.qml = SoraWidget
Service.qml = SoraService
Model.js = SoraKeyStore
SoraDropdown.qml = same
SoraTextField.qml = same
SearchablePackDropdown.qml = SoraPackPicker
CLEANUP.md = same
scripts/sorakey-setup = sora-install
scripts/build-sorakey.sh = sora-build
scripts/sorakey-enable-capture.sh = sora-keyboard-access
scripts/sorakey-detached-run =  sorakey-detached
scripts/sorakey-import-pack.py = sora-pack-import
scripts/sorakey-export-logs.py = sora-export-logs
scripts/_v1_shared.py = same
scripts/uninstall.sh = sora-uninstall
assets/icon-bar-dark.svg = same
assets/icon-bar-light.svg = same
assets/icon-hero-dark.svg = same
assets/icon-hero-light.svg = same
udev/70-sorakey-keyboard.rules = sora-keyboard-rules
docs/keyboard-access.md = same
docs/dev/note.md = same
docs/dev/plan.md = same
docs/dev/relse.md = same 
admin-scripts/dev-sync.sh = same
admin-scripts/DEV_SYNC.md = same
admin-scripts/IOHOOK_KEYCODES.md = same
admin-scripts/README.md = same
daemon/tests/version_sync.rs = daemon/tests/version_check.rs
daemon/src/control.rs = daemon/src/commands.rs
daemon/src/libs/bootstrap.rs = daemon/src/libs/startup.rs
daemon/src/libs/cli_args.rs = daemon/src/libs/names.rs
daemon/src/libs/device_manager.rs = daemon/src/libs/speakers.rs
daemon/src/libs/evdev_input_listener.rs = daemon/src/libs/keyboard.rs
daemon/src/libs/audio/engine.rs = daemon/src/libs/player.rs
daemon/src/libs/audio/resampler.rs = daemon/src/libs/sound_quality.rs
daemon/src/libs/audio/soundpack_loader.rs = daemon/src/libs/pack_loader.rs
daemon/src/state/config.rs = daemon/src/state/settings.rs
daemon/src/state/config_writer.rs = daemon/src/state/settings_saver.rs
daemon/src/state/health.rs = daemon/src/state/status.rs
daemon/src/state/paths.rs = daemon/src/state/folders.rs
daemon/src/state/soundpack.rs = daemon/src/state/packs.rs
daemon/src/utils/auto_startup.rs = daemon/src/utils/auto_start.rs
daemon/src/utils/config_converter.rs = daemon/src/utils/old_pack_fixer.rs
daemon/src/utils/constants.rs = daemon/src/utils/version.rs
daemon/src/utils/data.rs = daemon/src/utils/json_files.rs
daemon/src/utils/keymap.rs = daemon/src/utils/keys.rs
daemon/src/utils/log_buffer.rs = daemon/src/utils/logs.rs
daemon/src/utils/logger.rs = daemon/src/utils/printer.rs
daemon/src/utils/path.rs = daemon/src/utils/files.rs
daemon/src/utils/soundpack.rs = daemon/src/utils/pack_info.rs
daemon/src/utils/soundpack_validator.rs = daemon/src/utils/pack_checker.rs
daemon/src/utils/symphonia.rs = daemon/src/utils/sound_reader.rs
