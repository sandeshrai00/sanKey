# Sorakey Codebase Audit

Full audit of the sorakey plugin — daemon (Rust), panel (QML), scripts, and CI.
Generated: 2026-09-01

---

## Architecture Overview

- **Daemon** (`daemon/`, Rust 1.87, edition 2024): systemd user service. `main.rs` acquires a flock, spawns an audio engine thread (rodio/cpal), a Unix-socket control server (`control.rs`, one JSON line in/out), and an evdev input listener. State lives in `state/` (config behind a `OnceLock<Mutex>` with a single-writer `apply()` API, atomic file writes), utils in `utils/` (symphonia decode, rubato resample, V1→V2 conversion, log ring buffer, trace).
- **Plugin** (root QML/JS): `Panel.qml` (bar icon + panel, polls `sorakey ctl status`/`packs` via forked `Process`), `Service.qml` (shell-side lifecycle: enable/start daemon, freshness rebuild, import/export helpers), `Model.js` (parse helpers), `SearchablePackDropdown.qml` (searchable list + inline delete).
- **Scripts**: `build-sorakey.sh` (verified-prebuilt-or-source), `sorakey-setup` (installer + unit), `sorakey-import-pack.py` (GTK4 file dialog + zip extraction + V1→V2), `sorakey-export-logs.py`, `uninstall.sh`, `_v1_shared.py` (shared V1 keycode tables).

The codebase has already been through one optimization pass (git log: "ram leak fix", "remove dead and clean up code"). Findings below are the **current** state.

---

## 1. Bugs

### B1. Unbounded memory leak: `UiEvent` channel never drained (every keystroke leaks)

`daemon/src/libs/audio/engine.rs:582` — for *every* key event the engine does `event_tx.send(UiEvent::KeyDown/KeyUp)`. The receiver (`UI_EVENT_RX`, line 72) is stored in a `OnceLock` and the only accessor, `ui_event_receiver()` (line 107), **has zero callers** (verified by grep — no consumer exists anywhere in the daemon). Crossbeam `unbounded` + never-read = one leaked allocation per keystroke, forever. A long-lived systemd service grows without bound. Lines 514, 527, 548 leak command events too (bounded in practice).

**Fix:** delete the whole `UiEvent`/`event_tx`/`UI_EVENT_RX`/`ENGINE_HANDLE`/`engine_handle()`/`ui_event_receiver()` plumbing, or consume it.

### B2. Missing `XDG_RUNTIME_DIR` → daemon exits with misleading "already running"

`daemon/src/main.rs:50-61` — `acquire_lock()` returns `None` both when the lock is held *and* when `XDG_RUNTIME_DIR` is unset (line 51 `.ok()?`), and `main.rs:22-28` treats both as "already running" and exits 1. Meanwhile `control.rs:11-16` gracefully falls back to `$HOME/.sorakey.sock` when the var is missing. Inconsistent: on such systems the daemon can never start, and the error message is wrong.

### B3. V1 keycode tables diverge between the Python importer and the Rust in-daemon converter

`scripts/_v1_shared.py` (used by panel import) vs `daemon/src/utils/config_converter.rs:736-938`. For the same V1 pack, the two conversion paths produce *different* key→sound mappings:

| V1 code | Python final value | Rust final value |
|---------|-------------------|-----------------|
| 3597 | ControlRight | NumLock (`config_converter.rs:895` overwrites `:850`) |
| 3612 | NumpadEnter | NumpadDivide (`:896` overwrites `:849`) |
| 3613 | ControlRight | NumpadMultiply (`:897`) |
| 3640 | AltRight | Numpad8 (`:899`) |
| 3675 | NumpadDecimal | Numpad3 (`:907`) |
| 3676 | Numpad0 | NumpadEnter (`:908`) |
| 3677 | (absent) | Numpad0 (`:909`) |

Verified by executing the Python dict: it has 163 entries but only 159 unique keys.

### B4. Silent duplicate keys in Python `V1_KEY_TABLE`

`scripts/_v1_shared.py:51-55` vs `:65-69` — keys `3597`, `3612`, `3613`, `3640` each appear twice; Python silently keeps the last. Same class of bug in the Rust `HashMap` ("Alternative range" block at `config_converter.rs:895-924` overwrites the "CORRECT" block). Nobody can see this; a linter/warning would have caught it.

### B5. Read-path destroys user data: multi-method packs are permanently downgraded on cache refresh

`daemon/src/utils/soundpack.rs:76-102` — during a *metadata scan* (`SoundpackCache::load()` → `refresh_from_directory` → `load_soundpack_metadata`), any V2 pack with `definition_method: "multi"` is run through `convert_v2_multi_to_single` which **overwrites `config.json` on disk** (`config_converter.rs:475`). In that conversion, keys using a different audio file than the "most used" one are **dropped entirely** (`config_converter.rs:443-448`). Yet the engine explicitly *supports* multi packs (`soundpack_loader.rs:236-295`), and the panel importer deliberately produces multi configs (`sorakey-import-pack.py:225-232`). So: import a multi-pack → next cache refresh (triggered by first `diag`, first delete, first load, or empty cache at startup) silently corrupts it. A read command (`diag`) with destructive side effects is the worst kind.

### B6. `trace::init()` is never called — `SORAKEY_TRACE=1` is dead

`daemon/src/libs/trace.rs:151` (`init()`, documented "Call once early in main") has no callers; `main.rs` never calls it. Consequently `log_buffer::set_verbose` (`log_buffer.rs:83`, the only other enabler, which calls `trace::set_runtime_tracing`) is *also* unreachable — no `ctl` command, no panel control. The entire verbose/latency-tracing feature (trace.rs 120-316, ~200 LOC of writer thread) is inert at runtime. Only tests can reach it.

### B7. Engine death leaves a "zombie" daemon reporting healthy

`daemon/src/libs/audio/engine.rs:177-184` — if the selected *and* default audio device fail, the engine thread `panic!()`s. The main thread is parked (`main.rs:46`), so the process keeps living, the socket keeps answering `status → running:true`, and the panel shows "Playing" while no sound will ever play. No health signal, no restart (systemd `Restart=on-failure` never fires because the process doesn't die).

### B8. `switch_device` resamples to the wrong rate

`daemon/src/libs/device_manager.rs:324-337` — `get_current_output_sample_rate()` resolves the device from **config** (`config.selected_audio_device`), not from the stream actually being opened. `engine.rs:247` uses it in `switch_device` after opening a *different* device, so cached samples get resampled to the old device's rate (wrong pitch/speed). Currently masked only because `SwitchDevice` is unreachable (no `ctl` command for it) — but the code path exists and is wrong.

### B9. `bootstrap.rs` fallback starts both listeners (the exact thing its comment forbids)

`daemon/src/libs/bootstrap.rs:11-19` — comment says "rdev would double-fire", yet the `else` branch starts evdev *and* rdev. On Linux rdev's backend is also evdev, so if `evdev::enumerate()` saw no keyboard, rdev won't either — the fallback is dead code, and if it ever did work it would double-play.

### B10. No request-size limit on the control socket

`daemon/src/control.rs:51-66` — `read_line` into an unbounded `String`. The socket is 0600 (same user only) so risk is low, but a runaway local client (or a stuck `ctl`) can make the daemon OOM.

### B11. `AlsaErrorSuppressor` swaps process-wide stderr

`daemon/src/libs/device_manager.rs:16-46` — `dup2` on `STDERR_FILENO` is not thread-safe: while a device enumeration runs, `eprintln!` from any other thread (the engine logs constantly) is silently eaten or interleaves with the fd swap.

### B12. Shell-concatenation in panel breaks for `$HOME` with spaces

`Panel.qml:173-178` — `"/bin/sh", "-c", "mkdir -p " + root.home + "/..."` with no quoting. Any user whose home path contains a space gets a broken bar-section save (and a latent injection footgun). Same pattern risk at `Panel.qml:184` (read).

### B13. `rdev` fallback listener has per-keystroke locks and drops fast keystrokes

`daemon/src/libs/input_listener.rs:176-186` — 1ms/10ms rate-limiting at the listener layer ("drops fast typists") plus `Arc<Mutex>` traffic inside the event callback. Dead path on healthy systems (B9), but wrong if revived.

### B14. Config `enable_keyboard_sound` is a no-op

`daemon/src/state/config.rs:20` — stored, compared in `data_equals`, but never read by the engine (only `enable_sound` gates playback, `engine.rs:208`). A config field that does nothing.

### B15. V1 multi-conversion concatenation order is unstable for press/release pairs

`daemon/src/utils/config_converter.rs:92-110` — `audio_files_ordered` sorts by numeric code; a press and its release with the same numeric code (e.g. `"30"`/`"030"`) tie, and `sort_by_key` is stable over `HashMap` iteration order — i.e. arbitrary. If press and release map to different files, the concatenated audio order (and thus timings) is random per run.

### B16. Symphonia decode swallows errors

`daemon/src/utils/symphonia.rs:44-54` — `next_packet()`/`decode()` errors → `break`/`continue` with no log. A partially-decodable file yields truncated audio with the segment timings pointing past the buffer (then the noisy "start sample past end" spam in `engine.rs:341-349`).

---

## 2. Dead Code (verified by grep)

| Item | Location | Note |
|------|----------|------|
| `input_worker_host.rs` (498 LOC) | `daemon/src/libs/input_worker_host.rs` | **Orphaned file**: not declared in `libs/mod.rs`, Windows-only (`#![cfg(target_os="windows")]` inner attr on a non-crate file), references a nonexistent `crate::libs::input_worker` module. Only reachable via `include_str!` in `log_buffer.rs:545` tests. |
| `rand` dependency | `daemon/Cargo.toml:19` | Zero uses. (`random_pitch` is parsed in `state/soundpack.rs:25` but never implemented.) |
| `env_logger` + `log` | `Cargo.toml:12`, `main.rs:19` | `env_logger::init()` called, but no `log::` macro anywhere — all logging goes through custom `println!`-based macros. Init is a no-op. |
| `UiEvent`, `engine_handle()`, `ui_event_receiver()`, `ENGINE_HANDLE`, `UI_EVENT_RX` | `engine.rs:44-109` | See B1 — entire event API has no consumer (GUI was stripped from the fork). |
| DeviceManager: `get_output_devices`, `get_input_devices`, `get_input_device_by_id`, `test_output_device`, `test_input_device`, `device_supports_rate`, custom `Clone` | `device_manager.rs:76-82, 91-263, 293-354` | No callers outside the module. |
| `auto_startup::set_auto_startup` | `utils/auto_startup.rs:4-15` | No `ctl` command sets `auto_start`; the function is unreachable. |
| `paths::soundpacks::is_builtin_soundpack`, `get_soundpacks_dir` | `state/paths.rs:50, 87` | No callers. |
| `log_buffer::export_to_file`, `export_file_name`, `reveal_in_file_manager`, `recent` (runtime), `generation` (runtime) | `utils/log_buffer.rs:57-75, 213-237` | Only tests use them; the panel export goes through `ctl export_logs` → `export_contents`. |
| `trace::init`, `trace::now_ms` (external), `trace::enabled` (external) | `libs/trace.rs:74, 79, 151` | See B6. |
| `log_buffer::set_verbose` / `verbose_enabled` (external) | `log_buffer.rs:78-91` | See B6. |
| `bootstrap.rs` else-branch evdev start | `libs/bootstrap.rs:12-16` | See B9. |
| `impl SoundPack {}` / `impl SoundpackType {}` | `state/soundpack.rs:85-87` | Empty impl blocks. |
| `SoundpackType` single-variant enum, `SoundpackCount` single-field struct | `state/soundpack.rs:7-10, 123-126` | Scaffolding for a mouse/device model that was cut from the fork. |
| `debug_print!`/`debug_eprint!` vs `always_print!`/`always_eprint!` | `utils/logger.rs:1-7, 10-56` | `is_debug_enabled()` is hardcoded `true` — the "debug" pair is byte-for-byte the same as the "always" pair. Two of the four macros are redundant. |
| Icon "dynamic asset URLs" | `libs/audio/soundpack_loader.rs:147-159`, `utils/soundpack.rs:175-202` | Emits `/soundpack-images/{id}/{icon}` URLs "served by the asset handler" — this daemon has no HTTP/asset handler; the cache field is never read by anything. |
| `SoundpackMetadata.can_be_converted`, `last_accessed`, `validation_status` (mostly) | `state/soundpack.rs:104-111` | Written, never acted on by any consumer in this fork. |

---

## 3. Performance Issues

1. **evdev 20 ms busy-poll** — `evdev_input_listener.rs:137`: non-blocking `fetch_events()` on every keyboard, then `thread::sleep(20ms)`. Adds up to 20 ms input latency, ~50 wakeups/sec at idle, and no eventfd/epoll wait. This is the hot path for a keystroke-sound app.

2. **Per-keystroke allocations** — `engine.rs:374` `.to_vec()` of the segment slice + `Sink::try_new` + `SamplesBuffer::new` per key. At fast typing with 32 max voices this is the constant allocation pressure; the fade is applied to the copy (correct, but the copy could be precomputed once per (pack, key, segment) and cached — timings are static per pack).

3. **Keystroke → leaked `UiEvent`** — B1.

4. **Pack load blocks the engine thread** — `run_engine` decodes (symphonia) + resamples (sinc 64/32 BlackmanHarris2) inline (`soundpack_loader.rs`, `resampler.rs`). For a multi-second sprite that's a multi-second silence + UI stall on every pack switch (~2.5 s for a 24 MB pack). `LoadKeyboardPack` should run on a worker thread; the engine only swaps state when done.

5. **Resampler allocation churn** — `resampler.rs:68-79`: per chunk (1024 frames) × per channel `.to_vec()` copies of deinterleaved data, then re-interleave copies again (`:108-113`). Thousands of allocations per pack; could process slice views.

6. **`current()` deep-clones the whole `AppConfig`** (incl. `per_pack_volume` map) on every call — `config_writer.rs:18-24`; `control.rs:86-96` calls it 2–3× per volume request; `get_current_output_sample_rate` (device_manager.rs:325) calls it per invocation. An `RwLock`/`Arc` snapshot would be cheaper.

7. **`SoundpackCache::load()` does disk read + possible full rescan** (and destructive conversion, B5) on every `diag` — `control.rs:315`. A read-only status command with filesystem side effects.

8. **Panel forks 2 processes every 5 s while open** — `Panel.qml:389-397`: `ctl status` + `ctl packs` every poll. `packs` rarely changes; 30–60 s cadence (or a generation counter) would halve the fork load. Plus the 30 s closed poll (`:399-404`) and 5 s not-installed poll (`:407-415`).

9. **`build-sorakey.sh` freshness check runs at every shell start** — `Service.qml:156-170`: git status/diff + (offline/untagged) a full `cargo build --release` at login. Also the source hash (`build-sorakey.sh:22-23`) includes *everything* under `daemon/` — including `target/` if present — so the "up to date" short-circuit is unreliable; in a non-git installed plugin `release_matches_source` always fails, so the prebuilt path is dead there and it's always a cargo no-op build at least once.

10. **`DeviceManager` clones a fresh `Host` per clone** (`device_manager.rs:76-82`) — dead code, but if revived it re-enumerates.

---

## 4. UX Problems (Panel/Service)

1. **Invisible icon glyphs (U+FF30, zero-width filler)** — verified by hex dump:
- `SearchablePackDropdown.qml:326` `delBtn` `text: "\uFF30"` — the **delete button renders nothing**; the affordance is only discoverable by hover.
- `SearchablePackDropdown.qml:379` — the red "delete warning" icon is also invisible; the confirm footer is text-only.
- `Panel.qml:838` — `removeButton` `iconText: "\uFF30"` with `text: ""` → a completely empty button whose only identifier is a tooltip.
These look like Nerd Font glyphs that got mangled. The user cannot see delete/uninstall affordances at all.

2. **Failed `ctl` commands give no feedback** — `Panel.qml:91-96` (`sendCtl`) silently drops queued commands if `ctlProc.running`, and error responses (bad id, "daemon not running") are never surfaced; the UI just re-polls status. No toast/error path for `keyboard_pack`/`volume`/`mute` failures.

3. **Mute switch is misleading when the daemon is stopped** — `Panel.qml:515` `checked: root.running && !root.muted`: stopping the daemon flips the switch to "off" as if the user muted. Toggling it while stopped sends ctl that silently fails.

4. **Right-click mute and Ctrl+Alt+M are undiscoverable** — the tooltip (`Panel.qml:424`) only shows status; the hotkey exists only in the daemon's startup banner (`main.rs:44`). README documents it, but nothing in the UI does.

5. **"TEST TYPING" box is a weak feature** — `Panel.qml:774-794`: a plain `TextField` with no key feedback (no visual "pressed" indicator, no count, no clear button). It does nothing beyond "type here".

6. **Delete-all leaves an unexplained silent state** — deleting the last pack sets `keyboard_pack: ""`; the engine goes quiet, the dropdown is empty, the slider is disabled — but status still says "Playing" (`Panel.qml:70-75`). There's no "no soundpack selected" state.

7. **Per-pack volume can't be reset** to the pack's `recommended_volume` (the daemon tracks it, `control.rs:131-136`, but there's no "reset" control).

8. **Bar-section save uses a shell one-liner writing `~/.local/share/sorakey/bar-section`** (Panel.qml:169-191) — a file-based setting that duplicates what could be a plugin setting; also the restore logic (`:182-191`) runs a fork on every panel construction.

9. **Update flow parses free-form CLI output** — `Panel.qml:216-219` matches `"is up to date"`/`"Updated"` substrings in `omarchy plugin update` output — fragile to wording changes; errors show only the last stderr line.

10. **`Stop` is not sticky** — `Service.qml:147-153` re-enables/starts the daemon on every shell start, silently undoing the user's Stop.

11. **Import/export status strings clear after 4 s** (`Service.qml:65`) — an error the user misses is gone; no history.

12. **Duplicate "Update" button** — bottom row (`Panel.qml:822-831`) and Settings (`:595-604`) are two copies of the same control; the bottom-row spacer math (`:833`) is manual width arithmetic.

13. **Nerd Font dependency** — bar icon `󰌌` and all `iconText` glyphs require a Nerd Font; without one, the bar shows tofu with no fallback.

14. **aarch64 has no prebuilt** — CI builds x86_64 only (`release.yml:26-30`), while `build-sorakey.sh:16-17` anticipates `sorakey-aarch64` → every ARM user compiles from source at install.

---

## 5. Missing Features (expected in a keyboard-sound plugin)

1. **Output device selection** — the engine fully supports it (`AudioCommand::SwitchDevice`, `engine.rs:41, 236-283`; `DeviceManager` has all the lookup code) but there is no `ctl` command and no UI. A dead, and buggy (B8), capability.

2. **Auto-start toggle** — `auto_startup.rs` implements enable/disable via `systemctl`; no `ctl` command, no UI checkbox. The config field exists and is even synced on load (`config.rs:200-210`) but can never be set by the user.

3. **Log viewer** — `log_buffer` doc says "Shown in Settings" (`log_buffer.rs:1`); the panel only offers *export*. No in-panel recent-logs view.

4. **Verbose/latency tracing** — entire `trace.rs` machinery unreachable (B6); no UI toggle, no `ctl` command.

5. **`random_pitch`** — parsed from pack options (`state/soundpack.rs:24-25`) and written by the converter, but never applied (no `rand` usage). Pack authors setting it get no effect.

6. **No diagnostics/health surface** — `diag` (memory, cache size, pack) exists in `control.rs:311-327` but nothing displays it; no "input capture working?" check (the #1 failure mode: missing `input` group → silently no sounds; the daemon just runs quiet and the panel shows "Playing").

7. **No pack metadata display** — name/author/description/icon/tags are scraped into `SoundpackCache` but the UI shows only an id-derived pretty name (`Model.js:4-11`); icons are never rendered (dead asset URL, see dead code).

8. **No hotkey configuration/disable** — Ctrl+Alt+M is hardcoded in three places (evdev listener, rdev listener, and the orphaned Windows host).

9. **No per-key tuning** (per-key volume/mute), no key-press preview, no "test this pack" beyond the free-text box.

10. **No aarch64 prebuilt** (see UX 14).

11. **No `ctl` command for `diag`-less health**, no way to trigger a pack-cache rescan after manual file drops into the soundpacks dir (the panel's "Open folder" invites exactly that, but the list only refreshes on the 5 s poll — OK — while `SoundpackCache` can stay stale/corrupt, see B5).

12. **Uninstall gaps** — `uninstall.sh` leaves `~/.cache/sorakey` (build tree), `~/.local/lib/sorakey` (source hash), and `~/.local/share/sorakey/bar-section` behind (contradicting `CLEANUP.md`'s own checklist).

---

## 6. Code Organization Issues

1. **Three parallel key-mapping tables for the same W3C names** — `input_listener.rs:8-126` (rdev), `evdev_input_listener.rs:142-204` (evdev), `config_converter.rs:736-938` (IOHook). Divergence already happened (B3).

2. **Three parallel V1→V2 converters** — Rust `config_converter.rs`, `scripts/sorakey-import-pack.py:166-274`, `admin-scripts/v1-to-v2-converter.py:130-217`. Tables are shared via `_v1_shared.py`, but the conversion *logic* is triplicated and the two Python copies already differ (case-insensitive matching in the importer, not in the admin script; shared-file detection differs).

3. **Read functions with write side effects** — `utils/soundpack.rs::load_soundpack_metadata` (metadata read) performs in-place V1→V2 conversion with backup (`:30-67`) *and* destructive multi→single conversion (`:76-102`). The "load" call graph of a status command rewrites user files.

4. **Orphaned Windows module** left in-tree (`input_worker_host.rs`), pulled into tests via `include_str!` — so deleting it breaks `log_buffer.rs:545`'s test; the test is what keeps a dead file alive.

5. **`libs/bootstrap.rs` comment contradicts its code** (B9) — the "why" documentation lies.

6. **Process docs shipped at plugin root** — `plan.md` (an internal optimization plan with line numbers), `relse.md` (release runbook, typo'd name), `note.md`, `CLEANUP.md` all live in the plugin dir that `dev-sync.sh` rsyncs to users' `~/.config`. They're harmless but noise; `relse.md`'s name suggests a typo'd `release.md`.

7. **Two different "builtin soundpacks" concepts** — `paths.rs:37-48` hardcodes the 10 builtin ids as a constant list, while `collect_packs`/`ensure_soundpack_directories` treat the directory as the source of truth; the constant is unused (dead) and will drift from the actual `daemon/soundpacks/` contents (which currently has exactly those 10 — fragile).

8. **`state/` ↔ `utils/` split is arbitrary** — `state/soundpack.rs` (cache) vs `utils/soundpack.rs` (metadata+conversion) vs `utils/soundpack_validator.rs` vs `utils/config_converter.rs` — four modules for the soundpack lifecycle, with the conversion reachable from the cache layer.

9. **Meta-tests via `include_str!` source scanning** (`engine.rs:600-663`, `log_buffer.rs:543-563`) pin implementation details ("source must not contain `DeviceWatchdog`"). They work but couple tests to wording and will rot; the engine one also asserts on a file that no longer exists in the build (`input_worker_host.rs`).

10. **`config.rs` migration table** (`:156-170`) — hardcoded rename list that only grows; fine for now, no versioned-migration structure.

11. **`manifest.json` and `Cargo.toml` versions are manually kept in sync** (both 0.1.1) — `build-sorakey.sh` reads only the manifest; drift is possible.

12. **`scripts/__pycache__` and `admin-scripts/__pycache__` are checked into the repo** — build artifacts in a git repo (`.gitignore` is 34 bytes and clearly doesn't cover them).

---

## 7. Security Concerns

Mostly good; the issues are small:

1. **`Panel.qml:173-178` shell string concatenation with `$HOME`** (B12) — breaks with spaces; theoretically injectable if the environment were ever attacker-influenced. Use Quickshell file APIs instead of `sh -c`.

2. **No max request size on the control socket** (B10) — local-only (socket is `chmod 0600`, `control.rs:29`, and the lock file is in `XDG_RUNTIME_DIR`), so it's a robustness issue, not a privilege boundary.

3. **Prebuilt-binary trust model** — `build-sorakey.sh:56-82`: SHA256SUMS is fetched from the *same* release as the binary; without `gh` attestation a MITM/takeover of the GitHub release could swap both and still pass. The attested path is correct but optional (needs `gh auth login`); the script says so honestly. Acceptable, but worth documenting that "checksum verified" ≠ "provenance verified".

4. **`Service.qml:178` `pkill -x sorakey`** — kills *any* user process named `sorakey` (name collision risk, low).

5. **Log export privacy** — `log_buffer` masks usernames in paths (`mask_user_paths`, good) and key identities in verbose lines (good), but `evdev_input_listener.rs:17` logs `Current user: {:?}` directly, which `mask_user_paths` won't catch (it's not a path component) → username leaks into exported logs.

6. **Zip import** — traversal guards are present (`sorakey-import-pack.py:426-432`), zip-bomb cap is on *decompressed* size (`:404-410`, correct), extraction to a temp dir then rename (`:411-442`, good). No issues found.

7. **`delete_pack`** — double-checked: id validation (`control.rs:203-210`) + `canonicalize` containment check (`:216-220`). Solid.

8. **`dev-sync.sh` copies `.git` into `~/.config`** — intentional (documented in `DEV_SYNC.md`), but git config (user email/name) ends up in the installed plugin tree; benign, worth knowing.

9. **systemd unit** (`sorakey-setup:30-42`) — user-scoped, no privilege; `Restart=on-failure` with no `StartLimitIntervalSec` tuning → a crash-looping daemon (e.g., B7 panic is *not* a crash, but a real crash would be) retries every 2 s indefinitely.

---

## 8. Accessibility Issues

1. **Invisible glyphs on interactive controls** (UX-1): the delete button, delete-warning icon, and uninstall button have no visible icon — a sighted user using only the pointer cannot find them without hover; a screen reader gets empty text. Replace with visible icons or text buttons.

2. **Custom `Rectangle` delegates have no `Accessible` role/name** — `SearchablePackDropdown.qml:286-355` (row, delete button) and the confirm footer buttons are built from raw `Rectangle`/`Text`/`MouseArea`; QML's default accessibility won't expose them as list items or buttons. `delBtn` is a `Text` with a `MouseArea` — no `Accessible.role: Accessible.Button`, no `onAccessiblePress`.

3. **Bar button semantics** — right-click toggles mute, wheel changes volume, left-click opens the panel (`Panel.qml:425-439`) — three different actions on one control with no `Accessible` description of any of them beyond the status tooltip.

4. **Keyboard navigation** — the dropdown is actually good (arrows/j-k/Enter/Esc, `:203-284`); but the trigger's `Keys.onPressed` handles Space/Enter/Down while the inner `MouseArea` steals clicks — fine; however the **trigger has no visible "open/closed" state** for non-mouse users (only border via `controlSpec`).

5. **Color-only state** — the bar icon dims when stopped (`Panel.qml:422`) and changes `active` when muted (`:423`); if the host renders both the same, mute state is invisible. Delete/red uses hardcoded `#ff6b6b` (`SearchablePackDropdown.qml:380,402`) with no contrast guarantee on light themes.

6. **Tofu without Nerd Font** (UX-13) — every icon is a private-use glyph; no `fontFallbacks`.

7. **`TextField` test box** has a placeholder but no `Accessible.name`/label association (`Panel.qml:778-785`).

---

## 9. Dependency Notes (`daemon/Cargo.toml`)

- **Unused**: `rand` (0.9.0), `env_logger` (0.11.6, + its `log` transitive). Remove.
- **Questionable**: `directories` (6.0) is used only for `BaseDirs::new()` in `state/paths.rs:7-11` — replaceable with `XDG_DATA_HOME`/`$HOME` env reads (3 lines, drops a crate).
- **Reasonable**: `crossbeam-channel`, `evdev`, `hound` (WAV write in converter), `libc` (flock/malloc_trim), `rdev` (dead fallback path, B9 — removable if the fallback is deleted), `rodio`+`cpal` (note both are declared; cpal comes via rodio anyway and is used directly for device enumeration), `rubato`, `serde`/`serde_json`, `symphonia`, `chrono`.
- `[profile.dev] opt-level = 2` with `debug = true` — deliberate for a daemon developed locally; fine.

---

# Complete Fix Plan — All Issues, Phase by Phase

Each phase is independently deployable, testable, and builds on the previous. No phase introduces new bugs while fixing old ones.

## Phase 1 — Data Safety (prevent user data loss)

**Goal:** No read command can ever modify user files. V1 conversion is deterministic and consistent.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 1.1 | **B5: Remove destructive multi→single from read path** | `daemon/src/utils/soundpack.rs:76-102` | Delete the `convert_v2_multi_to_single` call from `load_soundpack_metadata`. The engine already supports multi packs. If a pack is truly broken, the load will fail at playback time with a clear error. |
| 1.2 | **B3/B4: One authoritative V1 keycode table** | `daemon/src/utils/config_converter.rs:736-938`, `scripts/_v1_shared.py` | Create a single canonical mapping. Fix the Rust "Alternative range" block that overwrites the "CORRECT" block. Dedup the Python dict (163 entries → 159 unique). Add a compile-time/test assertion that no key appears twice. |
| 1.3 | **B15: Stable sort for V1 multi-conversion** | `daemon/src/utils/config_converter.rs:92-110` | Sort by `(numeric_code, is_release)` so press always comes before release. Deterministic output. |
| 1.4 | **B16: Log symphonia decode errors** | `daemon/src/utils/symphonia.rs:44-54` | Replace silent `break`/`continue` with `eprintln!` + a counter. If >10% of packets fail, mark the pack as corrupted in the cache. |

**Verify:** Import a multi-pack → run `diag` 5× → `config.json` on disk is byte-identical. Convert the same V1 pack in Python and Rust → identical key mappings. `cargo test` passes.

---

## Phase 2 — Memory Leaks & Zombie States

**Goal:** Daemon RSS is flat over 24h. Dead engine = dead process = systemd restarts it.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 2.1 | **B1: Delete UiEvent channel** | `daemon/src/libs/audio/engine.rs:44-109, 514, 527, 548, 582` | Remove `UiEvent` enum, `event_tx`, `UI_EVENT_RX`, `ENGINE_HANDLE`, `engine_handle()`, `ui_event_receiver()`. Remove all `event_tx.send(...)` calls. Remove the `include_str!` test in `log_buffer.rs:545` that references `input_worker_host.rs`. |
| 2.2 | **B7: Engine death → process exit** | `daemon/src/libs/audio/engine.rs:177-184`, `daemon/src/main.rs` | Replace `panic!()` with a channel signal to main thread. Main thread calls `std::process::exit(1)`. systemd `Restart=on-failure` fires. Alternatively: `std::process::abort()` directly in the engine thread (simpler, one line). |
| 2.3 | **B10: Control socket request size limit** | `daemon/src/control.rs:51-66` | Use `read_line` with a `BufReader` wrapped in a length check, or switch to `read_until` with a 64KB cap. Reject with `{"ok":false,"error":"request too large"}`. |
| 2.4 | **B6: Delete dead trace machinery** | `daemon/src/libs/trace.rs`, `daemon/src/utils/log_buffer.rs:78-91` | `trace::init()` is never called. `set_verbose` is unreachable. Delete `trace.rs` entirely (~316 LOC), remove `set_verbose`/`verbose_enabled` from `log_buffer.rs`, remove `SORAKEY_TRACE` env check. (If you want the feature later, it's in git history.) |

**Verify:** Type 10,000 keys → RSS unchanged (was growing ~40B/keystroke). Unplug audio device → daemon exits within 1s → systemd restarts it. `diag` after 24h → same RSS as startup.

---

## Phase 3 — Correctness Bugs

**Goal:** All remaining bugs that cause wrong behavior (no data loss, no leak, but wrong result).

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 3.1 | **B2: Fix acquire_lock** | `daemon/src/main.rs:50-61` | If `XDG_RUNTIME_DIR` is unset, fall back to `$HOME/.sorakey.lock` (same pattern as `control.rs:11-16`). Only report "already running" when the lock is genuinely held by another PID. |
| 3.2 | **B8: Fix switch_device resample rate** | `daemon/src/libs/device_manager.rs:324-337`, `engine.rs:247` | After opening the new device, query *its* sample rate (from the opened stream), not from config. Pass the actual stream rate to the resampler. |
| 3.3 | **B9: Remove rdev fallback** | `daemon/src/libs/bootstrap.rs:11-19`, `daemon/src/libs/input_listener.rs` (entire file) | The fallback is dead code on Linux. Delete the `else` branch. Delete `input_listener.rs` (380+ LOC). Remove `rdev` from `Cargo.toml`. |
| 3.4 | **B11: Fix AlsaErrorSuppressor** | `daemon/src/libs/device_manager.rs:16-46` | Replace `dup2` on stderr with a per-thread `RustStream` redirect, or simply remove the suppressor (ALSA errors go to journal anyway via systemd). Simplest: delete the suppressor, let errors flow to journal. |
| 3.5 | **B12: Fix shell concatenation in Panel.qml** | `Panel.qml:173-178, 184` | Replace `sh -c` with a `ctl` command (`set_bar_section`/`get_bar_section`) handled by the daemon. No shell, no space issues. |
| 3.6 | **B14: Remove no-op config field** | `daemon/src/state/config.rs:20` | Remove `enable_keyboard_sound` from the struct, `data_equals`, and migration table. Or wire it to the engine. Decide: delete or wire. |

**Verify:** `cargo test` passes. Start daemon without `XDG_RUNTIME_DIR` → works. Switch audio device → correct pitch. No more `rdev` in `Cargo.lock`. Panel bar-section save works with `HOME="/home/test user"`.

---

## Phase 4 — Performance (hot path)

**Goal:** <5ms key-to-sound latency. No multi-second stalls on pack switch.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 4.1 | **evdev blocking read (kill 20ms poll)** | `daemon/src/libs/evdev_input_listener.rs:63-138` | Remove `set_nonblocking(true)` + the `sleep(20ms)` loop. Use blocking `fetch_events()` (the thread sleeps in the kernel until an event arrives). For multiple keyboards, one thread per device (max 1-2 keyboards). |
| 4.2 | **Pack load off engine thread** | `daemon/src/libs/audio/engine.rs`, `soundpack_loader.rs` | Spawn a worker thread for decode+resample. Send `PackLoaded { pack_data }` back on completion. Engine swaps state atomically (Arc<RefCell<>> or RwLock). User can keep typing during load (old pack still plays). |
| 4.3 | **Precompute per-keystroke buffers** | `daemon/src/libs/audio/engine.rs:374` | At pack load time, pre-apply the fade to each segment and store the final `Vec<f32>`. At keypress time: zero allocation, just `Sink::try_new` with the precomputed buffer. |
| 4.4 | **Resampler: reduce allocation churn** | `daemon/src/libs/audio/resampler.rs:68-113` | Process in-place where possible. Reuse a pre-allocated buffer across chunks instead of per-chunk `.to_vec()`. |
| 4.5 | **Config: RwLock snapshot** | `daemon/src/state/config_writer.rs:18-24` | Replace `Mutex<AppConfig>` with `RwLock<AppConfig>`. Readers take a read lock + clone only the field they need. Writers take a write lock. |
| 4.6 | **Reduce panel poll frequency** | `Panel.qml:389-415` | `packs` poll: 5s → 30s (packs only change on import/delete). Keep `status` at 5s. Closed panel: 30s → 60s. |
| 4.7 | **Fix build-sorakey.sh hash** | `scripts/build-sorakey.sh:22-23` | Exclude `target/` from the source hash: `find daemon/ -path daemon/target -prune -o -type f -print`. Or hash only `daemon/src/` + `daemon/Cargo.toml`. |

**Verify:** `cargo test` passes. Type fast (10 keys/sec) → audio latency <5ms. Switch to a 24MB pack → no audio gap >100ms. RSS stable under sustained typing. `git status` clean → `build-sorakey.sh` reports "up to date" instantly.

---

## Phase 5 — Dead Code Sweep (~2000+ LOC removed)

**Goal:** Every line in the codebase is either reachable or explicitly marked `#[allow(dead_code)]` with a reason.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 5.1 | **Delete `input_worker_host.rs`** | `daemon/src/libs/input_worker_host.rs` (498 LOC) | Orphaned Windows file. Also delete the `include_str!` test in `log_buffer.rs:545` that references it. |
| 5.2 | **Remove unused deps** | `daemon/Cargo.toml` | Remove `rand`, `env_logger`, `log`, `rdev` (if 3.3 done). Run `cargo build` to confirm no breakage. |
| 5.3 | **Remove UiEvent API** | (covered by Phase 2, step 2.1) | — |
| 5.4 | **Remove DeviceManager dead methods** | `daemon/src/libs/device_manager.rs:76-82, 91-263, 293-354` | Delete `get_output_devices`, `get_input_devices`, `get_input_device_by_id`, `test_output_device`, `test_input_device`, `device_supports_rate`, custom `Clone` impl. Keep only what's used. |
| 5.5 | **Remove `auto_startup`** (or wire in Phase 7) | `daemon/src/utils/auto_startup.rs` | Delete for now. Re-add when auto-start toggle is built (Phase 7). |
| 5.6 | **Remove `paths::soundpacks::is_builtin_soundpack`, `get_soundpacks_dir`** | `daemon/src/state/paths.rs:50, 87` | No callers. |
| 5.7 | **Remove dead log_buffer functions** | `daemon/src/utils/log_buffer.rs:57-75, 213-237` | `export_to_file`, `export_file_name`, `reveal_in_file_manager`, `recent`, `generation` — only tests use them. Delete or mark `#[cfg(test)]`. |
| 5.8 | **Remove trace.rs** | (covered by Phase 2, step 2.4) | — |
| 5.9 | **Remove `SoundpackType`, `SoundpackCount`, empty impls** | `daemon/src/state/soundpack.rs:7-10, 85-87, 123-126` | Scaffolding for a mouse/device model that was cut from the fork. |
| 5.10 | **Remove duplicate logger macros** | `daemon/src/utils/logger.rs` | Delete `debug_print!`/`debug_eprint!` (identical to `always_*`). Keep only `always_print!`/`always_eprint!`. Update all call sites. |
| 5.11 | **Remove dead icon URL code** | `daemon/src/libs/audio/soundpack_loader.rs:147-159`, `utils/soundpack.rs:175-202` | No asset handler exists. Delete. |
| 5.12 | **Remove `__pycache__` from git** | `scripts/__pycache__/`, `admin-scripts/__pycache__/` | `git rm -r --cached`, add `__pycache__/` to `.gitignore`. |
| 5.13 | **Remove `bootstrap.rs` dead branch** | (covered by Phase 3, step 3.3) | — |
| 5.14 | **Remove `SoundpackMetadata` dead fields** | `daemon/src/state/soundpack.rs:104-111` | `can_be_converted`, `last_accessed`, `validation_status` — never read. |

**Verify:** `cargo build --release` → 0 new warnings. `cargo test` passes. `wc -l daemon/src/**/*.rs` drops by ~2000 lines. `grep -r "rand\|env_logger\|rdev" Cargo.toml` → 0 matches.

---

## Phase 6 — UX Fixes (visible to user)

**Goal:** Every button is visible. Every action gives feedback. No confusing states.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 6.1 | **Replace invisible U+FF30 glyphs** | `SearchablePackDropdown.qml:326, 379`, `Panel.qml:838` | Delete button: use `iconText: "󰅝"` (trash) or `text: "✕"`. Delete warning: `iconText: "⚠"`. Uninstall: `iconText: "󰒗"` or `text: "✕"`. Pick glyphs that render in the user's Nerd Font. |
| 6.2 | **Add ctl error feedback** | `Panel.qml:91-96` | Parse the `ok:false` response in `ctlProc.onExited`. Show a `deleteToast`-style banner with the error message. Auto-clear after 5s. |
| 6.3 | **Fix mute switch when stopped** | `Panel.qml:515` | `checked: root.muted` (not `root.running && !root.muted`). When stopped, the switch shows the last known mute state, and is disabled (`enabled: root.running`). |
| 6.4 | **Add hotkey discoverability** | `Panel.qml:422` (tooltip), Settings section | Tooltip: `"Sorakey — Playing\nRight-click: Mute\nCtrl+Alt+M: Global mute"`. Add a "Shortcuts" info row in Settings. |
| 6.5 | **Improve or remove TEST TYPING** | `Panel.qml:774-794` | **Recommend: remove** — the daemon listens system-wide; the box adds no value. If kept: add `text: ""` reset on focus-lost, character count, "Clear" button. |
| 6.6 | **Add "no soundpack" state** | `Panel.qml:70-75` | When `keyboardPack === ""` and `keyboardPacks.length === 0`: statusText = `"No soundpack"`, disable slider, show "Import pack to get started" hint. |
| 6.7 | **Add per-pack volume reset** | `Panel.qml` (near slider), `daemon/src/control.rs` | Add a small "↺" button next to the slider that sends `ctl {cmd:"reset_volume", id: keyboardPack}`. Daemon restores `recommended_volume`. |
| 6.8 | **Fix bar-section persistence** | `Panel.qml:169-191` | Replace `sh -c` with a `ctl` command (`set_bar_section`/`get_bar_section`) handled by the daemon. No shell, no space issues. |
| 6.9 | **Make update flow robust** | `Panel.qml:216-219` | Instead of substring matching, check `exitCode === 0` → success, `!== 0` → show full stderr. Add a "Reinstall" option for broken states. |
| 6.10 | **Make Stop sticky** | `Service.qml:147-153` | Only auto-start if the user hasn't explicitly stopped it. Write a `~/.local/share/sorakey/stopped` flag on Stop; check it in `Component.onCompleted` before starting. Remove flag on Start. |
| 6.11 | **Extend status string timeout** | `Service.qml:65` | 4s → 10s. Or: don't auto-clear errors (only clear on next action). |
| 6.12 | **Remove duplicate Update button** | `Panel.qml:822-831` | Remove from bottom row (keep in Settings). Or remove from Settings. **Recommend: keep in bottom row only** (more visible). |
| 6.13 | **aarch64 CI build** | `.github/workflows/release.yml:26-30` | Add `matrix: [x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu]`. Upload both binaries + SHA256SUMS. |

**Verify:** Open panel → all buttons visible. Delete a pack → confirmation shows visible icon. Stop daemon → mute switch is disabled, not "off". Right-click tooltip shows hotkey. No soundpack → clear status message. `git diff` shows no `sh -c` in Panel.qml.

---

## Phase 7 — New Features

**Goal:** Ship the features users expect from a keyboard sound plugin.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 7.1 | **Output device selector** | `control.rs` (new `audio_devices` + `select_device` cmds), `Panel.qml` (Settings dropdown) | `ctl {cmd:"audio_devices"}` → list from `cpal`. `ctl {cmd:"select_device", id:"..."}` → `SwitchDevice`. Fix B8 first (Phase 3). UI: Dropdown in Settings, persists in config. |
| 7.2 | **Auto-start toggle** | `control.rs` (new `set_autostart` cmd), `Panel.qml` (Settings checkbox), re-add `auto_startup.rs` | `ctl {cmd:"set_autostart", enabled:true}` → `systemctl --user enable sorakey`. UI: ToggleSwitch in Settings. |
| 7.3 | **In-panel log viewer** | `Panel.qml` (Settings section), `control.rs` (`recent_logs` cmd) | `ctl {cmd:"recent_logs", count:50}` → last 50 lines. Display in a read-only `TextArea` in Settings, scrollable. "Copy" button. |
| 7.4 | **random_pitch** | `daemon/src/libs/audio/engine.rs`, re-add `rand` dep | At pack load, read `random_pitch` from pack options. At keypress, apply a random pitch multiplier (e.g. 0.95-1.05) to the precomputed buffer. Requires `rand` dep (re-add in Phase 7, removed in Phase 5). |
| 7.5 | **Diagnostics/health surface** | `Panel.qml` (Settings), `control.rs` (`diag` already exists) | Show in Settings: daemon PID, uptime, RSS, input device count, last key event time, active pack, output device. Red indicator if no key events in 30s while "Playing". |
| 7.6 | **Pack metadata display** | `Panel.qml` (below dropdown), `Model.js` | Show pack name, author, description from `SoundpackCache` metadata (already scraped, just not displayed). |
| 7.7 | **Hotkey configuration** | `control.rs` (`set_hotkey` cmd), `evdev_input_listener.rs`, config | Make Ctrl+Alt+M configurable. Store in `AppConfig`. Default to Ctrl+Alt+M. Allow disabling. |
| 7.8 | **Fix uninstall** | `scripts/uninstall.sh` | Add `rm -rf ~/.cache/sorakey ~/.local/lib/sorakey ~/.local/share/sorakey/bar-section`. Match `CLEANUP.md` checklist. |
| 7.9 | **Pack cache rescan trigger** | `control.rs` (`rescan_packs` cmd), `Panel.qml` ("Open folder" also triggers rescan) | `ctl {cmd:"rescan_packs"}` → `SoundpackCache::refresh_from_directory()`. Panel calls it after import/delete/manual folder drop. |

**Verify:** Settings shows output device dropdown (switch works, correct pitch). Auto-start toggle persists across reboot. Log viewer shows last 50 lines. Random pitch audible variation. Diag shows "Last key: 2s ago" updating. Uninstall removes all files.

---

## Phase 8 — Code Organization & Security

**Goal:** One table, one converter, no side-effects on reads, no shell injection.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 8.1 | **Consolidate key-mapping tables** | `evdev_input_listener.rs:142-204`, `config_converter.rs:736-938` | After Phase 1 fixes, the evdev listener and converter can share one `KeyMap` struct (generated from a single source, e.g. a `.rs` const or a build script). The rdev table is already deleted (Phase 3). |
| 8.2 | **Consolidate V1→V2 converters** | `config_converter.rs`, `sorakey-import-pack.py`, `admin-scripts/v1-to-v2-converter.py` | Delete `admin-scripts/v1-to-v2-converter.py` (one-off, done). Keep Python importer + Rust converter but ensure they use the same table (Phase 1). Add a cross-test: convert a sample V1 pack in both, assert identical output. |
| 8.3 | **Separate read/write in soundpack loading** | `daemon/src/utils/soundpack.rs` | `load_soundpack_metadata` becomes pure read (parse JSON, return metadata). V1→V2 conversion moves to an explicit `convert_pack_in_place` function, called only from the import path, never from cache refresh. |
| 8.4 | **Fix bootstrap.rs comment** | (file deleted in Phase 3 if rdev removed) | — |
| 8.5 | **Move process docs out of plugin dir** | `plan.md`, `relse.md`, `note.md`, `CLEANUP.md` | Move to `docs/` subdirectory (not synced to user `~/.config` by `dev-sync.sh`). Or delete `relse.md`/`note.md` if obsolete. |
| 8.6 | **Remove hardcoded builtin soundpacks list** | `daemon/src/state/paths.rs:37-48` | Delete the constant. The directory is the source of truth. |
| 8.7 | **Auto-sync version** | `manifest.json`, `daemon/Cargo.toml`, `build-sorakey.sh` | Add a test: `cargo test version_sync` that asserts both files match. |
| 8.8 | **Security: PID-based kill** | `Service.qml:178` | Replace `pkill -x sorakey` with reading the PID from `~/.local/share/sorakey/sorakey.pid` (daemon writes it on start) and `kill $PID`. |
| 8.9 | **Security: Mask username in evdev log** | `daemon/src/libs/evdev_input_listener.rs:17` | Remove the `Current user: {:?}` log line, or mask it: `user: [masked]`. |
| 8.10 | **Security: systemd StartLimit** | `scripts/sorakey-setup` (unit file) | Add `StartLimitIntervalSec=30` + `StartLimitBurst=5` to the unit. After 5 crashes in 30s, stop retrying. |
| 8.11 | **Fix .gitignore** | `.gitignore` | Add `__pycache__/`, `*.pyc`, `daemon/target/`. |

**Verify:** `grep -r "FF30\|sh -c.*home" Panel.qml` → 0. `ls ~/.config/omarchy/plugins/io.github.sandeshrai00.sorakey/` → no `.md` files. `systemctl --user status sorakey` → shows StartLimit. Kill -9 the daemon 6× in 30s → systemd stops retrying.

---

## Phase 9 — Accessibility

**Goal:** Screen reader users can navigate. All state is non-color. Icons have fallbacks.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 9.1 | **Add Accessible roles to custom delegates** | `SearchablePackDropdown.qml:286-355` | Add `Accessible.role: Accessible.ListItem` to rows, `Accessible.role: Accessible.Button` + `Accessible.name: "Delete " + packName` to delete buttons, `onAccessiblePress` handlers. |
| 9.2 | **Add Accessible descriptions to bar button** | `Panel.qml:422-439` | `Accessible.name: "Sorakey keyboard sounds"`, `Accessible.description: "Left click: open panel. Right click: toggle mute. Scroll: adjust volume."` |
| 9.3 | **Visible open/closed state on dropdown trigger** | `SearchablePackDropdown.qml` | Add a chevron icon that rotates (▲/▼) based on open state. Non-color indicator. |
| 9.4 | **Non-color state for mute** | `Panel.qml:422-423` | Muted state: add a small "M" badge or strikethrough on the icon, not just `active: true`. |
| 9.5 | **Font fallbacks** | `Panel.qml` (global), `SearchablePackDropdown.qml` | Add `fontFallbacks: ["Noto Sans", "DejaVu Sans", "system-ui"]` to all Text items. Prevents tofu if Nerd Font is missing. |
| 9.6 | **Accessible.name on TEST TYPING** | (moot if removed in Phase 6.5) | If kept: `Accessible.name: "Test typing — type here to hear keyboard sounds"`. |
| 9.7 | **Delete/red contrast** | `SearchablePackDropdown.qml:380, 402` | Use `Style.colors.danger` (theme-aware) instead of hardcoded `#ff6b6b`. |

**Verify:** Orca screen reader: navigate panel → announces "Sorakey keyboard sounds, button". Tab to delete → "Delete sankey-mx-brown, button". Mute state announced. All icons render with DejaVu Sans fallback (no tofu).

---

## Phase 10 — Final Hardening & Release

**Goal:** Ship-ready. All tests green. No warnings. Clean CI.

| # | Fix | Detail |
|---|-----|--------|
| 10.1 | **`cargo clippy --all-targets -- -D warnings`** | Fix all clippy lints. |
| 10.2 | **`cargo test`** | All tests pass. Add tests for: V1 conversion determinism, multi-pack non-destruction, socket size limit, engine death → exit. |
| 10.3 | **Update README.md** | Document: hotkey, output device selector, auto-start, log viewer, diagnostics. Update uninstall instructions. |
| 10.4 | **Version bump** | `manifest.json` + `Cargo.toml` → 0.2.0 (breaking: config format may change with Phase 1/3 fixes). |
| 10.5 | **Release** | Tag, CI builds x86_64 + aarch64, uploads binary + SHA256SUMS. |

---

## Dependency Graph (what must come before what)

```
Phase 1 (data safety) ─────────────────────────────────────────┐
Phase 2 (leaks/zombies) ───────────────────────────────────────┤
Phase 3 (correctness) ── depends on Phase 1 (B5 fix first) │
Phase 4 (performance) ── depends on Phase 2 (UiEvent removed) │
Phase 5 (dead code) ── depends on Phases 2,3 (items removed) │
Phase 6 (UX) ── independent, can parallel with 4-5 │
Phase 7 (features) ── depends on Phase 3 (B8 fix for device)│
Phase 8 (org/security)── depends on Phases 1,3,5 (consolidate) │
Phase 9 (a11y) ── depends on Phase 6 (icons fixed) │
Phase 10 (release) ── depends on ALL above │
```

**Parallelizable:** Phases 6 and 7 can run in parallel with 4-5 (different files). Phase 9 can start once 6.1 (icons) is done.

---

## Estimation

| Phase | Estimated effort | Risk |
|-------|-----------------|------|
| 1 — Data Safety | 2-3 h | Low (deletion + dedup) |
| 2 — Leaks/Zombies | 2-3 h | Medium (UiEvent removal touches engine) |
| 3 — Correctness | 3-4 h | Medium (XDG fix, device rate) |
| 4 — Performance | 4-6 h | High (evdev blocking, pack load thread) |
| 5 — Dead Code | 1-2 h | Low (pure deletion) |
| 6 — UX | 3-4 h | Low (QML only) |
| 7 — Features | 6-8 h | Medium (new ctl commands + UI) |
| 8 — Org/Security | 2-3 h | Low |
| 9 — Accessibility | 2-3 h | Low |
| 10 — Release | 1-2 h | Low |
| **Total** | **~26-38 h** | |

---

## How to Execute

For each phase:
1. **Read** the files listed in the "Files" column for every fix in that phase
2. **Make changes** to fix each item
3. **Build**: `cd daemon && cargo build --release`
4. **Test**: `cargo test` — all tests must pass
5. **Deploy**: `cp target/release/sorakey ~/.local/bin/sorakey && systemctl --user restart sorakey`
6. **Verify** against the "Verify" block at the end of the phase
7. **Deploy panel**: `./admin-scripts/dev-sync.sh`
8. **Move to next phase**
