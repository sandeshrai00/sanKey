# Admin Scripts

## dev-sync — local test without push

Sync dev repo → installed plugin, validate, restart shell. See `DEV_SYNC.md`.

```bash
./admin-scripts/dev-sync.sh
```

## V1 → V2 conversion

V1 packs are converted automatically on import (`scripts/sorakey-import-pack.py`,
shared table in `scripts/_v1_shared.py`, mirrored by the daemon's
`daemon/src/utils/config_converter.rs`). There is no standalone converter
script — the section below is obsolete and kept only for reference.

### (Obsolete) standalone converter notes

The script reads the V1 `config.json` and writes `configv2.json` to the **same directory**.
Audio files are expected in the same directory as `config.json`.

### What it does

- **Single-method packs** (`key_define_type: "single"`): V1 sprite sheets with `[start, duration]` per key → V2 with `[start, start+duration]`
- **Multi-method packs** (`key_define_type: "multi"`): V1 per-key audio files → V2 with per-key audio file references and actual durations from `ffprobe`
- Uses the authoritative IOHook keycode mapping from `mechvibes-dx-new`
- Reads audio durations via `ffprobe` (MP3/OGG/FLAC) or stdlib `wave` (WAV)

### Requirements

- Python 3
- `ffprobe` (from ffmpeg) — for non-WAV audio duration
- stdlib `wave` module — for WAV duration fallback

### Example

```bash
# Convert a V1 pack
python3 v1-to-v2-converter.py ~/Downloads/my-pack/config.json

# Result: ~/Downloads/my-pack/configv2.json created
```
