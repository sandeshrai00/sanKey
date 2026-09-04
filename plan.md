# Sorakey Full Audit — 2026-09-04

Full audit done — every file read end-to-end (Panel.qml all 1,499 lines,
all QML forks, Service, Model, scripts, udev rule, docs, README, CI),
plus two parallel deep-dives on the Rust daemon and the scripts.

> Note: the previous version of this file claimed Phases 1–7 "✅ done",
> but the code contradicts it in 4+ places (B1/B2/B3 below). That old
> content is superseded by this audit. Line numbers refer to the code
> as of 2026-09-04.

---

## PART 1 — The "AI text" (user-facing copy that reads machine-written)

| # | Location | Current text | Problem → rewrite |
|---|----------|--------------|-------------------|
| 1 | `Panel.qml:1200` | "Sorakey Need Keyboard Permission." | Broken grammar → "Keyboard access needed" |
| 2 | `Panel.qml:1209` | "Sorakey needs keyboard access to play sounds as you type. Just Basic Permission." | "Just Basic Permission." is meaningless filler; "Sorakey" 3× in one banner → "Sorakey listens for key presses to play sounds. One approval grants access to keyboards only." |
| 3 | `Panel.qml:1230` | "Enable Keyboard Sound with terminal" | Drifted from the chosen wording + random Title Case → "Enable keyboard permission with terminal" (sentence case, matches Enable button) |
| 4 | `Panel.qml:1131` | `"> " + lastResult` | Terminal-prompt cosplay in a GUI → drop the `"> "` |
| 5 | `Model.js:9` | `prettyPackName` capitalizes word-starts | "cherrymx-brown-abs" renders "Cherrymx Brown Abs" (one wrong word) → special-case or split known prefixes |
| 6 | `README.md:89-97` | Two adjacent paragraphs saying the same thing about daemon updates | Classic duplication → merge into one |
| 7 | `README.md:32-39` | Still instructs `sudo usermod -aG input $USER` + logout | Stale and wrong — contradicts the new one-tap approval flow; rewrite Install §2 |
| 8 | `README.md:114` + panel | README says "Import pack…", panel says "Import Sound" | Pick one name everywhere |
| 9 | `docs/keyboard-access.md:53,36` | Says panel offers "**Fix in terminal**" + mentions a "Retry button" | Neither exists anymore → sync with the current button |
| 10 | `sorakey-enable-capture.sh:79,85,113` | "Ready — type to hear it." / "Keyboard permission needed for sounds." / "Done — type to hear it." | Robotic triplet → plain sentences ("Ready. Type to hear sounds.") |
| 11 | `admin-scripts/DEV_SYNC.md:41` | `ctl '{"status":{}}'` | Dead protocol; working form is `{"cmd":"status"}` (verified live) |

---

## PART 2 — Bugs verified in the QML/docs layer

| # | Bug | File:line | Severity |
|---|-----|-----------|----------|
| B1 | Mute-switch binding still wrong: `checked: running && !muted` shows the wrong state when stopped (old plan 6.3 claimed fixed — it isn't) | `Panel.qml:909` | major |
| B2 | Nerd-Font PUA glyphs still everywhere (old plan 6.1 claimed fixed; only ✕/⚠ were): settings ``, back ``, export ``, update ``, uninstall ``, both chevrons `` | `Panel.qml:926,949,1089,1104,1141`, `SoraDropdown.qml:141`, `SearchablePackDropdown.qml:139` | major (tofu without Nerd Font) |
| B3 | Unquoted `$HOME` in `sh -c` for start/stop/restart flag (old plan 3.5 claimed fixed via ctl — it isn't; space-paths break it silently) | `Panel.qml:246,252,258` | major |
| B4 | `ctlProc` detects errors via substring `"ok":false` — daemon JSON with a space (`"ok": false`) slips through silently | `Panel.qml:635` | minor |
| B5 | Delete-result detected via substring `"deleted"` — any error containing that word fakes a success toast | `Panel.qml:645` | minor |
| B6 | Update success shows the raw last CLI line verbatim | `Panel.qml:387` | minor |
| B7 | `Qt.fileExists` is not a function + `stoppedFlag` frozen at creation (known journal warning) | `Service.qml:28` | minor |
| B8 | `currentLabel()` returns raw ids (`keyboard/...`) when value isn't in options | `SoraDropdown.qml:129`, `SearchablePackDropdown.qml:127` | minor |
| B9 | Dropdown parent `MouseArea` shadows the inner one (works today, fragile duplication) | `Panel.qml:985,1059` + forks | note only |
| B10 | Installed plugin ships junk: rsync excludes miss `admin-scripts/`, `.github/`, `__pycache__/`; `cp` fallback ships literally everything incl. `.git` | `admin-scripts/dev-sync.sh:28-42` | minor |
| B11 | `plan.md` / `relse.md` (typo for release.md) / `note.md` ship in every git clone; `admin-scripts/README.md` documents a converter that doesn't exist | repo root + `admin-scripts/README.md:18` | minor (hygiene) |

---

## PART 3 — Daemon bugs (deep audit, worst first)

Same failure class as the original silent bug is still present:

- `engine.rs:308-332` — `Sink::try_new` failure ignored, `audio_error` never set → unplug = `audio_ok:true`, silent by design. **Major.**
- `engine.rs:154-166` — fallback keeps failed device id, reports success; total failure calls `process::exit(1)` with no health line. **Major.**
- `engine.rs:611-639` — no device watchdog → unplug stays quiet forever. **Major.**
- `health.rs:11` — `AUDIO_OK=true` initially → pre-engine `status` lies. **Major.**

Wrong behavior: delete-last-pack never clears the engine pack — deleted
audio keeps playing (`control.rs:342-372`); `select_device` stores any
string, validates nothing, still returns ok (`411-428`); `reset_volume`
ignores global and returns 100 (`278`); master `volume` can never change
the global default while a pack is set (`102-118`); key allowlist rejects
F13+, media, NumpadEnter/Divide the converter supports (`164-201`);
`load_pack` returns ok before the async load (`221-254`); `ctl_client`
prints empty output with exit 0 on server drop (`510-534`).

Memory/crash: `key_pressed` map grows unbounded per unique IPC string
(`engine.rs:186-202`); symphonia trusts header `n_frames` → malicious
file OOM (`symphonia.rs:37-45`) — **critical**; control socket is
thread-per-connection on a 64K stack with unbounded `read_line`
(`control.rs:31-66`).

Input: keyboard detection requires `KEY_A` — A-less numpads/macropads
invisible (`evdev:122,172`); key-hold autorepeat (`value==2`) ignored
(`26-77`); Ctrl on kb1 + M on kb2 never fires the hotkey.

Data: migration typo `red-pbt → red-abs` (`config.rs:165`);
`volume==1.0→0.6` migration eats an explicit user max (`191-195`); one bad
`per_pack_volume` entry drops the whole table (`29-64`); V1 converter
nondeterminism (first-file-missing rate, press/release overwrite,
non-atomic writes, wrong Pause code `58437`); validator accepts empty `{}`
as Valid; zero-files-loaded pack reports `loaded:true`.

Privacy: key identities leak into `export_logs` (`control.rs:469-478`);
`Current user:` logged raw (`evdev:17`).

Lifecycle: socket-bind failure ignored → headless daemon reporting
`running:true` (`main.rs:43-48`); `thread::park()` with no supervision.

---

## PART 4 — Script/packaging bugs (worst first)

- **CRITICAL** `uninstall.sh:22` — unquoted `rm -rf ~/.local/...`: a space in `$HOME` word-splits into deleting the wrong paths.
- **CRITICAL** `sorakey-enable-capture.sh:89` (+`60`) — `$SRC` interpolated single-quoted into `pkexec/sudo bash -c`: a `'` in the path = root command injection.
- **CRITICAL** `sorakey-import-pack.py:411` — predictable `install_dir+".tmp."+pid` + `makedirs(exist_ok=True)` follows symlinks.
- **Major:** import size-check runs *after* full-file read (zip-bomb OOM); `endswith("config.json")` matches `myconfig.json`; `__MACOSX` can win; `strip_enclosing_folder` order-dependent; validates `defs` but installs `definitions` (empty pack passes); destructive `rmtree` overwrite, no backup; daemon-controlled export filename unsanitized (`..` traversal); Gtk4-missing raises uncaught `ValueError`; hardcoded `~/.local/bin/sorakey` + last-line-is-JSON assumption; setup uses `cp -rn` (packs never update) and greps daemon JSON by string; sudo-branch stderr lost → real errors misreported as "dismissed"; error classifier misses `Not authorized`/`Authorization failed`; `build-sorakey.sh` hash breaks on spaces, never cross-checks `Cargo.toml`; `release.yml` installs clippy but never runs `cargo test`/clippy, never gates the daemon version, all actions floating unpinned.

---

## Fix plan

- **Phase A — safety (first, no behavior change):** quote `uninstall.sh:22-23,27-31` (`"$HOME/…"`); injection-proof the pkexec/sudo snippet (argv-passing, not quoting); `mkdtemp` for import staging; sanitize export filename; move import size-check before read.
- **Phase B — copy:** all 11 AI-text items with the rewrites above; sync README install/update/import sections; archive `plan.md`/`relse.md`/`note.md` out of the clone.
- **Phase C — panel truth:** mute-switch binding (`checked: muted, enabled: running`); PUA→emoji/text glyphs; replace `sh -c` flag writes with argv `rm`/`mkdir`; robust `ok:false` parse.
- **Phase D — daemon truth:** sink-failure → `audio_error`; `select_device` validation; delete-last-pack clears engine; volume/reset semantics; extended key allowlist; `AUDIO_OK` initial false.
- **Phase E — daemon hardening:** socket caps, symphonia limits, KEY_A detect fix, autorepeat, migration typo, per-entry config tolerance, bind-fail exit.
- **Phase F — CI:** run `cargo test` + clippy, gate `Cargo.toml` version, pin actions.

**Release note:** Phases A–C are QML/scripts-only (no release needed);
D–E touch `daemon/` → bump to `v0.1.2` per the release rule
(`relse.md`: any `daemon/` change must ship a new tag or users build
from source).
