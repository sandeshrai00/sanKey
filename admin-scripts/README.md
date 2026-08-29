# Admin Scripts

## V1 → V2 Converter

Converts V1 soundpack config files to V2 format.

### Usage

```bash
python3 v1-to-v2-converter.py /path/to/pack/config.json
```

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
