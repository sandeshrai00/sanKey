#!/usr/bin/env python3
"""Convert V1 soundpack config to V2.

Usage: python3 v1-to-v2-converter.py /path/to/pack/config.json
Writes configv2.json next to the original.
"""

import json
import os
import re
import subprocess
import sys
import tempfile


import importlib.util as _ilu, pathlib as _pl2
_spec = _ilu.spec_from_file_location("_v1_shared", str((_pl2.Path(__file__).parent.parent / "scripts" / "_v1_shared.py").resolve()))
_mod = _ilu.module_from_spec(_spec); _spec.loader.exec_module(_mod)
V1_KEY_TABLE = _mod.V1_KEY_TABLE
SMART_DONOR = _mod.SMART_DONOR
_fill_missing_keys = _mod._fill_missing_keys


def is_v1_config(cfg):
    """V1 config has no definition_method."""
    if not isinstance(cfg, dict):
        return False
    if "definition_method" in cfg or "definitions" in cfg:
        return False
    return "defines" in cfg


def is_v1_multi(defines):
    """Multi-method V1 — string paths per key."""
    if not defines:
        return False
    string_values = sum(1 for v in defines.values() if isinstance(v, str) and v)
    return string_values > sum(1 for v in defines.values() if isinstance(v, list))


def get_audio_duration_ms(audio_path):
    """Audio duration in ms — ffprobe or wave fallback."""
    if not os.path.exists(audio_path):
        return None

    # check WAV
    try:
        with open(audio_path, "rb") as f:
            header = f.read(12)
        is_wav = header[:4] == b'RIFF' and header[8:12] == b'WAVE'
    except Exception:
        is_wav = False

    if is_wav:
        try:
            import wave as wave_module
            with wave_module.open(audio_path) as w:
                return w.getnframes() / w.getframerate() * 1000.0
        except Exception:
            pass

    # ffprobe
    try:
        result = subprocess.run(
            ["ffprobe", "-v", "quiet", "-print_format", "json",
             "-show_format", audio_path],
            capture_output=True, text=True, timeout=15
        )
        d = json.loads(result.stdout)
        return float(d["format"]["duration"]) * 1000.0
    except Exception:
        return None


SMART_DONOR = {
    "MetaLeft": ["CapsLock", "ControlLeft", "AltLeft", "KeyA"],
    "MetaRight": ["CapsLock", "ControlLeft", "AltLeft", "KeyA"],
    "ContextMenu": ["CapsLock", "ControlLeft", "KeyA"],
    "AltRight": ["AltLeft", "ControlLeft", "KeyA"],
    "ControlRight": ["ControlLeft", "ShiftLeft", "KeyA"],
    "PrintScreen": ["Escape", "F12", "KeyA"],
    "ScrollLock": ["Escape", "CapsLock", "KeyA"],
    "Pause": ["Escape", "CapsLock", "KeyA"],
    "Insert": ["Backspace", "Delete", "KeyA"],
    "Delete": ["Backspace", "Insert", "KeyA"],
    "Home": ["PageUp", "ArrowUp", "Backspace", "KeyA"],
    "End": ["PageDown", "ArrowDown", "Enter", "KeyA"],
    "PageUp": ["Home", "ArrowUp", "KeyA"],
    "PageDown": ["End", "ArrowDown", "KeyA"],
    "ArrowUp": ["ArrowDown", "Space", "KeyA"],
    "ArrowDown": ["ArrowUp", "Space", "KeyA"],
    "ArrowLeft": ["ArrowRight", "Space", "KeyA"],
    "ArrowRight": ["ArrowLeft", "Space", "KeyA"],
    "Power": ["Escape", "KeyA"],
    "Sleep": ["Escape", "KeyA"],
    "WakeUp": ["Escape", "KeyA"],
    "NumLock": ["CapsLock", "KeyA"],
    "Clear": ["CapsLock", "KeyA"],
    "Numpad0": ["KeyA"], "Numpad1": ["KeyA"], "Numpad2": ["KeyA"],
    "Numpad3": ["KeyA"], "Numpad4": ["KeyA"], "Numpad5": ["KeyA"],
    "Numpad6": ["KeyA"], "Numpad7": ["KeyA"], "Numpad8": ["KeyA"],
    "Numpad9": ["KeyA"], "NumpadDecimal": ["KeyA"], "NumpadAdd": ["KeyA"],
    "NumpadSubtract": ["KeyA"], "NumpadMultiply": ["KeyA"],
    "NumpadDivide": ["KeyA"], "NumpadEnter": ["KeyA"],
    "NumpadEquals": ["KeyA"], "NumpadComma": ["KeyA"],
    "AudioVolumeMute": ["Space", "Enter", "KeyA"],
    "AudioVolumeDown": ["Space", "Enter", "KeyA"],
    "AudioVolumeUp": ["Space", "Enter", "KeyA"],
    "MediaTrackPrevious": ["Space", "Enter", "KeyA"],
    "MediaTrackNext": ["Space", "Enter", "KeyA"],
    "MediaPlayPause": ["Space", "Enter", "KeyA"],
    "MediaStop": ["Space", "Enter", "KeyA"],
    "MediaSelect": ["Space", "Enter", "KeyA"],
    "LaunchApp1": ["Space", "Enter", "KeyA"],
    "LaunchApp2": ["Space", "Enter", "KeyA"],
    "LaunchApp3": ["Space", "Enter", "KeyA"],
    "LaunchMail": ["Space", "Enter", "KeyA"],
    "BrowserHome": ["Space", "Enter", "KeyA"],
    "BrowserSearch": ["Space", "Enter", "KeyA"],
    "BrowserFavorites": ["Space", "Enter", "KeyA"],
    "BrowserRefresh": ["Space", "Enter", "KeyA"],
    "BrowserStop": ["Space", "Enter", "KeyA"],
    "BrowserForward": ["Space", "Enter", "KeyA"],
    "BrowserBack": ["Space", "Enter", "KeyA"],
}




def convert_v1_to_v2(cfg, pack_dir):
    """Convert V1 dict to V2 — needs pack_dir for duration checks."""
    defines = cfg.pop("defines", {})
    sound = cfg.pop("sound", None)

    definitions = {}

    if is_v1_multi(defines):
        per_key_files = {}
        for code, filename in defines.items():
            if not isinstance(filename, str) or not filename or filename.strip().lower() == "null":
                continue
            w3c_name = V1_KEY_TABLE.get(str(code))
            if not w3c_name:
                continue
            per_key_files[w3c_name] = filename.strip()

        unique_files = set(per_key_files.values())

        # check for shared file
        chosen_audio = None
        for f in unique_files:
            path = os.path.join(pack_dir, f)
            if os.path.exists(path):
                chosen_audio = f
                break

        if len(unique_files) == 1 and chosen_audio:
            # single shared file
            dur = get_audio_duration_ms(os.path.join(pack_dir, chosen_audio))
            duration_ms = dur if dur else 100.0
            for w3c_name in per_key_files:
                definitions[w3c_name] = {"timing": [[0.0, duration_ms]]}
            definition_method = "single"
            audio_file = chosen_audio
        else:
            file_durations = {}
            for fname in unique_files:
                dur = get_audio_duration_ms(os.path.join(pack_dir, fname))
                file_durations[fname] = dur if dur else 100.0

            for w3c_name, filename in per_key_files.items():
                dur = file_durations.get(filename, 100.0)
                definitions[w3c_name] = {
                    "timing": [[0.0, dur]],
                    "audio_file": filename,
                }
            definition_method = "multi"
            audio_file = None

    else:
        # single-method sprite regions
        for code, timing in defines.items():
            w3c_name = V1_KEY_TABLE.get(str(code))
            if not w3c_name:
                continue
            if isinstance(timing, list) and len(timing) == 2 and all(
                isinstance(x, (int, float)) for x in timing
            ):
                start = float(timing[0])
                duration = float(timing[1])
                end = start + duration
                definitions[w3c_name] = {"timing": [[start, end]]}

        definition_method = "single"
        audio_file = sound if sound else None

    _fill_missing_keys(definitions)

    new_cfg = {
        "id": cfg.get("id") or f"imported-{os.path.basename(pack_dir)}",
        "name": cfg.get("name") or os.path.basename(pack_dir),
        "author": cfg.get("author") or cfg.get("m_author") or "Unknown",
        "config_version": "2",
        "definition_method": definition_method,
        "definitions": definitions,
        "soundpack_type": "Keyboard",
    }
    if audio_file:
        new_cfg["audio_file"] = audio_file
    if cfg.get("version"):
        new_cfg["version"] = cfg["version"]
    if cfg.get("description"):
        new_cfg["description"] = cfg["description"]
    if cfg.get("tags"):
        new_cfg["tags"] = cfg["tags"]

    return new_cfg


def main():
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} /path/to/pack/config.json", file=sys.stderr)
        sys.exit(1)

    config_path = sys.argv[1]

    if not os.path.isfile(config_path):
        print(f"ERROR: File not found: {config_path}", file=sys.stderr)
        sys.exit(1)

    pack_dir = os.path.dirname(os.path.abspath(config_path))

    try:
        with open(config_path, "r") as f:
            cfg = json.load(f)
    except json.JSONDecodeError as e:
        print(f"ERROR: Invalid JSON in config.json: {e}", file=sys.stderr)
        sys.exit(1)

    if not is_v1_config(cfg):
        print("ERROR: Already a V2 config (has definition_method field) or missing 'defines' field.")
        sys.exit(1)

    print(f"Converting V1 pack: {cfg.get('name', 'unnamed')}")
    print(f"  key_define_type: {cfg.get('key_define_type', 'single')}")
    print(f"  keys in defines: {len(cfg.get('defines', {}))}")
    print(f"  Pack directory: {pack_dir}")

    v2_cfg = convert_v1_to_v2(cfg, pack_dir)

    output_path = os.path.join(pack_dir, "configv2.json")
    with open(output_path, "w") as f:
        json.dump(v2_cfg, f, indent=4)
        f.write("\n")

    print(f"\n✅ Wrote configv2.json ({len(v2_cfg['definitions'])} key definitions)")
    print(f"   definition_method: {v2_cfg['definition_method']}")
    print(f"   audio_file: {v2_cfg.get('audio_file', 'per-key (multi-method)')}")
    print(f"   Output: {output_path}")

    # sample mappings
    sample_keys = list(v2_cfg["definitions"].keys())[:6]
    print(f"\n   Sample mappings:")
    for k in sample_keys:
        print(f"     {k}: {v2_cfg['definitions'][k]}")


if __name__ == "__main__":
    main()
