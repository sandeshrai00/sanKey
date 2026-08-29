#!/usr/bin/env python3
"""
Standalone V1 → V2 soundpack config converter.

Usage:
    python3 v1-to-v2-converter.py /path/to/pack/config.json

Reads the V1 config.json, converts it to V2 format, and writes configv2.json
to the same directory. Audio files are expected in the same directory as config.json.

No installation required — pure stdlib + ffprobe.
"""

import json
import os
import re
import subprocess
import sys
import tempfile


# ---- IOHook VC_* -> W3C key names ----
# Directly mirrors mechvibes-dx-new's create_iohook_to_web_key_mapping()
# (config_converter.rs lines 993-1180)
V1_KEY_TABLE = {
    "1": "Escape",
    "2": "Digit1", "3": "Digit2", "4": "Digit3", "5": "Digit4",
    "6": "Digit5", "7": "Digit6", "8": "Digit7", "9": "Digit8",
    "10": "Digit9", "11": "Digit0",
    "12": "Minus", "13": "Equal",
    "14": "Backspace",
    "15": "Tab",
    "16": "KeyQ", "17": "KeyW", "18": "KeyE", "19": "KeyR",
    "20": "KeyT", "21": "KeyY", "22": "KeyU", "23": "KeyI",
    "24": "KeyO", "25": "KeyP",
    "26": "BracketLeft", "27": "BracketRight",
    "28": "Enter",
    "29": "ControlLeft",
    "30": "KeyA", "31": "KeyS", "32": "KeyD", "33": "KeyF",
    "34": "KeyG", "35": "KeyH", "36": "KeyJ", "37": "KeyK",
    "38": "KeyL",
    "39": "Semicolon", "40": "Quote", "41": "Backquote",
    "42": "ShiftLeft",
    "43": "Backslash",
    "44": "KeyZ", "45": "KeyX", "46": "KeyC", "47": "KeyV",
    "48": "KeyB", "49": "KeyN", "50": "KeyM",
    "51": "Comma", "52": "Period", "53": "Slash",
    "54": "ShiftRight",
    "55": "NumpadMultiply",
    "56": "AltLeft",
    "57": "Space",
    "58": "CapsLock",
    "59": "F1", "60": "F2", "61": "F3", "62": "F4",
    "63": "F5", "64": "F6", "65": "F7", "66": "F8",
    "67": "F9", "68": "F10",
    "69": "NumLock", "70": "ScrollLock",
    "71": "Numpad7", "72": "Numpad8", "73": "Numpad9",
    "74": "NumpadSubtract",
    "75": "Numpad4", "76": "Numpad5", "77": "Numpad6",
    "78": "NumpadAdd",
    "79": "Numpad1", "80": "Numpad2", "81": "Numpad3",
    "82": "Numpad0",
    "83": "NumpadDecimal",
    "87": "F11", "88": "F12",
    "91": "F13", "92": "F14", "93": "F15",
    "99": "F16", "100": "F17", "101": "F18", "102": "F19",
    "103": "F20", "104": "F21", "105": "F22", "106": "F23",
    "107": "F24",
    "112": "Convert", "115": "Lang1", "119": "Lang2",
    "121": "KanaMode", "123": "HiraganaKatakana",
    "125": "IntlYen", "126": "NumpadComma",
    # Alternative range (some iohook implementations use different scancode values).
    # These are OVERRIDDEN by the standard codes below — do not reorder above them.
    "3597": "NumLock",
    "3612": "NumpadDivide",
    "3613": "NumpadMultiply",
    "3639": "Numpad7",
    "3640": "Numpad8",
    "3653": "Numpad9",
    "3655": "NumpadAdd",
    "3657": "Numpad4",
    "3663": "Numpad5",
    "3665": "Numpad6",
    "3666": "Numpad1",
    "3667": "Numpad2",
    "58444": "Clear",
    "58470": "IntlBackslash",
    # ---- Standard iohook codes (override alternative range above) ----
    "3637": "NumpadDivide",
    "3612": "NumpadEnter",
    "3597": "ControlRight",
    "3613": "ControlRight",
    "3640": "AltRight",
    "3645": "NumpadEquals",
    "3675": "NumpadDecimal",
    "3676": "Numpad0",
    "57399": "PrintScreen",
    "57415": "Home",
    "57416": "ArrowUp",
    "57417": "PageUp",
    "57419": "ArrowLeft",
    "57421": "ArrowRight",
    "57423": "End",
    "57424": "ArrowDown",
    "57425": "PageDown",
    "57426": "Insert",
    "57427": "Delete",
    "57400": "AltRight",
    "57435": "MetaLeft",
    "57436": "MetaRight",
    "57437": "ContextMenu",
    "57438": "Power",
    "57439": "Sleep",
    "57443": "WakeUp",
    "57360": "MediaTrackPrevious",
    "57369": "MediaTrackNext",
    "57376": "AudioVolumeMute",
    "57377": "LaunchApp2",
    "57378": "MediaPlayPause",
    "57380": "MediaStop",
    "57390": "AudioVolumeDown",
    "57392": "AudioVolumeUp",
    "57394": "BrowserHome",
    "57404": "LaunchApp1",
    "57444": "LaunchApp3",
    "57445": "BrowserSearch",
    "57446": "BrowserFavorites",
    "57447": "BrowserRefresh",
    "57448": "BrowserStop",
    "57449": "BrowserForward",
    "57450": "BrowserBack",
    "57452": "LaunchMail",
    "57453": "MediaSelect",
}


def is_v1_config(cfg):
    """Return True if this is a V1 config (no definition_method field)."""
    if not isinstance(cfg, dict):
        return False
    if "definition_method" in cfg or "definitions" in cfg:
        return False
    return "defines" in cfg


def is_v1_multi(defines):
    """Return True if this is a multi-method V1 pack (string file paths per key)."""
    if not defines:
        return False
    string_values = sum(1 for v in defines.values() if isinstance(v, str) and v)
    return string_values > sum(1 for v in defines.values() if isinstance(v, list))


def get_audio_duration_ms(audio_path):
    """Get audio duration in ms. Uses ffprobe for non-WAV, wave stdlib for WAV.
    Returns None on failure."""
    if not os.path.exists(audio_path):
        return None

    # Check WAV header
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

    # ffprobe for everything else
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


# Donor for missing keys: missing -> [donor priority list]
# Numpad missing -> KeyA as requested; others use nearest semantic neighbor.
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
    # Numpad missing -> KeyA per request
    "Numpad0": ["KeyA"], "Numpad1": ["KeyA"], "Numpad2": ["KeyA"],
    "Numpad3": ["KeyA"], "Numpad4": ["KeyA"], "Numpad5": ["KeyA"],
    "Numpad6": ["KeyA"], "Numpad7": ["KeyA"], "Numpad8": ["KeyA"],
    "Numpad9": ["KeyA"], "NumpadDecimal": ["KeyA"], "NumpadAdd": ["KeyA"],
    "NumpadSubtract": ["KeyA"], "NumpadMultiply": ["KeyA"],
    "NumpadDivide": ["KeyA"], "NumpadEnter": ["KeyA"],
    "NumpadEquals": ["KeyA"], "NumpadComma": ["KeyA"],
    # Media / browser / launch — generic
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


def _fill_missing_keys(definitions):
    """Fill missing W3C keys by copying donor definition."""
    import copy
    for missing, donors in SMART_DONOR.items():
        if missing in definitions:
            continue
        for donor in donors:
            if donor in definitions:
                definitions[missing] = copy.deepcopy(definitions[donor])
                break


def convert_v1_to_v2(cfg, pack_dir):
    """Convert a V1 config dict to V2.

    `pack_dir` is the directory containing config.json and audio files.
    Returns a V2 config dict (does NOT write any files)."""
    defines = cfg.pop("defines", {})
    sound = cfg.pop("sound", None)

    definitions = {}

    if is_v1_multi(defines):
        # Multi-method: each key has its own audio file
        per_key_files = {}
        for code, filename in defines.items():
            if not isinstance(filename, str) or not filename or filename.strip().lower() == "null":
                continue
            w3c_name = V1_KEY_TABLE.get(str(code))
            if not w3c_name:
                continue
            per_key_files[w3c_name] = filename.strip()

        unique_files = set(per_key_files.values())

        # Try to find a shared audio file (single-method fallback)
        chosen_audio = None
        for f in unique_files:
            path = os.path.join(pack_dir, f)
            if os.path.exists(path):
                chosen_audio = f
                break

        if len(unique_files) == 1 and chosen_audio:
            # All keys share one file -> single-method
            dur = get_audio_duration_ms(os.path.join(pack_dir, chosen_audio))
            duration_ms = dur if dur else 100.0
            for w3c_name in per_key_files:
                definitions[w3c_name] = {"timing": [[0.0, duration_ms]]}
            definition_method = "single"
            audio_file = chosen_audio
        else:
            # Genuine multi-method
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
        # Single-method: defines are [start_ms, duration_ms] sprite regions
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
        # sound field is the sprite sheet
        audio_file = sound if sound else None
        # For single-method, we don't verify the sprite file exists here

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

    # Show a sample of key mappings
    sample_keys = list(v2_cfg["definitions"].keys())[:6]
    print(f"\n   Sample mappings:")
    for k in sample_keys:
        print(f"     {k}: {v2_cfg['definitions'][k]}")


if __name__ == "__main__":
    main()
