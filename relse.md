# Sorakey release guide (copy-paste ready)

Repo: github.com/sandeshrai00/soraKey
Binary built by CI: `sorakey-x86_64` (Rust 1.87, pinned in rust-toolchain.toml)

---

## RULE: when do I need a new release?

The release exists to ship the compiled **daemon binary** (`sorakey-x86_64`).
The freshness check in the shell (`scripts/build-sorakey.sh`) only accepts the
GitHub prebuilt when **the tagged commit's daemon source == current daemon
source**. If not, every user compiles from source (slow).

| What you changed | New release? |
|---|---|
| anything in `daemon/` (any `.rs`, `Cargo.toml`) | **YES — must release** |
| `rust-toolchain.toml` | **YES — must release** |
| only `Panel.qml`, `Service.qml`, `Model.js`, `scripts/*.py`, README, note.md | NO — plugin files ship from the repo, users get them with `omarchy plugin update` |

Check what has changed since the last tag:

```bash
git log --oneline v0.1.2..HEAD        # replace v0.1.2 with your last tag
git diff --name-only v0.1.2..HEAD -- daemon rust-toolchain.toml   # non-empty = release needed
```

---

## Steps to release a new version (e.g. 0.1.1 -> 0.1.2)

### 1. Bump the version in BOTH files (they must match)

`manifest.json`:

```json
  "version": "0.1.2",
```

`daemon/Cargo.toml`:

```toml
version = "0.1.2"
```

CI compares the tag name with `manifest.json:version`. If they disagree the
release run fails at "Validate release identity" and no release is created.

### 2. Commit the bump

```bash
git add manifest.json daemon/Cargo.toml
git commit -m "bump 0.1.2"
```

### 3. Tag + push (this triggers CI)

```bash
git tag v0.1.2
git push origin main --tags
```

Tag name is exactly `v` + version: `v0.1.2`, not `0.1.2`, not `v1.2`.

### 4. Watch CI (~3-5 min)

Open: https://github.com/sandeshrai00/soraKey/actions
Workflow name: `release-sorakey`. It:

1. validates tag == manifest version
2. builds `sorakey-x86_64` from the tagged commit
3. writes `SHA256SUMS`
4. attests the binary (provenance)
5. creates **Release v0.1.2** with both files attached

### 5. Verify the release landed

Open: https://github.com/sandeshrai00/soraKey/releases

You must see `v0.1.2` with exactly 2 assets:
`sorakey-x86_64` and `SHA256SUMS`.

### 6. Verify on your machine

```bash
omarchy restart shell
sleep 5
journalctl --user --since "1 min ago" | grep -i sorakey
```

Expected: `sorakey freshness: ... up to date` (installed binary already matches)
or `Installed verified prebuilt 0.1.2` (prebuilt downloaded).
If you see `building from source` — something is wrong (tag/source mismatch).

---

## Writing the release description

CI auto-generates the description from your commit titles
(`generate_release_notes: true`) — `feat:`/`fix:` lines show up automatically.
That is often enough. To edit it:

1. GitHub → **Releases** → click the release (e.g. v0.1.2)
2. Click the pencil icon (Edit) next to the title
3. Replace the body, keep the title as `v0.1.2`
4. **Update**

Copy-paste template:

```markdown
## What's new
- (one line per user-visible change, from your commit subjects)

## Install / update
- Existing users: `omarchy plugin update` (or restart the shell) — the
  verified prebuilt binary downloads automatically.
- Fresh install: `omarchy plugin add https://github.com/sandeshrai00/soraKey.git`

## Assets
- `sorakey-x86_64` — prebuilt daemon (x86_64 Linux, Rust 1.87)
- `SHA256SUMS` — checksum for the binary (verified + attested by CI)
```

Example body (what 0.1.2 would say):

```markdown
## What's new
- Per-pack keyboard volume with recommended volume
- Pack search + delete inside the dropdown
- Import: clear error messages (names the missing file), 20MB size limit
- Import: panel closes for the file dialog, result arrives as notification

## Install / update
- Existing users: `omarchy plugin update` (or restart the shell) — the
  verified prebuilt binary downloads automatically.
- Fresh install: `omarchy plugin add https://github.com/sandeshrai00/soraKey.git`

## Assets
- `sorakey-x86_64` — prebuilt daemon (x86_64 Linux, Rust 1.87)
- `SHA256SUMS` — checksum for the binary (verified + attested by CI)
```

To list what changed since the last release (for "What's new"):

```bash
git log --oneline v0.1.1..HEAD
```

---

## Fixing mistakes

**CI failed at "Validate release identity"** (tag/version mismatch):
fix the version, commit, delete the tag, re-tag, re-push:

```bash
git tag -d v0.1.2
git push origin :refs/tags/v0.1.2
git tag v0.1.2
git push origin --tags
```

**You committed daemon changes AFTER tagging** (common):
the prebuilt is now stale — users build from source until you release again.
Just bump to the next version and repeat the steps.

**You want to change the release description**: pencil icon (see above).
Release files themselves can NOT be replaced — a new binary means a new tag.

**Reusing a version number is normally impossible** (tags are permanent —
only exception: you deleted the tag first, see "Deleting a release" below).
Always bump.

---

## Pre-release (unstable build)

A pre-release is a normal release **marked** "pre-release". It still
downloads the same way (the freshness check uses the same URL), but on
GitHub it sits under *Pre-releases* instead of being the default latest
release. Use it for experimental daemon changes you want testable but not
final.

Good thing: pre-release → final is just a checkbox — **no new tag needed**.

After CI created the release, do one of:

**GitHub UI:**
1. Releases page → open the release (e.g. v0.1.3)
2. Pencil icon (Edit)
3. Check **"This is a pre-release"**
4. **Update**

**Terminal (needs `gh auth login`):**

```bash
gh release edit v0.1.3 --prerelease
```

**Make it final again** (uncheck in the same UI, or):

```bash
gh release edit v0.1.3 --prerelease=false
```

Note: CI (`release.yml`) always creates a NORMAL release — the pre-release
mark is your manual step after it runs. Don't add `prerelease: true` to the
workflow, that would make every release a pre-release.

---

## Deleting a release (last resort)

**Warning:** deleting a release removes the binary. Every user on that
version falls back to compiling from source until you ship a replacement.
If the mistake is only the description — don't delete, just edit the
description (pencil icon).

**GitHub UI:**
1. Releases page → open the release
2. Scroll to the very bottom → **"Delete release v0.1.2"** → confirm

**Terminal (needs `gh auth login`):**

```bash
# delete release, keep tag:
gh release delete v0.1.2 --yes

# delete release AND tag:
gh release delete v0.1.2 --cleanup-tag
```

**Tag only** (release already gone, or you want to clean the tag):

```bash
git tag -d v0.1.2
git push origin :refs/tags/v0.1.2
```

**Re-releasing the same version after a full delete** (tag is gone, so
re-tagging is allowed):

```bash
git tag v0.1.2
git push origin --tags
```
CI runs again and recreates the release.

---

## Quick sanity checks (paste anytime)

```bash
# versions agree? (both lines must print the same number)
python3 -c 'import json; print(json.load(open("manifest.json"))["version"])'
grep -m1 '^version' daemon/Cargo.toml

# do I need a release? (any output = yes)
git diff --name-only $(git describe --tags --abbrev=0)..HEAD -- daemon rust-toolchain.toml

# what will the next release contain?
git log --oneline $(git describe --tags --abbrev=0)..HEAD
```