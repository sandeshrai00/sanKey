# Sorakey Codebase Audit

Full audit of the sorakey plugin — daemon (Rust), panel (QML), scripts, and CI.
Generated: 2026-09-01 · **Every finding verified against code 2026-09-01** (read + executed where possible).

Legend: `[V]` verified as described · `[C]` verified, with correction noted · `[YAGNI]` recommend deferring

---

## Architecture Overview

- **Daemon** (`daemon/`, Rust 1.87, edition 2024): systemd user service. `main.rs` acquires a flock, spawns an audio engine thread (rodio/cpal), a Unix-socket control server (`control.rs`, one JSON line in/out), and an evdev input listener. State lives in `state/` (config behind a `OnceLock<Mutex>` with a single-writer `apply()` API, atomic file writes), utils in `utils/` (symphonia decode, rubato resample, V1→V2 conversion, log ring buffer, trace).
- **Plugin** (root QML/JS): `Panel.qml` (bar icon + panel, polls `sorakey ctl status`/`packs` via forked `Process`), `Service.qml` (shell-side lifecycle: enable/start daemon, freshness rebuild, import/export helpers), `Model.js` (parse helpers), `SearchablePackDropdown.qml` (searchable list + inline delete).
- **Scripts**: `build-sorakey.sh` (verified-prebuilt-or-source), `sorakey-setup` (installer + unit), `sorakey-import-pack.py` (GTK4 file dialog + zip extraction + V1→V2), `sorakey-export-logs.py`, `uninstall.sh`, `_v1_shared.py` (shared V1 keycode tables).

The codebase has already been through one optimization pass (git log: "ram leak fix", "remove dead and clean up code"). Findings below are the **current** state.

---

## 1. Bugs

### B1. Unbounded memory leak: `UiEvent` channel never drained (every keystroke leaks) `[V]`

`daemon/src/libs/audio/engine.rs:88` — for *every* key event the engine does `event_tx.send(UiEvent::KeyDown/KeyUp)` (line 582, in the hot path). The receiver (`UI_EVENT_RX`, line 72) is stored in a `OnceLock` and the only accessor, `ui_event_receiver()` (line 107), **has zero callers** (verified by grep). Crossbeam `unbounded` + never-read = one leaked allocation per keystroke, forever. Lines 514, 527, 548 leak command events too (bounded in practice). `engine_handle()` (line 102) also has zero callers.

**Scope note `[C]`:** the `AudioEngineHandle` *struct* is live (returned by `spawn_engine`, cloned per control connection, threaded into `control::serve`) — only the `UiEvent` enum, `event_tx`/`event_rx`, both `OnceLock` statics, both accessor functions, and the four `send()` sites are dead.

**Fix:** delete the dead plumbing (see Phase 2.1).

### B2. Missing `XDG_RUNTIME_DIR` → daemon exits with misleading "already running" `[V]`

`daemon/src/main.rs:50-61` — `acquire_lock()` returns `None` both when the lock is held *and* when `XDG_RUNTIME_DIR` is unset (line 51 `.ok()?`) or the lock file can't be opened (line 53 `.ok()?`), and `main.rs:22-28` treats all three as "already running" and exits 1. Meanwhile `control.rs` falls back to `$HOME/.sorakey.sock` when the var is missing. Inconsistent: on such systems the daemon can never start, and the error message is wrong.

### B3. V1 keycode tables diverge between the Python importer and the Rust in-daemon converter `[V]`

`scripts/_v1_shared.py` (used by panel import) vs `daemon/src/utils/config_converter.rs:736-938`. For the same V1 pack, the two conversion paths produce *different* key→sound mappings (Python dict executed to verify: 163 entries, 159 unique keys):

| V1 code | Python final value | Rust final value |
|---------|-------------------|-----------------|
| 3597 | ControlRight | NumLock (`:895` overwrites `:850`) |
| 3612 | NumpadEnter | NumpadDivide (`:896` overwrites `:849`) |
| 3613 | ControlRight | NumpadMultiply (`:897`) |
| 3640 | AltRight | Numpad8 (`:899`) |
| 3675 | NumpadDecimal | Numpad3 (`:907`) |
| 3676 | Numpad0 | NumpadEnter (`:908`) |
| 3677 | (absent) | Numpad0 (`:909`) |

**Scope note `[C]`:** the Rust "Alternative range" block (`:895-909`) is *mostly useful* — 3639–3667 map numpad keys (7,8,9,4,5,6,1,2) that the main block lacks. The fix is to resolve the 4 conflicting entries, not delete the block.

### B4. Silent duplicate keys in Python `V1_KEY_TABLE` `[V]`

`scripts/_v1_shared.py:51-55` vs `:65-69` — keys `3597`, `3612`, `3613`, `3640` each appear twice; Python silently keeps the last. Same class of bug in the Rust `HashMap` ("Alternative range" block overwrites the main block). Nobody can see this; a test asserting no duplicate keys would catch it.

### B5. Read-path destroys user data: multi-method packs are permanently downgraded on cache refresh `[V]`

`daemon/src/utils/soundpack.rs:76-102` — during a *metadata scan* (`load_soundpack_metadata`), any V2 pack with `definition_method: "multi"` is run through `convert_v2_multi_to_single` which **overwrites `config.json` on disk** (`config_converter.rs:475`). In that conversion, keys using a different audio file than the "most used" one are **dropped entirely** (`config_converter.rs:443-448`). The engine explicitly *supports* multi packs (`soundpack_loader.rs:236-295`), and the panel importer deliberately produces multi configs (`sorakey-import-pack.py:231`). So: import a multi-pack → next rescan silently corrupts it. A read command with destructive side effects is the worst kind.

**Trigger correction `[C]`:** `diag` does *not* rescan — `control.rs:315` calls `SoundpackCache::load()`, which only calls `refresh_from_directory()` when the cache is **empty** (`state/soundpack.rs:151-155`). Actual corruption triggers: (1) startup with empty cache, (2) first `delete_pack` (`control.rs:224`), (3) any pack load that writes a new cache entry (`soundpack_loader.rs:336, 393`).

### B6. `trace::init()` is never called — `SORAKEY_TRACE=1` is dead `[V]`

`daemon/src/libs/trace.rs:151` (`init()`, documented "Call once early in main") has no callers; `main.rs` never calls it. Consequently `log_buffer::set_verbose` (`log_buffer.rs:83`, the only other enabler) is also unreachable — no `ctl` command, no panel control. The entire verbose/latency-tracing feature is inert at runtime; only tests can reach it.

**Scope correction `[C]`:** `trace::record`/`trace::time` **are** called from live runtime code (`engine.rs:505, 522, 577-581`) — the feature is a permanent no-op (enable path dead), so the fix is to delete the enable machinery *and* the call sites, or keep them as a compile-time no-op stub. You cannot "delete trace.rs" without also touching the engine.

### B7. Engine start-failure crashes with no clean signal `[C] — corrected from "zombie daemon"`

`daemon/src/libs/audio/engine.rs:177-184` — if the selected *and* default audio device fail, the engine thread `panic!()`s in `EngineState::new` (runs on the spawned thread, `run_engine` line 538).

**Correction:** the original audit claimed the process keeps living and `status` keeps reporting healthy. That is **wrong for Rust**: a panic on a non-main thread **aborts the whole process** (non-main-thread panics never unwind, no `catch_unwind` here). So the process *does* die, systemd `Restart=on-failure` *does* fire, and the default start rate-limiting (5 restarts / 10 s) stops the loop in a `failed` state. There is no zombie.

What remains (mild): the only error signal is the panic message on stderr/journal; there's no clean "no audio device — check configuration" line, and the failure mode is a crash-loop rather than a quiet exit. If both devices fail on every restart, the unit ends `failed`, which is correct behavior.

**Fix:** optional polish — replace `panic!()` with a clear `always_eprint!` + `std::process::exit(1)`. No functional change needed.

### B8. `switch_device` resamples to the wrong rate `[V]`

`daemon/src/libs/device_manager.rs:324-337` — `get_current_output_sample_rate()` resolves the device from **config** (`config.selected_audio_device`), not from the stream actually being opened. `engine.rs:247` uses it in `switch_device` *after* opening a different device (config not yet updated), so cached samples get resampled to the old device's rate (wrong pitch/speed). Verified unreachable today: no `ctl` command and no QML sender for `SwitchDevice` exists anywhere — the capability is dead, and buggy.

### B9. `bootstrap.rs` fallback starts both listeners (the exact thing its comment forbids) `[V]`

`daemon/src/libs/bootstrap.rs:11-19` — comment (line 6) says "rdev would double-fire", yet the `else` branch starts evdev *and* rdev. On Linux rdev's backend is also evdev, so if `evdev::enumerate()` saw no keyboard, rdev won't either — the fallback is dead code, and if it ever did work it would double-play. Verified: `input_listener.rs` is referenced **only** by `bootstrap.rs`, so deleting the else branch lets the whole file (218 LOC) and the `rdev` dependency go.

### B10. No request-size limit on the control socket `[V]`

`daemon/src/control.rs:51-66` — `read_line` into an unbounded `String`. The socket is 0600 (same user only) so risk is low, but a stuck local client can make the daemon OOM.

### B11. `AlsaErrorSuppressor` swaps process-wide stderr `[V]`

`daemon/src/libs/device_manager.rs:16-46` — `dup2` on `STDERR_FILENO` is not thread-safe: while a device enumeration runs, `eprintln!` from any other thread (the engine logs constantly) is silently eaten or interleaves with the fd swap. Instantiated only in the dead `test_*_device` paths, so the practical blast radius today is nil — but the pattern is wrong.

### B12. Shell-concatenation in panel breaks for `$HOME` with spaces `[V]`

`Panel.qml:173-178` — `"/bin/sh", "-c", "mkdir -p " + root.home + "/..."` with no quoting. Any user whose home path contains a space gets a broken bar-section save (and a latent injection footgun). Same pattern at `Panel.qml:184` (read).

### B13. `rdev` fallback listener has per-keystroke locks and drops fast keystrokes `[V]` (dead path, dies with B9)

`daemon/src/libs/input_listener.rs:176-186` — 1ms/10ms rate-limiting plus `Arc<Mutex>` traffic inside the event callback. Dead on healthy systems (B9); the whole file is deleted in Phase 3.3.

### B14. Config `enable_keyboard_sound` is a no-op `[V]`

`daemon/src/state/config.rs:20` — stored, compared in `data_equals` (`:119`), defaulted (`:274`), but never read by anything that produces sound (only `enable_sound` gates playback, `engine.rs:203, 207-210`). Verified no panel/README/UI reference. The only other reference is one thread in a config_writer concurrency test (`config_writer.rs:257`) — a fix must update that test.

### B15. V1 multi-conversion concatenation order is unstable for press/release pairs `[V]`

`daemon/src/utils/config_converter.rs:92-94` — `audio_files_ordered` sorts by numeric code; a press and its release with the same numeric code (e.g. `"30"`/`"030"`) tie, and stable `sort_by_key` over `HashMap` iteration order is arbitrary. If press and release map to different files, the concatenated audio order (and thus timings) is random per run. Note: the *definitions*-order sort at `:188-192` already does `(press-before-release)` correctly — the bug is an inconsistency, not a blind spot.

### B16. Symphonia decode swallows errors `[V]`

`daemon/src/utils/symphonia.rs:44-54` — `next_packet()`/`decode()` errors → `break`/`continue` with no log. A partially-decodable file yields truncated audio with the segment timings pointing past the buffer (then the "start sample past end" spam in `engine.rs:341-349`).

---

## 2. Dead Code (verified by grep)

| Item | Location | Note |
|------|----------|------|
| `input_worker_host.rs` (498 LOC) | `daemon/src/libs/input_worker_host.rs` | **Orphaned file**: not declared in `libs/mod.rs`, Windows-only, references a nonexistent `crate::libs::input_worker` module. Only reachable via `include_str!` in `log_buffer.rs:545` tests. |
| `rand` dependency | `daemon/Cargo.toml:19` | Zero uses. (`random_pitch` is parsed in `state/soundpack.rs:25` but never implemented.) |
| `env_logger` + transitive `log` | `Cargo.toml:12`, `main.rs:19` | `env_logger::init()` called, but no `log::` macro anywhere. Init is a no-op. |
| `UiEvent`, `engine_handle()`, `ui_event_receiver()`, `ENGINE_HANDLE`, `UI_EVENT_RX` | `engine.rs:44-109, 514, 527, 548, 582` | See B1. Keep the `AudioEngineHandle` struct (live). |
| DeviceManager: `get_output_devices`, `get_input_devices`, `get_input_device_by_id`, `test_output_device`, `test_input_device`, `device_supports_rate`, custom `Clone` | `device_manager.rs:76-82, 91-263, 293-354` | No callers outside the module. `get_input_device_by_id` is called only by `test_input_device` — the whole cluster dies together. No `DeviceManager` `.clone()` call exists, so the custom `Clone` (re-enumerates per clone) is dead too. |
| `auto_startup::set_auto_startup` | `utils/auto_startup.rs:4-15` | No `ctl` command sets `auto_start`; unreachable. (`get_auto_startup_state` IS called at config load, `config.rs:201` — keep that one.) |
| `paths::soundpacks::is_builtin_soundpack`, `get_soundpacks_dir`, `BUILTIN_SOUNDPACKS` | `state/paths.rs:37-52, 87` | No callers. The constant exists only to serve `is_builtin_soundpack`. |
| `log_buffer::export_to_file`, `export_file_name`, `reveal_in_file_manager` (runtime) | `utils/log_buffer.rs:57-75, 213-237` | No runtime callers. `recent()`/`generation()` are used by `logger.rs`'s own tests — those go under `#[cfg(test)]`, not deletion. |
| `trace::init`, `trace::now_ms` (external), `trace::enabled` (external) | `libs/trace.rs` | See B6. `record`/`time` have live callers — see B6 scope note. |
| `log_buffer::set_verbose` / `verbose_enabled` (external) / `push_verbose` | `log_buffer.rs:78-99` | See B6. `push_verbose`'s only production caller is the trace writer (dies with it). |
| `bootstrap.rs` else-branch | `libs/bootstrap.rs:12-19` | See B9. |
| `impl SoundPack {}` / `impl SoundpackType {}` | `state/soundpack.rs:85-87` | Empty impl blocks. |
| `SoundpackType` single-variant enum, `SoundpackCount` single-field struct, `default_soundpack_type()` | `state/soundpack.rs:7-18, 123-126` | Scaffolding for a mouse/device model that was cut from the fork. |
| `debug_print!`/`debug_eprint!` | `utils/logger.rs:11-32` | `is_debug_enabled()` is hardcoded `true` — byte-for-byte identical to the `always_*` pair. Call sites to migrate: `control.rs:43`, `config.rs:143, 259-260`, `bootstrap.rs:9`. Test at `log_buffer.rs:550` asserts on the macro names — update the list. |
| Icon "dynamic asset URLs" | `libs/audio/soundpack_loader.rs:147-159`, `utils/soundpack.rs:175-202` | Emits `/soundpack-images/{id}/{icon}` URLs "served by the asset handler" — no HTTP/asset handler exists. Keep the `icon` *field* (cache JSON contract); emit the raw filename or `None`. |
| `SoundpackMetadata.can_be_converted`, `last_accessed` | `state/soundpack.rs:103-108` | Written, never read. **`validation_status` must stay** — `soundpack_loader.rs:355` writes `"loading_error"` into it; it's the panel's only per-pack error channel. `last_error` stays too. |

---

## 3. Performance Issues

1. **evdev 20 ms busy-poll** `[V]` — `evdev_input_listener.rs:44, 63-138`: non-blocking `fetch_events()` on every keyboard, then `thread::sleep(20ms)`. Up to 20 ms input latency, ~50 wakeups/sec at idle, no eventfd/epoll wait. The hot path for a keystroke-sound app. evdev 0.13.2 supports blocking reads and `AsRawFd` (crate docs recommend epoll) — the fix is implementable.

2. **Per-keystroke allocations** `[V]` — `engine.rs:374-376` `.to_vec()` of the segment slice + `apply_fade` + `SamplesBuffer::new` + `Sink::try_new` per key. The fade depends only on (segment, channels, sample_rate) — static per (pack, key, device rate) — so precomputing at load is correct. `Sink::try_new` stays per key (sinks can't be precomputed).

3. **Keystroke → leaked `UiEvent`** — B1.

4. **Pack load blocks the engine thread** `[V]` — `engine.rs:569` dispatches `LoadKeyboardPack` inside the engine's `select!` loop; decode (symphonia) + resample (sinc) run inline, keystroke events sit unprocessed in `keyboard_rx` for the whole load. ~2.5 s silence on a 24 MB pack. Note: `EngineState` is thread-owned plain fields *on purpose* (comment `engine.rs:111-115`) — the fix is a real restructure, the riskiest item in Phase 4.

5. **Resampler allocation churn** `[V]` — `resampler.rs:19-24, 68-79, 108-113`: per-chunk (1024-frame) × per-channel `.to_vec()` + zero-pad, then re-interleave. Runs at **pack-load time only**, never per keystroke — a load-time cost, subsumed by item 4's worker thread. Defer behind it.

6. **`current()` deep-clones the whole `AppConfig`** `[C]` — `config_writer.rs:18-24` clones on every call, and `control.rs:86` + `device_manager.rs:325` call it per request — true. **But** the per-keystroke path never reads config (the engine caches `volume`/`sound_enabled` in `EngineState`), so `current()` only runs at ctl-request time, and it's cloning a 7-field struct with a small `HashMap` — nanoseconds. `RwLock` is more code and a second lock type for a cost no profiler will show. **Skipped as a fix.**

7. **`SoundpackCache::load()` does disk read + possible full rescan** (and destructive conversion, B5) — `control.rs:315`. A read command with filesystem side effects. Fixed by B5's fix.

8. **Panel forks 2 processes every 5 s while open** `[V]` — `Panel.qml:389-397`: `ctl status` + `ctl packs` every poll; plus 30 s closed poll (`:399-404`) and 5 s not-installed poll (`:407-415`). `packs` rarely changes. The fork is cheap (one Unix-socket round trip), so this is polish, not a fix.

9. **`build-sorakey.sh` freshness check runs at every shell start** `[C]` — `Service.qml:156-170` runs the build script at every shell start; the source hash (`build-sorakey.sh:22-23`) has no `-prune`, so a `daemon/target/` (created by the normal dev build) contaminates the hash and kills the "up to date" short-circuit — **that part is a real bug**, one-line fix. The "non-git installed plugin → prebuilt path dead, always a cargo build" part is **by design**: `release_matches_source` (`:31-38`) deliberately requires git to trust a prebuilt against a tag — that's the security property, not a bug.

10. **`DeviceManager` clones a fresh `Host` per clone** — dead code (see dead code table), dies with the cluster.

---

## 4. UX Problems (Panel/Service)

1. **Invisible icon glyphs (U+FF30, zero-width filler)** `[V] — hex-verified` — `SearchablePackDropdown.qml:326` and `:379`, `Panel.qml:838` all contain `EF BD B0` (U+FF30). The delete button, delete-warning icon, and uninstall button render nothing. Look like mangled Nerd Font glyphs. **Fix note:** prefer emoji/text glyphs (`⌫`, `✕`, `⚠`) over PUA Nerd Font glyphs — PUA renders only if the user's exact font is present, and no `fontFallbacks` list can fix it (see Phase 9.5).

2. **Failed `ctl` commands give no feedback** `[V]` — `Panel.qml:91-96` (`sendCtl`) silently drops queued commands if `ctlProc.running`, and the single shared `ctlProc` only interprets `status` responses; error responses for `keyboard_pack`/`volume`/`mute`/`per_pack_volume` are never surfaced. **Implementation note:** because there's one shared process, the fix must track the pending request (closure variable) to correlate the response.

3. **Mute switch is misleading when the daemon is stopped** `[C]` — `Panel.qml:515` `checked: root.running && !root.muted`: stopping the daemon flips the switch to **off** (reads as "unmuted"), not "off as if muted" — the original direction was misdescribed; the user-visible annoyance is the same. Toggling while stopped sends a ctl that silently drops. Fix unchanged: `checked: root.muted`, `enabled: root.running`.

4. **Right-click mute and Ctrl+Alt+M are undiscoverable** `[V]` — the tooltip (`Panel.qml:424`) only shows status; the hotkey exists only in the daemon's startup banner (`main.rs:44`).

5. **"TEST TYPING" box is a weak feature** `[V]` — `Panel.qml:774-794`: a plain `TextField` with no key feedback; it does nothing beyond "type here". **Recommend: remove** — the daemon listens system-wide; the box adds no value.

6. **Delete-all leaves an unexplained silent state** `[V]` — deleting the last pack sets `keyboard_pack: ""`; the engine goes quiet but `statusText` (`Panel.qml:70-75`) has no "no soundpack" branch, so status still says "Playing".

7. **Per-pack volume can't be reset** `[V]` — the daemon tracks `recommended_volume` (`control.rs:131-136`) but no `reset_volume` cmd exists and there's no "reset" control.

8. **Bar-section save uses a shell one-liner** `[V]` — `Panel.qml:169-191`, same code as B12. **Same fix as Phase 3.5 — implement once, in whichever phase runs first.**

9. **Update flow parses free-form CLI output** `[V]` — `Panel.qml:216-219` matches `"is up to date"`/`"Updated"` substrings in `omarchy plugin update` output — fragile to wording changes; errors show only the last stderr line.

10. **`Stop` is not sticky** `[V]` — `Service.qml:147-153` re-enables/starts the daemon on every shell start, silently undoing the user's Stop.

11. **Import/export status strings clear after 4 s** `[V]` — `Service.qml:65`. An error the user misses is gone.

12. **Duplicate "Update" button** `[V]` — bottom row (`Panel.qml:822-831`) and Settings (`:595-604`) are two copies of the same control; the bottom-row spacer math (`:833`) is manual width arithmetic.

13. **Nerd Font dependency** `[V]` — bar icon `󰌌` and all `iconText` glyphs are PUA codepoints; without a Nerd Font, the bar shows tofu with no fallback.

14. **aarch64 has no prebuilt** `[V]` — CI builds x86_64 only (`release.yml:26-32`), while `build-sorakey.sh:16-17` anticipates `sorakey-aarch64` → every ARM user compiles from source.

---

## 5. Missing Features (expected in a keyboard-sound plugin)

1. **Output device selection** `[V]` — the engine fully supports it (`AudioCommand::SwitchDevice`, `engine.rs:236-283`; `DeviceManager::get_output_devices` exists) but there is no `ctl` command and no UI. Dead, and buggy (B8) — fix B8 first. **Highest-value feature.**
2. **Auto-start toggle** `[V]` — `auto_startup.rs` implements enable/disable via `systemctl`; config `auto_start` is synced on load (`config.rs:200-210`) but can never be set by the user.
3. **Log viewer** `[V]` — `log_buffer::recent()` already exists; a `recent_logs` cmd + read-only `TextArea` is it. Cheapest feature here.
4. **`random_pitch`** `[YAGNI]` — parsed (`state/soundpack.rs:24-25`) and written by the converter, never applied. Honest fix: **delete the dead field** (and the `rand` dep). Implement only if a real pack in the wild sets it.
5. **Diagnostics/health surface** `[V]` — `diag` (memory, cache size, pack) exists in `control.rs:311-327` but nothing displays it. **Scope note:** the engine stores **no last-key-event timestamp** — the "Last key: 2s ago / red if quiet 30s" indicator requires *adding* a field bumped in `handle_key_event`, not just displaying `diag`. This is the only thing that catches the #1 failure mode (missing `input` group → silently no sounds while the panel says "Playing").
6. **No pack metadata display** `[C]` — name/author/description are in the `SoundpackCache`, **but** the `packs()` ctl handler (`control.rs:271-276`) returns **IDs only** via a directory scan (`collect_packs`) — it never reads the cache. Displaying metadata needs a metadata-returning ctl command (pulling from the cache, i.e. depends on the B5 read-path fix). Not "just display it".
7. **No hotkey configuration/disable** `[YAGNI]` — Ctrl+Alt+M is hardcoded in the evdev listener (the rdev copy dies in Phase 3). For a single global mute hotkey, no user needs to rebind it. Leave as-is; add config only if asked.
8. **No per-key tuning** (per-key volume/mute), no key-press preview.
9. **No way to trigger a pack-cache rescan** after manual file drops into the soundpacks dir (the panel's "Open folder" invites exactly that).
10. **Uninstall gaps** `[V]` — `scripts/uninstall.sh` leaves `~/.cache/sorakey` (build tree), `~/.local/lib/sorakey` (source hash), and (non-purge) `bar-section` behind. Three lines to add.

---

## 6. Code Organization Issues

1. **Three parallel key-mapping tables for the same W3C names** `[V]` — `input_listener.rs` (rdev, dies in Phase 3), `evdev_input_listener.rs:143-204` (evdev), `config_converter.rs:736-938` (IOHook). Divergence already happened (B3). Post-Phase 3 there are two; minimum fix = one shared Rust `static` table, not a code-generation build script.
2. **Three parallel V1→V2 converters** `[V]` — Rust `config_converter.rs`, `scripts/sorakey-import-pack.py`, `admin-scripts/v1-to-v2-converter.py`. The two Python copies differ (case-insensitive file matching in the importer, not the admin script; different shared-file detection). **Bonus finding (missed by original audit):** the admin converter imports `SMART_DONOR` from `_v1_shared` (line 20) then **redefines the entire dict at line 75**, shadowing the import — a dead import plus an 80-line duplicated table (B4-class bug).
3. **Read functions with write side effects** `[V]` — `utils/soundpack.rs::load_soundpack_metadata` performs in-place V1→V2 conversion with backup *and* destructive multi→single conversion. A status command's call graph rewrites user files (B5).
4. **Orphaned Windows module** left in-tree (`input_worker_host.rs`), kept alive by a test `include_str!` (`log_buffer.rs:545`).
5. **`libs/bootstrap.rs` comment contradicts its code** (B9) — the "why" documentation lies; the comment goes with the else-branch in Phase 3.3.
6. **Process docs shipped at plugin root** `[C]` — `plan.md`, `relse.md`, `note.md`, `CLEANUP.md` live in the plugin dir, and `dev-sync.sh:26-29` rsyncs the *entire* dev dir into `~/.config/omarchy/plugins/...` — so they ship to users. **Correction:** "move to `docs/`" does *not* fix it (the rsync copies that too). The fix is an rsync `--exclude` for process docs, or deleting `relse.md`/`note.md` if obsolete.
7. **Two different "builtin soundpacks" concepts** `[V]` — `paths.rs:37-48` hardcodes the 10 builtin ids; `collect_packs` treats the directory as the source of truth. The constant is dead — same deletion as dead-code item (do once).
8. **`state/` ↔ `utils/` split is arbitrary** — four modules for the soundpack lifecycle, with conversion reachable from the cache layer.
9. **Meta-tests via `include_str!` source scanning** (`engine.rs:600-663`, `log_buffer.rs:543-563`) pin implementation details and rot; one asserts on `input_worker_host.rs` (dies with the file, Phase 5.1) and one asserts on the `UiEvent::DeviceSwitched` string (dies with B1, Phase 2.1).
10. **`config.rs` migration table** (`:156-170`) — hardcoded rename list that only grows; fine for now.
11. **`manifest.json` and `Cargo.toml` versions are manually kept in sync** (both 0.1.1, verified); `build-sorakey.sh` reads only the manifest.
12. **`scripts/__pycache__` and `admin-scripts/__pycache__` are checked into the repo** `[V]` — confirmed via `git ls-files`; `.gitignore` is 4 lines and covers neither.

---

## 7. Security Concerns

1. **`Panel.qml:173-178` shell string concatenation with `$HOME`** (B12) — breaks with spaces; use a daemon `ctl` command instead of `sh -c`.
2. **No max request size on the control socket** (B10) — local-only (socket `chmod 0600`, `control.rs:29`), a robustness issue, not a privilege boundary.
3. **Prebuilt-binary trust model** `[V]` — `build-sorakey.sh:56-82`: SHA256SUMS fetched from the *same* release as the binary; a MITM/takeover of the GitHub release could swap both. The attested path is correct but optional (needs `gh auth login`); the script says so honestly. Worth documenting that "checksum verified" ≠ "provenance verified".
4. **`Service.qml:178` `pkill -x sorakey`** — kills any user process named `sorakey` (name collision risk, low). Note: the daemon writes **no PID file** (verified — every `process::id()` use is a temp-file name), so a PID-based kill requires new daemon-side code.
5. **Log export privacy** `[V]` — `log_buffer` masks usernames in paths and key identities in verbose lines, but `evdev_input_listener.rs:17` logs `Current user: {:?}` directly, which `mask_user_paths` won't catch → username leaks into exported logs.
6. **Zip import** `[V]` — traversal guards, zip-bomb cap on decompressed size, temp-dir-then-rename. No issues found.
7. **`delete_pack`** `[V]` — id validation + `canonicalize` containment check. Solid.
8. **`dev-sync.sh` copies `.git` into `~/.config`** — intentional (documented); git config (user email/name) ends up in the installed tree; benign.
9. **systemd unit** `[C]` — user-scoped, no privilege, `Restart=on-failure` + `RestartSec=2` with no explicit limits — true, **but "retries indefinitely" is wrong**: systemd applies default start rate-limiting (`StartLimitBurst=5` per 10 s) to `Restart=` already. A crash-looping daemon ends in `failed` state after ~5 fast restarts. Explicit limits would only document intent, not fix a bug.

---

## 8. Accessibility Issues

1. **Invisible glyphs on interactive controls** (UX-1) — verified; no visible icon, screen reader gets empty text.
2. **Custom `Rectangle` delegates have no `Accessible` role/name** `[V]` — `SearchablePackDropdown.qml:286-355` (row = raw `Rectangle` + `Text` + `MouseArea`s; `delBtn` = `Text` + `MouseArea`) and the confirm footer. Zero `Accessible.role`/`onAccessiblePress` in the file (grep). QML's default accessibility won't expose them.
3. **Bar button semantics** `[V]` — right-click mutes, wheel changes volume, left-click opens the panel (`Panel.qml:417-440`): three actions on one control with no `Accessible` description.
4. **Keyboard navigation** — actually good (arrows/j-k/Enter/Esc, `:203-284`); the trigger has `activeFocusOnTab` + key handling.
5. **Color-only state** `[V]` — verified in framework: `WidgetButton` renders `dimmed` as `opacity: 0.45` and `active` as a glyph color swap. If the host renders both similarly, mute state is invisible. Delete/red uses hardcoded `#ff6b6b`.
6. **Tofu without Nerd Font** (UX-13) — every icon is a PUA codepoint. **Correction:** `fontFallbacks: ["Noto Sans", "DejaVu Sans", ...]` **cannot** fix this — those fonts contain no PUA glyphs; Qt skips straight to tofu. The fix is the glyph choice itself (emoji/text, see UX-1) or shipping/requiring the font.
7. **`TextField` test box** — no `Accessible.name`; moot if removed in Phase 6.5.

---

## 9. Dependency Notes (`daemon/Cargo.toml`)

- **Unused**: `rand` (0.9.0), `env_logger` (0.11.6, + its `log` transitive). Remove.
- **Questionable**: `directories` (6.0) used only for `BaseDirs::new()` in `state/paths.rs:7-11` — replaceable with `XDG_DATA_HOME`/`$HOME` env reads.
- **Dead after Phase 3.3**: `rdev` (the fallback listener is deleted).
- **Reasonable**: `crossbeam-channel`, `evdev`, `hound`, `libc`, `rodio`+`cpal`, `rubato`, `serde`/`serde_json`, `symphonia`, `chrono`.
- `[profile.dev] opt-level = 2` with `debug = true` — deliberate for local development; fine.

---

# Complete Fix Plan — All Issues, Phase by Phase

Each phase is independently deployable and testable. Findings marked `[V]` were verified against the code; corrections are folded into the fix rows.

## Phase 1 — Data Safety (prevent user data loss) ✅ done

**Goal:** No read command can ever modify user files. V1 conversion is deterministic and consistent.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 1.1 | **B5: Remove destructive multi→single from read path** | `daemon/src/utils/soundpack.rs:76-102` | Delete the `convert_v2_multi_to_single` call (and the re-read block) from `load_soundpack_metadata` — ~27 lines of pure deletion. The engine already supports multi packs. |
| 1.2 | **B3/B4: One authoritative V1 keycode table** | `daemon/src/utils/config_converter.rs:736-938`, `scripts/_v1_shared.py` | Fix the Rust "Alternative range" block that overwrites the main block (keep 3639–3667, which add missing numpad keys; resolve the 4 conflicts `3597/3612/3613/3640`). Dedup the Python dict (163 → 159). Add a test asserting no key appears twice in either table. |
| 1.3 | **B15: Stable sort for V1 multi-conversion** | `daemon/src/utils/config_converter.rs:92-94` | Sort by `(numeric_code, is_release)` so press always comes before release — matching the existing definitions sort at `:188-192`. |
| 1.4 | **B16: Log symphonia decode errors** | `daemon/src/utils/symphonia.rs:44-54` | Replace silent `break`/`continue` with a log line + failure counter. (Original "mark pack corrupted in cache if >10% fail" dropped — scope creep for a logging fix; the cache flag would be state nobody reads.) |

**Verify:** Convert the same V1 pack in Python and Rust → identical key mappings. Multi-pack `config.json` is byte-identical before/after cache writes from `delete_pack` and pack load (NOT `diag` — it doesn't rescan; see B5 correction). `cargo test` passes.

**Phase checklist:**
1. **Read** the files listed in the "Files" column for every fix in this phase
2. **Make changes** to fix each item
3. **Build**: `cd daemon && cargo build --release`
4. **Test**: `cargo test` — all tests must pass
5. **Deploy**: `cp target/release/sorakey ~/.local/bin/sorakey && systemctl --user restart sorakey`
6. **Verify** against the "Verify" block at the end of the phase
7. **Deploy panel**: `./admin-scripts/dev-sync.sh`
8. **Move to next phase**

---

## Phase 2 — Memory Leak & Engine-Start Failure ✅ done

**Goal:** No per-keystroke leak. Engine-start failure is clean and loud.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 2.1 | **B1: Delete UiEvent channel** | `daemon/src/libs/audio/engine.rs:44-53, 71-72, 88, 91, 95-96, 100-109, 493, 514, 527, 534, 548, 569, 582` | Remove `UiEvent` enum, `event_tx`/`event_rx`, `ENGINE_HANDLE`, `UI_EVENT_RX`, `engine_handle()`, `ui_event_receiver()`, all four `event_tx.send(...)` sites, and the `event_tx` param from `run_engine`/`handle_command`. **Keep** the `AudioEngineHandle` struct (live). **Also:** delete the assert on the string `"UiEvent::DeviceSwitched"` in the test `manual_device_switching_is_still_supported` (`engine.rs:662`) — keep the `SwitchDevice`/`fn switch_device` asserts. (The `log_buffer.rs:545` `include_str!` test is a Phase 5.1 concern, not here.) |
| 2.2 | **B7 (corrected): Clean engine-start failure** | `daemon/src/libs/audio/engine.rs:177-184` | *Optional polish, no functional change.* A panic on the non-main engine thread already aborts the process, so systemd already restarts (and rate-limits) — there is no zombie. Replace `panic!()` with a clear `always_eprint!` + `std::process::exit(1)` so the journal shows "no audio device — check configuration" instead of a backtrace. |
| 2.3 | **B10: Control socket request size limit** | `daemon/src/control.rs:51-66` | `read_until` with a 64KB cap; reject with `{"ok":false,"error":"request too large"}`. |
| 2.4 | **B6: Delete dead trace machinery** | `daemon/src/libs/trace.rs`, `daemon/src/utils/log_buffer.rs:78-99` | `trace::init()` has no callers, so the enable path is dead — **but** `trace::record`/`time` have live call sites (`engine.rs:505, 522, 577-581`), so "delete trace.rs" alone breaks the build. Correct scope: delete the enable machinery (`init`, `set_runtime_tracing`, `ensure_writer`, `writer_loop`/`Pending`/file rotation, `SORAKEY_TRACE` read), remove the call-site wrappers in the engine (unwrap `time(...)`, delete `record(...)` lines; the worker-host call dies with Phase 5.1), remove `set_verbose`/`verbose_enabled`/`push_verbose` from `log_buffer.rs`, and delete the trace-dependent tests (`log_buffer.rs:566, 641` and the `Point::UiEventSent` references). Then `trace.rs` can be deleted entirely. |

**Verify:** Type 10,000 keys → RSS flat (was growing ~40B/keystroke). `cargo build --release` + `cargo test` pass with zero trace references. Unplug audio before first start → daemon exits 1 with a clear journal line → systemd restarts, rate-limits to `failed` after 5 fast tries.

**Phase checklist:**
1. **Read** the files listed in the "Files" column for every fix in this phase
2. **Make changes** to fix each item
3. **Build**: `cd daemon && cargo build --release`
4. **Test**: `cargo test` — all tests must pass
5. **Deploy**: `cp target/release/sorakey ~/.local/bin/sorakey && systemctl --user restart sorakey`
6. **Verify** against the "Verify" block at the end of the phase
7. **Deploy panel**: `./admin-scripts/dev-sync.sh`
8. **Move to next phase**

---

## Phase 3 — Correctness Bugs ✅ done

**Goal:** All remaining bugs that cause wrong behavior.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 3.1 | **B2: Fix acquire_lock** | `daemon/src/main.rs:50-61` | Fall back to `$HOME/.sorakey.lock` when `XDG_RUNTIME_DIR` is unset (same pattern as `control.rs`). Only report "already running" when the flock is genuinely held. |
| 3.2 | **B8: Fix switch_device resample rate** | `daemon/src/libs/device_manager.rs:324-337`, `engine.rs:247` | After opening the new device, query *its* rate (from the opened stream), not from config. Required before Phase 7.1 (device selector). |
| 3.3 | **B9: Remove rdev fallback** | `daemon/src/libs/bootstrap.rs:11-19`, `daemon/src/libs/input_listener.rs` (entire file), `daemon/src/libs/mod.rs:5` | Delete the `else` branch (and the lying comment that goes with it). Delete `input_listener.rs` (218 LOC — verified referenced only by bootstrap). Remove the `mod` declaration. |
| 3.4 | **B11: Delete AlsaErrorSuppressor** | `daemon/src/libs/device_manager.rs:11-46` | Delete the suppressor (it's only used by dead `test_*_device` paths anyway); ALSA errors flow to the journal. |
| 3.5 | **B12: Fix shell concatenation in Panel.qml** | `Panel.qml:173-178, 184` | Replace `sh -c` with a `ctl` command (`set_bar_section`/`get_bar_section`) handled by the daemon. No shell, no space issues. **Same code as UX-8 / Phase 6.8 — implement once.** |
| 3.6 | **B14: Remove no-op config field** | `daemon/src/state/config.rs:20, 119, 274`, `daemon/src/state/config_writer.rs:257` | Remove `enable_keyboard_sound` from the struct, `data_equals`, and defaults — **and** from the concurrency test at `config_writer.rs:257` (hidden test touchpoint; build breaks otherwise). Lazy call: delete (nothing reads it, nothing sets it). serde is lenient (`parse_lenient`), so existing configs with the field still load. |

**Verify:** `cargo test` passes. Start daemon without `XDG_RUNTIME_DIR` → works. No `rdev` in `Cargo.toml`/`Cargo.lock`. Panel bar-section save works with `HOME="/home/test user"`. (Switch-device pitch check is moot until 7.1 exposes it.)

**Phase checklist:**
1. **Read** the files listed in the "Files" column for every fix in this phase
2. **Make changes** to fix each item
3. **Build**: `cd daemon && cargo build --release`
4. **Test**: `cargo test` — all tests must pass
5. **Deploy**: `cp target/release/sorakey ~/.local/bin/sorakey && systemctl --user restart sorakey`
6. **Verify** against the "Verify" block at the end of the phase
7. **Deploy panel**: `./admin-scripts/dev-sync.sh`


---

## Phase 4 — Performance (hot path) ✅ done

**Goal:** <5ms key-to-sound latency. The risk in this phase is item 4.2 specifically — the others are small.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 4.1 | **evdev blocking read (kill 20ms poll)** | `daemon/src/libs/evdev_input_listener.rs:44, 63-138` | Remove `set_nonblocking(true)` + the `sleep(20ms)` loop. Blocking `fetch_events()` per device (one thread per device, max 1-2 keyboards); the existing `Err → remove device` handling still works. evdev 0.13.2 supports both (verified crate API). |
| 4.2 | **Pack load off engine thread** | `daemon/src/libs/audio/engine.rs`, `soundpack_loader.rs` | Worker thread for decode+resample; engine swaps state when done, old pack keeps playing during load. **Riskiest item in the phase** — `EngineState` is thread-owned plain fields on purpose (`engine.rs:111-115`), so this restructures the state ownership model. Do after 4.1/4.3. |
| 4.3 | **Precompute per-keystroke buffers** | `daemon/src/libs/audio/engine.rs:374-376` | At pack load, pre-apply the fade per (key, segment) and store the final `Vec<f32>`; keypress time becomes zero-allocation except `Sink::try_new`. Memory-trivial (~1 MB for 60 keys of 50 ms). |
| 4.4 | **Resampler: reduce allocation churn** — *deferred* | `daemon/src/libs/audio/resampler.rs:68-113` | Real but runs at pack-load time only, never per keystroke — subsumed by 4.2's worker thread. Revisit only if load time stays visible after 4.2. |
| ~~4.5~~ | ~~Config RwLock snapshot~~ — **skipped** | `config_writer.rs` | Claim is true (full `AppConfig` clone per `current()`) but the fix is cargo-culted: config is never read per keystroke (engine caches `volume`/`sound_enabled`), `current()` only runs at ctl-request time, and it clones a 7-field struct — nanoseconds. Revisit if a profiler ever shows a hot read path. |
| 4.6 | **Reduce panel poll frequency** | `Panel.qml:389-415` | `packs` poll 5s → 30s (packs change only on import/delete). Keep `status` at 5s. |
| 4.7 | **Fix build-sorakey.sh hash** | `scripts/build-sorakey.sh:22-23` | Exclude `target/` from the source hash: `find "$DAEMON_DIR" -path "$DAEMON_DIR/target" -prune -o -name "Cargo.toml" -print -o ...`. (The "non-git → prebuilt dead" behavior is by design — `release_matches_source` requires git to trust a prebuilt against a tag; leave it.) |

**Verify:** `cargo test` passes. Type fast (10 keys/sec) → key-to-sound latency <5ms (was up to ~20ms+). Switch to a 24MB pack → no multi-second silence (after 4.2). RSS stable under sustained typing. `git status` clean + `daemon/target/` present → `build-sorakey.sh` still reports "up to date".

**Phase checklist:**
1. **Read** the files listed in the "Files" column for every fix in this phase
2. **Make changes** to fix each item
3. **Build**: `cd daemon && cargo build --release`
4. **Test**: `cargo test` — all tests must pass
5. **Deploy**: `cp target/release/sorakey ~/.local/bin/sorakey && systemctl --user restart sorakey`
6. **Verify** against the "Verify" block at the end of the phase
7. **Deploy panel**: `./admin-scripts/dev-sync.sh`


---

## Phase 5 — Dead Code Sweep (~900 LOC in this phase; ~1,400 total across Phases 2–5) ✅ done

**Goal:** Every line in the codebase is reachable.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 5.1 | **Delete `input_worker_host.rs`** | `daemon/src/libs/input_worker_host.rs` (498 LOC), `daemon/src/utils/log_buffer.rs:543-563` | Orphaned Windows file. Delete the `include_str!` test `no_per_event_code_path_logs` too — **but keep the engine half of its assertions** (move the `handle_key_event`-must-not-log checks into `engine.rs`'s own test module) so the invariant survives. |
| 5.2 | **Remove unused deps** | `daemon/Cargo.toml` | Remove `rand` and `env_logger` (+ `main.rs:19` init call). Remove `rdev` **only after Phase 3.3**. Note: 7.2/7.4 re-add `auto_startup`/`rand` if those features are built — skip the deletion in that case. |
| 5.3 | UiEvent API | covered by Phase 2.1 | — |
| 5.4 | **Remove DeviceManager dead methods** | `daemon/src/libs/device_manager.rs:76-82, 91-263, 293-354` | `get_input_devices`, `get_input_device_by_id`, `test_output_device`, `test_input_device`, `device_supports_rate`, custom `Clone` — all verified zero external callers (`get_output_devices` **stays** — Phase 7.1 uses it). |
| 5.5 | **Remove `auto_startup`** | `daemon/src/utils/auto_startup.rs` | Delete `set_auto_startup` (unreachable). **Keep `get_auto_startup_state`** (called at config load, `config.rs:201`). If Phase 7.2 is being built, skip this item. |
| 5.6 | **Remove builtin-soundpacks list** | `daemon/src/state/paths.rs:37-52, 87` | `BUILTIN_SOUNDPACKS`, `is_builtin_soundpack`, `get_soundpacks_dir` — no callers. Same item as org issue 7 — do once. |
| 5.7 | **Remove dead log_buffer functions** | `daemon/src/utils/log_buffer.rs:57-75, 213-237` | `export_to_file`, `export_file_name`, `reveal_in_file_manager` → delete. `recent()`, `generation()` → **move under `#[cfg(test)]`, do NOT delete** — `logger.rs`'s tests use them (`logger.rs:66-73`). |
| 5.8 | trace.rs | covered by Phase 2.4 | — |
| 5.9 | **Remove `SoundpackType`, `SoundpackCount`, empty impls** | `daemon/src/state/soundpack.rs:7-18, 85-87, 123-126` | One-variant enum + bespoke default fn, one-field struct. `update_count`/`count` simplify to a `usize`. |
| 5.10 | **Remove duplicate logger macros** | `daemon/src/utils/logger.rs:11-32`, call sites | Delete `debug_print!`/`debug_eprint!`, migrate call sites (`control.rs:43`, `config.rs:143, 259-260`, `bootstrap.rs:9`) to `always_*`. Update the macro-name list in the test at `log_buffer.rs:550`. |
| 5.11 | **Remove dead icon-URL code** | `daemon/src/libs/audio/soundpack_loader.rs:147-159`, `utils/soundpack.rs:175-202` | **Keep the `icon` field** (cache JSON contract; Phase 7.6 displays it) — but emit the raw filename or `None` instead of fake `/soundpack-images/...` URLs. Wipes ~10 lines of `[CACHE DEBUG]` logging for free. |
| 5.12 | **Remove `__pycache__` from git** | `scripts/__pycache__/`, `admin-scripts/__pycache__/` | `git rm -r --cached` (confirmed tracked via `git ls-files`), add `__pycache__/` + `*.pyc` to `.gitignore`. |
| 5.13 | `bootstrap.rs` dead branch | covered by Phase 3.3 | — |
| 5.14 | **Remove `SoundpackMetadata` dead fields** | `daemon/src/state/soundpack.rs:103-108` | `can_be_converted` + `last_accessed` — written, never read. **Keep `validation_status`** (`soundpack_loader.rs:355` writes `"loading_error"` into it — the panel's only per-pack error channel) and `last_error`. |

**Verify:** `cargo build --release` → 0 new warnings. `cargo test` passes. `grep -r "rand\|env_logger\|rdev" Cargo.toml` → 0 matches. `wc -l` on the daemon drops ~900 in this phase (the original "~2,000+" counted Phases 2–3 deletions too: 498 + 218 + 453 ≈ 1,170 already gone there).

**Phase checklist:**
1. **Read** the files listed in the "Files" column for every fix in this phase
2. **Make changes** to fix each item
3. **Build**: `cd daemon && cargo build --release`
4. **Test**: `cargo test` — all tests must pass
5. **Deploy**: `cp target/release/sorakey ~/.local/bin/sorakey && systemctl --user restart sorakey`
6. **Verify** against the "Verify" block at the end of the phase
7. **Deploy panel**: `./admin-scripts/dev-sync.sh`


---

## Phase 6 — UX Fixes (visible to user) ✅ done

**Goal:** Every button is visible. Every action gives feedback. No confusing states.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 6.1 | **Replace invisible U+FF30 glyphs** | `SearchablePackDropdown.qml:326, 379`, `Panel.qml:838` | Hex-verified (`EF BD B0` at all three). Use **emoji or ASCII text** (`⌫`/`✕`, `⚠`) rather than other PUA Nerd Font glyphs — PUA codepoints render only if the user has exactly that font, and no fallback list can fix tofu. This also resolves Phase 9.5. |
| 6.2 | **Add ctl error feedback** | `Panel.qml:91-96` | Parse `ok:false` in `ctlProc.onExited`; show a toast banner, auto-clear 5s. **Note:** single shared `ctlProc` — track the pending request (closure var) to correlate the response with the command that produced it. |
| 6.3 | **Fix mute switch when stopped** | `Panel.qml:515` | `checked: root.muted` + `enabled: root.running`. (Corrected diagnosis: stopping currently flips the switch to **off/unmuted**, not "off as if muted" — same fix.) |
| 6.4 | **Add hotkey discoverability** | `Panel.qml:424` | Tooltip: `"Sorakey — Playing\nRight-click: Mute\nCtrl+Alt+M: Global mute"`. Optional "Shortcuts" row in Settings. |
| 6.5 | **Remove TEST TYPING** | `Panel.qml:774-794` | Delete the whole Column. The daemon listens system-wide; the box does nothing. |
| 6.6 | **Add "no soundpack" state** | `Panel.qml:70-75` | When `keyboardPack === ""` and `keyboardPacks.length === 0`: statusText = "No soundpack", disable slider, hint "Import a pack to get started". |
| 6.7 | **Add per-pack volume reset** | `Panel.qml` (near slider), `daemon/src/control.rs` | Small "↺" button → `ctl {cmd:"reset_volume", id}`. Daemon: delete the `per_pack_volume` entry, re-apply `recommended_volume_for` (already exists, `control.rs:131-136`) + `SetVolume`. ~10 daemon lines. |
| 6.8 | **Bar-section persistence** | `Panel.qml:169-191` | **Same fix as Phase 3.5 (B12) — one implementation.** If 3.5 already landed, this row is done. |
| 6.9 | **Make update flow robust** | `Panel.qml:216-219` | `exitCode === 0` → success, `!== 0` → show full stderr. Drop substring matching. |
| 6.10 | **Make Stop sticky** | `Service.qml:147-153` | Only auto-start if the user hasn't explicitly stopped it. `~/.local/share/sorakey/stopped` flag: write on Stop, check in `Component.onCompleted`, remove on Start. |
| 6.11 | **Extend status string timeout** | `Service.qml:65` | 4s → 10s (errors: don't auto-clear). |
| 6.12 | **Remove duplicate Update button** | `Panel.qml:595-604` or `:822-831` | Keep in Settings (with the version text); delete the bottom-row copy and the manual spacer math at `:833`. (Original plan said bottom-row — either works, Settings groups it with the rest.) |
| 6.13 | **aarch64 CI build** | `.github/workflows/release.yml` | **Correction:** a matrix alone is not enough: (a) each runner generates its own single-entry `SHA256SUMS` (line 31) → add a merge step combining both architectures into one file; (b) aarch64 needs explicit `--target aarch64-unknown-linux-gnu` + `aarch64-linux-gnu-gcc` apt dep (the build step currently uses the host default target); (c) the X11 deps (`libx11-dev libxtst-dev libxi-dev`) can drop once rdev is gone (Phase 3.3). |

**Verify:** Open panel → all buttons visible. Delete a pack → visible confirm icon. Stop daemon → mute switch disabled. No soundpack → clear status. `grep -n "FF30\|/bin/sh" Panel.qml SearchablePackDropdown.qml` → 0.

**Phase checklist:**
1. **Read** the files listed in the "Files" column for every fix in this phase
2. **Make changes** to fix each item
3. **Build**: `cd daemon && cargo build --release`
4. **Test**: `cargo test` — all tests must pass
5. **Deploy**: `cp target/release/sorakey ~/.local/bin/sorakey && systemctl --user restart sorakey`
6. **Verify** against the "Verify" block at the end of the phase
7. **Deploy panel**: `./admin-scripts/dev-sync.sh`


---

## Phase 7 — New Features

**Goal:** Ship the features that fix real pain. Lazy subset: 7.1, 7.2, 7.3, 7.5, 7.8, 7.9. Skipped as YAGNI: 7.4, 7.7.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 7.1 | **Output device selector** | `control.rs` (new `audio_devices` + `select_device` cmds), `Panel.qml` (Settings dropdown) | `ctl {cmd:"audio_devices"}` → `get_output_devices()` (already exists, currently dead). `select_device` → `SwitchDevice` (fix B8 first, Phase 3.2). Persists in config. Highest-value feature. |
| 7.2 | **Auto-start toggle** | `control.rs` (`set_autostart`), `Panel.qml` (checkbox), re-add `auto_startup::set_auto_startup` | `systemctl --user enable/disable sorakey`. Conflicts with 5.5 — if this is built, skip 5.5. |
| 7.3 | **In-panel log viewer** | `control.rs` (`recent_logs`), `Panel.qml` (Settings) | `ctl {cmd:"recent_logs", count:50}` → `log_buffer::recent(50)` (already exists). Read-only `TextArea` + copy button. Cheapest feature. |
| 7.4 | **random_pitch** — *YAGNI, skipped* | — | Honest fix: delete the dead `random_pitch` field (and the `rand` dep, with 5.2). Implement only if a real pack sets it. |
| 7.5 | **Diagnostics/health surface** | `engine.rs` (new state), `control.rs` (`diag` already exists), `Panel.qml` (Settings) | Show PID, uptime, RSS, active pack, output device. **Correction:** the engine stores **no last-key-event timestamp** — add a field bumped in `handle_key_event`, expose via `diag`, display "Last key: Ns ago" with a red indicator if quiet >30s while "Playing". The only thing that catches the #1 failure mode (missing `input` group → silent). |
| 7.6 | **Pack metadata display** | `control.rs` (metadata-returning `packs` or new cmd), `Panel.qml`, `Model.js` | **Correction:** `packs()` returns IDs only (directory scan, `control.rs:271-290`) — it never reads the cache. Needs a metadata-returning command (name/author/description from `SoundpackCache` — depends on 1.1 making the cache read pure). Not "just display it". |
| 7.7 | **Hotkey configuration** — *YAGNI, skipped* | — | Ctrl+Alt+M stays hardcoded. Config + ctl cmd + UI for a single mute hotkey is complexity nobody asked for. Revisit if someone asks. |
| 7.8 | **Fix uninstall** | `scripts/uninstall.sh` | Add `rm -rf ~/.cache/sorakey ~/.local/lib/sorakey` to the purge path (and `bar-section` note). Three lines. |
| 7.9 | **Pack cache rescan trigger** | `control.rs` (`rescan_packs`), `Panel.qml` | `ctl {cmd:"rescan_packs"}` → `refresh_from_directory()`. Panel calls after import/delete. Closes the stale-cache gap (B5's aftermath). |

**Verify:** Device dropdown switches output with correct pitch. Auto-start toggle persists across reboot. Log viewer shows last 50 lines. Diag shows "Last key: 2s ago" updating. Uninstall `--purge` leaves nothing.

**Phase checklist:**
1. **Read** the files listed in the "Files" column for every fix in this phase
2. **Make changes** to fix each item
3. **Build**: `cd daemon && cargo build --release`
4. **Test**: `cargo test` — all tests must pass
5. **Deploy**: `cp target/release/sorakey ~/.local/bin/sorakey && systemctl --user restart sorakey`
6. **Verify** against the "Verify" block at the end of the phase
7. **Deploy panel**: `./admin-scripts/dev-sync.sh`


---

## Phase 8 — Code Organization & Security

**Goal:** One table, one converter, no side-effects on reads, no shell injection.

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 8.1 | **Consolidate key-mapping tables** | `evdev_input_listener.rs:143-204`, `config_converter.rs:736-938` | One shared Rust `static` `KeyMap` table referenced by both the evdev listener and the converter (generated-code build script dropped — over-engineered for a constant table). Only two tables remain after Phase 3.3. |
| 8.2 | **Consolidate V1→V2 converters** | `admin-scripts/v1-to-v2-converter.py` (delete) | Delete the one-off admin converter (job done; also carries a shadowed-import bug: it imports `SMART_DONOR` at line 20 then redefines the whole dict at line 75). Keep Python importer + Rust converter on the one table from 1.2. Add a cross-test: convert one sample V1 pack in both, assert identical mappings. |
| 8.3 | **Separate read/write in soundpack loading** | `daemon/src/utils/soundpack.rs` | Same as Phase 1.1, re-scoped — do once. V1→V2 auto-conversion (with backup) stays in the import path only; `load_soundpack_metadata` becomes pure read. |
| 8.4 | `bootstrap.rs` comment | — | Nothing to do — the comment goes with the else-branch in Phase 3.3 (the file stays; it still starts the evdev listener). |
| 8.5 | **Keep process docs out of the synced plugin** | `admin-scripts/dev-sync.sh:26-29`, root `*.md` | **Correction:** "move to `docs/`" doesn't work — `dev-sync.sh` rsyncs the *entire* dir into `~/.config`. Fix: add `--exclude` for process docs (`plan.md`, `relse.md`, `note.md`, `CLEANUP.md`), or delete `relse.md`/`note.md` if obsolete. |
| 8.6 | Hardcoded builtin soundpacks list | — | **Same item as Phase 5.6 — do once there.** |
| 8.7 | **Version sync test** | `daemon/tests/` (new) | `cargo test version_sync`: parse `manifest.json` + `Cargo.toml`, assert equal. ~10 lines. |
| 8.8 | **Safer kill of the daemon** | `Service.qml:178` | **Correction:** the daemon writes **no PID file** (verified — all `process::id()` uses are temp-file names), so the plan's "read `sorakey.pid`" requires new daemon-side code. Lazy fix: replace `pkill -x sorakey` with a `pkill -x -f "$HOME/.local/bin/sorakey"`-style exact-path match (QML-only). Or leave `pkill -x` — the collision risk is tiny. |
| 8.9 | **Mask username in evdev log** | `daemon/src/libs/evdev_input_listener.rs:17` | Delete the `Current user: {:?}` line — it adds no value and leaks into exported logs. |
| 8.10 | systemd StartLimit | `scripts/sorakey-setup:30-42` | **Correction:** systemd *already* applies default `StartLimitBurst=5` / 10 s to `Restart=` — a crash-looping daemon ends in `failed` state; it does not retry indefinitely. Explicit `StartLimitIntervalSec=30`/`Burst=5` would be *looser* than the default. **Dropped** (or kept only as "document the intent" — optional). |
| 8.11 | **Fix .gitignore** | `.gitignore` | Add `__pycache__/`, `*.pyc` (`daemon/target/` already covered). |

**Verify:** `grep -rn "FF30" *.qml` → 0. `ls ~/.config/omarchy/plugins/io.github.sandeshrai00.sorakey/*.md` → none after dev-sync. `cargo test version_sync` passes. `journalctl --user -u sorakey` shows no `Current user` lines.

**Phase checklist:**
1. **Read** the files listed in the "Files" column for every fix in this phase
2. **Make changes** to fix each item
3. **Build**: `cd daemon && cargo build --release`
4. **Test**: `cargo test` — all tests must pass
5. **Deploy**: `cp target/release/sorakey ~/.local/bin/sorakey && systemctl --user restart sorakey`
6. **Verify** against the "Verify" block at the end of the phase
7. **Deploy panel**: `./admin-scripts/dev-sync.sh`


---

## Phase 9 — Accessibility

**Goal:** Screen-reader users can navigate. All state is non-color. (Depends on 6.1 for glyphs.)

| # | Fix | Files | Detail |
|---|-----|-------|--------|
| 9.1 | **Accessible roles on custom delegates** | `SearchablePackDropdown.qml:286-355` | Verified: zero `Accessible` properties in the file. `Accessible.role: Accessible.ListItem` + name on rows, `Accessible.role: Accessible.Button` + `Accessible.name: "Delete " + packName` + `onAccessiblePress` on `delBtn`, same for the confirm footer buttons. |
| 9.2 | **Accessible descriptions on bar button** | `Panel.qml:417-440` | `Accessible.name: "Sorakey keyboard sounds"`, `Accessible.description: "Left click: open panel. Right click: toggle mute. Scroll: adjust volume."` |
| 9.3 | **Visible open/closed state on dropdown trigger** | `SearchablePackDropdown.qml:127-136` | **Correction:** the trigger **already has a chevron** (`󰅀`, line 132) — don't add a second. Rotate/swap it on `popup.opened`. (Note: if 6.1 replaces PUA glyphs with emoji, the chevron becomes `▼`/`▲` and this is free.) |
| 9.4 | **Non-color mute state** | `Panel.qml:422-423` | Verified in framework: `dimmed` → `opacity: 0.45`, `active` → glyph color swap only. Muted: add a non-color indicator (glyph swap, e.g. `⌨`→`🔇`, or an "M" badge). |
| 9.5 | ~~Font fallbacks~~ — **merged into 6.1** | — | **Correction:** `fontFallbacks: ["Noto Sans", "DejaVu Sans", ...]` cannot fix PUA tofu — those fonts contain no private-use glyphs; Qt skips straight to tofu. The real fix is the glyph choice (emoji/text) made in 6.1. |
| 9.6 | Accessible name on TEST TYPING | moot (removed in 6.5) | — |
| 9.7 | ~~Theme-aware danger color~~ — **dropped** | — | **Correction:** `Style.colors.danger` does not exist in the omarchy framework (grep-verified — the palette exposes `Color.accent`, `Color.popups.*`, etc.). Keeping `#ff6b6b` (readable on both light and dark) is fine; adding a token is upstream framework work, not a sorakey fix. |

**Verify:** Orca: navigate panel → announces "Sorakey keyboard sounds, button". Tab to delete → "Delete sankey-mx-brown, button". Mute state announced. All icons render without a Nerd Font (after 6.1's glyph choice).

**Phase checklist:**
1. **Read** the files listed in the "Files" column for every fix in this phase
2. **Make changes** to fix each item
3. **Build**: `cd daemon && cargo build --release`
4. **Test**: `cargo test` — all tests must pass
5. **Deploy**: `cp target/release/sorakey ~/.local/bin/sorakey && systemctl --user restart sorakey`
6. **Verify** against the "Verify" block at the end of the phase
7. **Deploy panel**: `./admin-scripts/dev-sync.sh`


---

## Phase 10 — Final Hardening (release steps deferred)

**Goal:** Ship-ready code. The release itself (tag, CI upload, version bump) is done separately when a release is actually planned.

| # | Fix | Detail |
|---|-----|--------|
| 10.1 | **`cargo clippy --all-targets -- -D warnings`** | Fix all lints. |
| 10.2 | **`cargo test`** | All green. Ensure the new tests from earlier phases exist and pass: V1 conversion determinism (Py vs Rust parity), multi-pack non-destruction on cache write, socket 64KB limit, no duplicate keys in either table, version sync. |
| 10.3 | **README pass** | Document hotkey, output device selector, auto-start, log viewer, diagnostics. Update uninstall instructions. |

**When a release is actually planned (not now):**
- Version bump `manifest.json` + `Cargo.toml` → 0.2.0 (config format may change with Phases 1/3).
- CI: x86_64 + aarch64 matrix **with the 6.13 corrections** (merged SHA256SUMS, explicit `--target`, dropped X11 deps).
- Tag, CI builds, upload binary + SHA256SUMS.

**Phase checklist:**
1. **Read** the files listed in the "Files" column for every fix in this phase
2. **Make changes** to fix each item
3. **Build**: `cd daemon && cargo build --release`
4. **Test**: `cargo test` — all tests must pass
5. **Deploy**: `cp target/release/sorakey ~/.local/bin/sorakey && systemctl --user restart sorakey`
6. **Verify** against the "Verify" block at the end of the phase
7. **Deploy panel**: `./admin-scripts/dev-sync.sh`


---

## Dependency Graph (what must come before what)

```
Phase 1 (data safety) ─────────────────────────────────────────┐
Phase 2 (leak/clean-exit) ─────────────────────────────────────┤
Phase 3 (correctness) ── depends on Phase 1 (B5 fix first)     │
Phase 4 (performance) ── depends on Phase 2 (UiEvent removed)  │
Phase 5 (dead code) ── depends on Phases 2, 3.3 (rdev, UiEvent)│
Phase 6 (UX) ── independent; 6.8 == 3.5 (one implementation)   │
Phase 7 (features) ── 7.1 depends on 3.2 · 7.6 depends on 1.1  │
Phase 8 (org/security) ── depends on 1, 3, 5 · 8.6 == 5.6      │
Phase 9 (a11y) ── depends on 6.1 (glyphs) · 9.5 merged into 6.1│
Phase 10 (hardening) ── depends on ALL above                   │
```

**Conflicts (pick one side):** 7.2 vs 5.5 (auto_startup), 7.4 vs 5.2 (rand) — if a feature is built, skip its dead-code deletion.
**Parallelizable:** Phases 6 and 7 can run in parallel with 4–5 (different files). Phase 9 starts once 6.1 is done.

---

## Estimation

| Phase | Estimated effort | Risk | Notes |
|-------|-----------------|------|-------|
| 1 — Data Safety | 2-3 h | Low | deletion + dedup |
| 2 — Leak/Clean-exit | 1-2 h | Medium | 2.2 is now optional polish (no zombie — corrected) |
| 3 — Correctness | 2-3 h | Medium | XDG fix, device rate |
| 4 — Performance | 3-5 h | High (4.2 only) | 4.5 skipped, 4.4 deferred |
| 5 — Dead Code | 1-2 h | Low | pure deletion, ~900 LOC in-phase |
| 6 — UX | 3-4 h | Low | QML only; 6.13 deferred with release |
| 7 — Features | 4-5 h (lazy subset) | Medium | 7.4/7.7 dropped as YAGNI |
| 8 — Org/Security | 2-3 h | Low | 8.10 dropped |
| 9 — Accessibility | 1-2 h | Low | 9.5 merged, 9.7 dropped |
| 10 — Hardening | 1-2 h | Low | release steps deferred |
| **Total** | **~20-29 h** | | |

---

## How to Execute

Each phase ends with its own **Phase checklist** (Read → change → build → test → deploy → verify → deploy panel → next phase). Work the phases in dependency order (see Dependency Graph above); run each phase's checklist as you go.