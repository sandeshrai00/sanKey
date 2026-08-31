# Ponytail Plan — sorakey optimization (dead code + improvement + RAM)

> Scope: `daemon/` (26 .rs) + QML/JS + Python + CI. Goal: less code, less RAM, less CPU, same behavior. Ladder: stdlib > native > reuse > one line > minimal code.

## 0. Global debt

| file:line | issue | fix |
|---|---|---|
| `daemon/src/main.rs:10` | ` #![allow(dead_code)]` blinds `cargo warn` | delete, add per-item `#[allow(dead_code)]` only where proven |
| `daemon/src/libs/device_manager.rs:110` | `#[allow(dead_code)]` hides 11 unused fns | remove, delete unused or `#[cfg(feature)]` |
| `daemon/src/utils/soundpack_validator.rs:6,47` | 3 extra fields never read | keep `status`+`can_be_converted` |
| `daemon/Cargo.toml:16,21,25` | `once_cell`, `strum`, `futures-timer` 0 hits | delete (~400KB dep, -compile sec) — `OnceLock` is stdlib |

## 1. Dead code — delete without behavior change

**Rust**
- `utils/path.rs:30` `write_file_contents` — 0 callers
- `utils/data.rs:20` `save_json_to_file` (non-atomic) — only `*_atomically` used
- `libs/audio/soundpack_loader.rs:8` `determine_soundpack_type()->Keyboard` — inline
- `libs/cli_args.rs:1` `SOUNDPACK_ARG` — 0 hits
- `utils/logger.rs:13` `DEBUG_ENABLED` always true — `debug_print==always_print`
- `libs/device_manager.rs:12-14` `CACHED_* OnceLock<Mutex<Vec>>` + `405-518` `initialize_cache/get_cached*/refresh_cache` — never called
- `libs/input_worker_host.rs` `#[cfg(windows)]` but `libs/mod.rs:9` not cfg-gated; `enabled_keyboards` missing from `AppConfig`
- `state/soundpack.rs:10` `SoundpackType{Keyboard}` single variant; `132 SoundpackCount{keyboard:usize}` one-field struct
- `utils/soundpack.rs:8-231` auto V1→V2 convert on every `SoundpackCache::load` (3 reads, mutates user data) — move to `sorakey migrate` CLI
- `trace.rs:22-423` 400 LOC writer thread + `SINK` unbounded channel — daemon has no UI, never drains

**QML/JS/Python**
- `Service.qml:26` `signal importFailed` — never connected (`Panel.qml:241` only `onPacksImported`)
- `SearchablePackDropdown.qml:39` `signal hovered` + `16 triggerLabel` + `55 optionDescription` + `74 desc` filter — packs have no description
- `Service.qml:58-62` `if(output==="" && exitCode===0)` cancelled — unreachable (`gui_main` prints `ERROR:Cancelled`)
- `SearchablePackDropdown.qml:358` `visible:true` tautology
- `Model.js:4` + `SearchablePackDropdown.qml:44` + `scripts/sorakey-import-pack.py:211` `prettyPackName` copied 3×
- `scripts/sorakey-import-pack.py:58` + `admin-scripts/v1-to-v2-converter.py:25` `V1_KEY_TABLE` 110 keys copied; same for `SMART_DONOR`/`convert_v1_to_v2` ~170 lines — extract `scripts/_v1_table.py`

## 2. Improvement — dedup / native / zero-copy

### Audio pipeline (biggest ROI)
| file:line | over-built | lazy replacement | impact |
|---|---|---|---|
| `soundpack_loader.rs:76-453` + `config_converter.rs:755` | 2× `load_audio_with_symphonia` 377 LOC, 9× mono/stereo branches | extract `utils/symphonia.rs::decode_interleaved(path)->(Vec<f32>,u16,u32)` via `SampleBuffer::copy_interleaved_ref` | -500 LOC, fixes drift |
| `soundpack_loader.rs:135` + `resampler.rs:19` | `per_channel: Vec<Vec<f32>>` + interleave loop | push directly interleaved in decode loop, no matrix | -22MB transient peak, -30% decode |
| `resampler.rs:5-116` | `sinc_len:64, oversampling:32, BlackmanHarris2` for 5ms clicks | `sinc_len 16, oversampling 8` or `FastFixedIn` or skip offline resample (let `rodio::Sink` do it) | 2.5s → 0.3s for 24MB, `engine.rs:257` no longer blocks |
| `resampler.rs:19-24,65` | `deinterleaved` + `to_vec()` per chunk (3k allocs) | slice views, avoid per-chunk `to_vec` | -48MB peak → 24MB |
| `engine.rs:254` | `new_multi.insert(fname.clone(), Arc::new(resample…))` | `for (fname,audio) in std::mem::take(&mut self.multi_key_audio)` | zero String clones |
| `resampler.rs:12` | `return samples.to_vec()` on rate match | return `Arc::clone` (check before calling) | saves 24MB copy when rates match |
| `engine.rs:116-137` | `Host` cloned via `default_host()` each `DeviceManager::clone` | `Arc<Host>` shared | consistency |
| `soundpack_loader.rs:603` | `clear()` retains HashMap buckets | `take()` or document `ponytail: clear retains capacity` | few KB |
| `engine.rs:695` | `unsafe{ libc::malloc_trim(0)}` | delete — `drop(_old_kb)` already frees; keep only if RSS>100MB proven | removes `unsafe`+`libc` dep |

### Config / state
- `state/config.rs:49-87` `parse_lenient` clones + 8× `serde_json::from_value::<AppConfig>` — use `#[serde(deserialize_with)]` per field.
- `state/config.rs:164` `load()` spawns `systemctl` inside `OnceLock` init lock — move `auto_startup` check async.
- `state/config_writer.rs:46` `OnceLock<Mutex<AppConfig>>` — `RwLock` + `Arc<AppConfig>`; `current()` returns `Arc::clone` not deep clone.
- `state/paths.rs:13` `directories::BaseDirs` — `std::env::var("XDG_DATA_HOME")` native, cut crate.
- `state/paths.rs:73` manual `split('/')` + `fold` — `Path::components()` native.
- `control.rs:25` `thread::spawn` per accept (8MB stack unbounded) — `Builder::stack_size(64K)` or single thread.
- `control.rs:56` `stream.try_clone()` — `BufReader::new(&stream)` no clone.
- `control.rs:92` volume does 2× `current()`+`apply` — single `apply` returning `effective_volume`.
- `control.rs:193` `delete_pack` full `refresh_from_directory()` scan (600ms) — `cache.soundpacks.remove(&id)` O(1).
- `control.rs:235` `rand::choose` for fallback — `ids[now%len]` stdlib, or keep `rand` but one line.
- `control.rs:272` `collect_packs` `contains` O(n²) — delete check (`read_dir` unique).

### Input
- `bootstrap.rs:3` spawns `evdev`+`rdev` both → double sound per key — gate `if evdev_count>0 { evdev } else { rdev }`
- `evdev_input_listener.rs:146` `sleep 20ms` poll 50Hz — `libc::poll` event-driven, -20ms latency, 0% idle vs 1%
- `evdev:24` treats any `supported_keys` device as keyboard (mice) — filter `KEY_A` in set or name contains keyboard
- `evdev:151` + `input_listener.rs:15` duplicate `map_*_keycode` 70+100 lines — share `utils/keycodes::to_code`
- `input_listener.rs:151` `Arc<Mutex<Instant>>`+`HashSet` inside `rdev::listen` `Fn` — `Fn` sequential, use `RefCell`/`Cell`, remove 2 locks/keystroke
- `input_listener.rs:194` listener debounce 1ms/10ms duplicates `engine.rs:281` engine debounce — delete listener layer (drops fast typists)
- `input_listener.rs:186` `ctrl_pressed` mut bool in `Fn` — `Cell<bool>` native

### QML / Python / CI
- `Panel.qml:178,188` `"/bin/sh -c mkdir && printf"` concat — `Quickshell.Io.FileView` native (0 forks, watchChanges)
- `Panel.qml:68` `clearDeleteToast 3s` + `Service.qml:46` `clearImportTimer 4s` + `Panel.qml:231` `clearUpdateTimer 5s` — 3 identical timers → one `ToastTimer`
- `Panel.qml:410` `installPollTimer 500ms*10` — delete (5s open poll + `setupProc.onExited` enough) — -20 forks after install
- `Panel.qml:429` 3 timers (5s open, 30s closed, 5s not-installed fork storm) — one `interval: opened?5000: (!installed?0:30000)`
- `Service.qml:67-95` 4× `notify-send`+`restart()` — `function notify(t,m){…}`
- `Service.qml:128` `restartAssertTimer 3s` double `enable --now` — delete (systemd `Restart=on-failure`)
- `Panel.qml:615+849` update button handlers duplicated — `function doUpdate()`
- `Panel.qml:865` manual `width: parent.width - a - b` spacer — `Row { Layout.fillWidth }` native
- `SearchablePackDropdown.qml:67` `recomputeFiltered` loop — `options.filter(o=>label(o).toLowerCase().includes(q))` JS stdlib
- `SearchablePackDropdown.qml:283` vim `j/k` speculative — delete (`keyNavigationEnabled` covers)
- `scripts/sorakey-import-pack.py:661` `Gtk.FileDialog` 57 lines — `zenity --file-selection` one line, remove `libgtk-4-dev` from `release.yml:25`
- `scripts/sorakey-import-pack.py:464` 3 passes over `zf.namelist()` — single scan
- `relse.md` 263 + `CLEANUP.md` 31 + `note.md` 16 — shrink `relse.md` to 30 lines, link `gh release`

## 3. RAM / speed — measured + target

| pattern | file:line | now | after | saving |
|---|---|---|---|---|
| duplicate `Vec` clone equal-rate | `soundpack_loader.rs:13,69` | 61MB gravastar (clone 24MB) | 37MB | -24MB (done, `Arc`) |
| peak before `malloc_trim` | `soundpack_loader.rs:603` + `engine.rs:697` | 126MB cycling 37→77→108→126 | 37→37→37 (`take+drop` before alloc) | -89MB peak (done) |
| `per_channel` matrix | `soundpack_loader.rs:138` | +22MB transient for 30s pack | 0 | -22MB |
| `sinc 64/32` | `resampler.rs:26` | 2.5s block on `switch_device` | 0.3s with 16/8 | -2.2s stall |
| `thread per connection` | `control.rs:42` | 8MB virtual per socket, unbounded | 64K or 1 thread | -800MB DoS |
| `full scan on delete` | `state/soundpack.rs:204` | 600ms re-read 20 configs | ~1ms `remove` | -599ms |
| `evdev poll 20ms` | `evdev:146` | 50 wake/s, 20ms lag, 1% CPU | `poll` event-driven 0% | -20ms latency |

**Multi-file RAM model (verified):**
- `RAM = sum_unique(duration * 352800)` + ~13MB overhead
- `gravastar-v60 70s` → 24MB +13 = 37MB; `crush80` 2.95s unique → 1.04MB+13=14MB; `epomakers` 2.23s → 0.79MB+13≈14MB; `tiger80` 8×0.18s→0.5MB+13=13MB
- File type (ogg/mp3/wav) irrelevant after decode — disk size ≠ RAM (ffprobe shows ogg 667K→24MB, wav 32K→0.06MB decoded)

## 4. Leaks / retain

- `engine.rs:128` `multi_key_audio` — fixed (`clear` before decode, rebuild on `switch_device`). Keep, add `#[test] switch_device_resamples_multi_or_clears`.
- `control.rs:99` `per_pack_volume` inserts unbounded — add `retain(|k,_| packs.contains(k))` on delete/fallback.
- `state/soundpack.rs:316` `insert_error_metadata` stale — clear on successful load.
- `trace::SINK` unbounded — `bounded(512)` drop oldest.
- `log_buffer` 2000 cap 240KB — ok. `device_manager CACHED_*` never refreshed — wire or delete.

## 5. Plan phases

**Phase A — safe, 1 PR (no behavior change)**
- delete dead crates/consts/fns, `allow(dead_code)`, `DEBUG_ENABLED`, `SoundpackType`/`SoundpackCount` sugar
- unify `V1_KEY_TABLE`/`SMART_DONOR` to `scripts/_v1_table.py`
- dedup `Model.prettyPackName`, delete `importFailed`/`hovered` signals, `optionDescription`
- collapse 3 toast timers→1, 3 poll timers→1, `notify()` helper, `doUpdate()`
- `cargo check` surfaces next batch

Verify: `cargo check`, `cargo test`, `omarchy plugin update` smoke.

**Phase B — audio (RAM/speed)**
- dedup symphonia → `utils/symphonia.rs`
- `Arc` return on rate match, `take` not `clone` in `switch_device`
- `sinc_len 16/8` or `FastFixedIn`, remove `per_channel` matrix, `malloc_trim` behind flag
- inline `per_channel` → interleaved push

Verify: `for id in gravastar crush80 epomakers tiger80 sora; do ctl keyboard_pack; sleep 1; cat /proc/$(pgrep -x sorakey)/status|grep VmRSS; done` → stable 37/14/14/13, no 77→126 climb; `cargo bench` resample 24MB <400ms.

**Phase C — infra**
- `RwLock+Arc<AppConfig>`, `paths` stdlib, `poll` evdev, `Builder::stack_size`, incremental `remove` not full scan, gate `evdev` vs `rdev`
- CI: remove `libgtk-4-dev`, `rustfmt/clippy` from release build

Verify: `ps -T` thread count, hotplug check, `systemd` enable, no double sound per key.

## 6. Questions before edits
- `Gtk.FileDialog` vs `zenity` — keep Wayland native or one-liner?
- Resampler keep `sinc_len 64` or drop to 16 (inaudible for clicks, ABX if needed)?
- `admin-scripts/v1-to-v2-converter.py` keep dev-only or share module and delete?

> Say `go phase A` / `A+B` / `fix RAM first` — then edits.
