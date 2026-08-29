# Sankey — Future: GitHub, Updates & Versioning

This doc is a **future plan only**. Today Sankey is local-only (no `.git`, no remote). When we publish to GitHub, this is how updates and versioning will work.

---

## 1. Current State (Aug 2026)

- `~/Work/Oursankey` has **no `.git`** — intentionally deleted, VSCodium shows no Source Control.
- Installed copy: `~/.config/omarchy/plugins/io.github.sanman.sankey/` (manual `cp -r`).
- Updates today: manual recopy + `bin/sankey-setup` + `pkill -x quickshell`.
- `omarchy plugin update` skips Sankey — it is **unmanaged** (not a git checkout).

This is intentional while the mouse system is being removed and the plugin is stabilizing locally.

---

## 2. How Omarchy Plugin Updates Work (verified from `Omarchy-Spotify-main`)

- **Install (git-managed):** `omarchy plugin add https://github.com/<user>/<repo>.git --enable` clones the repo to `~/.config/omarchy/plugins/<id>/` with `.git`.
- **Update:** `omarchy plugin update <id> --yes` (or `omarchy plugin update --yes` for all) is `git pull` inside each plugin directory. No background auto-update — user or `omarchy update` triggers it.
- **Manifest version** (`manifest.json:version`, Spotify uses `1.0.3`) is display/metadata only, not enforced by Omarchy. The shell just reloads the new QML after update.

## 3. How Omarchy-Spotify Handles Its Rust Backend (pattern to copy)

- `scripts/backend-source-id.sh` hashes `backend/Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `backend/src/*.rs` → single SHA256.
- `scripts/build-backend.sh` stores it in `~/.local/lib/omarchy-spotify/backend-source.sha256` + binary hash. Skips rebuild if hashes match (`backend_install_is_current()`).
- Tries **verified download**: needs clean git (`git status --porcelain` empty), gets commit SHA (`git rev-parse HEAD`) and version from `manifest.json`, then downloads `omarchy-spotify-backend-<arch>` from GitHub Releases — only if GitHub attestation proves the artifact was built from that exact commit via `release-backend.yml` (checked with `gh` + `sha256sum`).
- **Fallback:** builds locally with `cargo build --release` in `XDG_CACHE/omarchy-spotify/target` (outside plugin dir to avoid hot-reload loop), then atomically installs to `~/.local/lib/omarchy-spotify/omarchy-spotify-backend`.
- `scripts/setup.sh` installs systemd units, writes `~/.config/omarchy-spotify/spotifyd.conf` (600), `daemon-reload`, restarts only if it was active and something changed.

---

## 4. Proposed Sankey Version System (when on GitHub)

- **Single source of truth:** `manifest.json:version` (today `1.0.0`). Keep `daemon/Cargo.toml:version` in sync — bump both together.
- **Semver:** `MAJOR` = breaking (socket API `ctl` shape, config path), `MINOR` = feature (new pack, new `ctl` cmd, new panel control), `PATCH` = fix (build, fade, Input group note).
- **CHANGELOG.md:** Add file like Spotify's `CHANGELOG.md` — one heading per version, date, bullet list. Bump version and changelog in same commit.
- **Optional hash gate:** Add `bin/sankey-source-id.sh` hashing `daemon/Cargo.toml`, `Cargo.lock`, `daemon/src/**/*.rs`, `Panel.qml`, `Service.qml`, `Model.js`, `manifest.json`, `bin/*`. Store in `~/.local/share/sankey/.source.sha256` + binary hash in `~/.local/bin/.sankeyd.sha256`. `bin/sankey-setup` skips `cargo build` if hashes match — makes `omarchy plugin update` fast when only QML changed.

## 5. Proposed Update Flow (when on GitHub)

### Publish (maintainer)

```bash
cd ~/Work/Oursankey
git init
git add . && git commit -m "chore: init Sankey 1.0.0 (keyboard-only)"
gh repo create sanman/Oursankey --private --source=. --push   # or: git remote add origin https://github.com/sanman/Oursankey.git && git push -u origin main
git tag v1.0.0 && git push --tags
gh release create v1.0.0 --generate-notes   # or manual Release
```

Target repo: `github.com/sanman/Oursankey` (matches `manifest.json:id io.github.sanman.sankey`). If your GitHub handle differs, replace `sanman`.

### User install (git-managed)

```bash
omarchy plugin add https://github.com/sanman/Oursankey.git --enable
# click Install Sankey in panel, or: ~/.config/omarchy/plugins/io.github.sanman.sankey/bin/sankey-setup
```

Private repo needs `gh auth login` or credential helper; public needs nothing.

### User update

```bash
omarchy plugin update io.github.sanman.sankey --yes
~/.config/omarchy/plugins/io.github.sanman.sankey/bin/sankey-setup   # rebuilds only if hash changed
# or wrapper: bin/sankey-update  (does pull + setup + systemctl --user restart sankey + omarchy restart shell if QML changed)
```

Could also make `sankey-setup` idempotent: check source hash at top, skip build if current, like Spotify's `backend_install_is_current()`.

## 6. Migration Checklist (day we push)

- [ ] `git init` in `~/Work/Oursankey`, `git add .`, commit.
- [ ] Update `README.md` Install section: replace "Local-only repo" with `omarchy plugin add https://github.com/sanman/Oursankey.git --enable`.
- [ ] Add `CHANGELOG.md` with `1.0.0` entry.
- [ ] Ensure `.gitignore` keeps `daemon/target/`, `target/`, `build/`, `dist/`, `*.log` (already does).
- [ ] `omarchy plugin validate ~/Work/Oursankey` passes.
- [ ] Test fresh install on clean user: `omarchy plugin add <url> --enable` → Install Sankey → sound works.
- [ ] Tag and Release.

## 7. What NOT to Do

- Don't `git add daemon/target/` — it is build output.
- Don't bump version in only one of `manifest.json` / `daemon/Cargo.toml`.
- Don't auto-enable systemd unit at login without user opt-in (keep current `enable --now`).
- Don't rely on upstream `mechvibes-dx` GitHub link for installs — keep it only as attribution in `README.md:10` and `LICENSE`.

---

*Private vs public:* Keep repo **private** while iterating (needs auth for `omarchy plugin add`). Flip to public when you want community packs — no code change, just `gh repo edit --visibility public`.
