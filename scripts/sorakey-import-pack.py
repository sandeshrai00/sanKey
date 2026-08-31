#!/usr/bin/env python3
"""
Import a soundpack from a ZIP file.

ZIP layout (one of):
  pack-name/config.json
  pack-name/config.json + pack-name/sound.ogg
  pack-name/sounds/keydown.ogg  (multi-method, will be auto-converted)
  config.json  (no enclosing folder)

The config is installed at:
  ~/.local/share/sorakey/soundpacks/keyboard/{id}/

with directory structure preserved relative to config.json so any
subdirectory paths in `audio_file` (e.g. "sounds/click.ogg") still resolve.

V1 configs (no `definition_method` field) are converted to V2 before
installing. The daemon has a V1-aware metadata loader, but the pack
loader is V2-only, so conversion has to happen at import time.
"""

import json
import os
import re
import sys
import uuid
import zipfile
import subprocess
import tempfile

try:
    import gi
    gi.require_version("Gtk", "4.0")
    from gi.repository import Gtk, Gio, GLib
except ImportError:
    Gtk = None

SOUNDPACKS = os.path.expanduser("~/.local/share/sorakey/soundpacks")

# Zip bomb guard: cap total uncompressed
MAX_PACK_SIZE = 20 * 1024 * 1024

# Non-zip container formats the file dialog's "All Files" filter can surface
NON_ZIP_EXTS = (".7z", ".rar", ".tar", ".gz", ".bz2", ".xz")


def _short(value, limit=50):
    """One-line, whitespace-collapsed, length-capped error text for messages."""
    s = " ".join(str(value).split())
    if len(s) > limit:
        s = s[:limit - 1] + "…"
    return s or "unknown error"


# ----- V1 -> V2 conversion -----
# IOHook VC_* constants -> W3C key names.
# Directly mirrors mechvibes-dx-new's create_iohook_to_web_key_mapping()
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
    """V1 configs use `defines` (numeric keys) and `sound`, not V2 fields."""
    if not isinstance(cfg, dict):
        return False
    if "definition_method" in cfg or "definitions" in cfg or "defs" in cfg:
        return False
    if "defines" in cfg:
        return True
    return False


def is_v1_multi(defines):
    """V1 multi-method packs have string values per key (file paths)."""
    if not defines:
        return False
    string_values = sum(1 for v in defines.values() if isinstance(v, str) and v)
    return string_values > sum(1 for v in defines.values() if isinstance(v, list))


def looks_auto_generated_id(s):
    """True when the id looks like a placeholder or machine-generated string."""
    if not s:
        return True
    s = str(s).strip()
    if s.startswith("imported-"):
        return True
    if s.startswith("custom-sound-pack-"):
        return True
    if re.fullmatch(r"[0-9][0-9_\-]*", s):
        return True
    return False


def derive_name_from_zip(zip_path):
    """Turn a path like '/home/user/My Cool Pack.zip' into
    ('my-cool-pack', 'My Cool Pack') — slug + human label."""
    base = os.path.basename(zip_path)
    if base.lower().endswith(".zip"):
        base = base[:-4]
    slug = re.sub(r"[^a-zA-Z0-9_-]+", "-", base).strip("-").lower()
    if not slug:
        slug = f"imported-{uuid.uuid4().hex[:8]}"
    label = re.sub(r"[-_]+", " ", base).strip()
    if not label:
        label = slug
    return slug, label


def get_audio_duration_ms(audio_path, zf=None, zip_prefix=None):
    """Get audio duration in ms for any audio format.

    Uses ffprobe for non-WAV formats (MP3, OGG, FLAC, etc.).
    Falls back to Python stdlib wave module for WAV files.
    Returns None on failure."""
    try:
        if zf is not None and zip_prefix is not None:
            zip_path = zip_prefix + audio_path
            with zf.open(zip_path) as src:
                data = src.read()
        elif zf is not None:
            with zf.open(audio_path) as src:
                data = src.read()
        else:
            with open(audio_path, "rb") as f:
                data = f.read()
    except Exception:
        return None

    # Check if WAV
    is_wav = data[:4] == b'RIFF' and data[8:12] == b'WAVE'
    if is_wav:
        try:
            import wave as wave_module
            import io
            with wave_module.open(io.BytesIO(data)) as w:
                return w.getnframes() / w.getframerate() * 1000.0
        except Exception:
            pass

    # ffprobe for everything else
    tmp = None
    try:
        with tempfile.NamedTemporaryFile(suffix=os.path.splitext(audio_path)[1], delete=False) as f:
            f.write(data)
            tmp = f.name
        try:
            result = subprocess.run(
                ["ffprobe", "-v", "quiet", "-print_format", "json", "-show_format", tmp],
                capture_output=True, text=True, timeout=10
            )
        except Exception:
            # ffprobe missing/timed out — fall back to the 100ms default
            return None
    finally:
        if tmp:
            try:
                os.unlink(tmp)
            except Exception:
                pass

    try:
        d = json.loads(result.stdout)
        return float(d["format"]["duration"]) * 1000.0
    except Exception:
        return None


def _case_insensitive_match(filename, file_list):
    """Find filename in file_list case-insensitively. Also matches basename."""
    filename_lower = filename.lower()
    base_lower = os.path.basename(filename).lower()
    for f in file_list:
        if f.lower() == filename_lower:
            return f
        if os.path.basename(f).lower() == base_lower:
            return f
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


def _fill_missing_keys(definitions):
    import copy
    for missing, donors in SMART_DONOR.items():
        if missing in definitions:
            continue
        for donor in donors:
            if donor in definitions:
                definitions[missing] = copy.deepcopy(definitions[donor])
                break


def convert_v1_to_v2(cfg, available_files, soundpack_dir=None, zf=None, zip_prefix=None):
    """Convert a V1 soundpack config dict in place to V2."""
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

        # Normalize all filenames to their case-matched ZIP names
        normalized_files = {}
        for fname in unique_files:
            matched = _case_insensitive_match(fname, available_files)
            if matched:
                normalized_files[fname] = matched

        per_key_files_normalized = {}
        for w3c_name, fname in per_key_files.items():
            per_key_files_normalized[w3c_name] = normalized_files.get(fname, fname)

        normalized_unique = set(per_key_files_normalized.values())
        chosen_audio = None
        for f in normalized_unique:
            if f in available_files:
                chosen_audio = f
                break
        if chosen_audio is None and sound:
            matched = _case_insensitive_match(sound, available_files)
            if matched:
                chosen_audio = matched

        if len(normalized_unique) == 1 and chosen_audio:
            duration_ms = 100.0
            dur = get_audio_duration_ms(chosen_audio, zf, zip_prefix)
            if dur and dur > 0:
                duration_ms = dur
            for w3c_name in per_key_files_normalized:
                definitions[w3c_name] = {"timing": [[0.0, duration_ms]]}
            definition_method = "single"
            audio_file = chosen_audio
        else:
            file_durations = {}
            for fname in normalized_unique:
                dur = get_audio_duration_ms(fname, zf, zip_prefix)
                if dur and dur > 0:
                    file_durations[fname] = dur
                else:
                    file_durations[fname] = 100.0

            for w3c_name, filename in per_key_files_normalized.items():
                dur = file_durations.get(filename, 100.0)
                definitions[w3c_name] = {
                    "timing": [[0.0, dur]],
                    "audio_file": filename,
                }
            definition_method = "multi"
            audio_file = None
    else:
        # Single-method: defines are [start_ms, duration_ms] sprite regions.
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
                definitions[w3c_name] = {
                    "timing": [[start, end]]
                }
        definition_method = "single"
        audio_file = None
        if sound and sound in available_files:
            audio_file = sound
        elif sound:
            audio_file = sound

    _fill_missing_keys(definitions)

    new_cfg = {
        "id": cfg.get("id") or f"imported-{uuid.uuid4().hex[:8]}",
        "name": cfg.get("name") or "Imported Soundpack",
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


def find_config_in_zip(zf):
    """Find config.json anywhere in the ZIP. Returns (config_path, config_bytes)
    or (None, None). Prefers config.json at the shallowest level."""
    candidates = []
    for name in zf.namelist():
        if name.endswith("/") or not name.endswith("config.json"):
            continue
        depth = name.count("/")
        candidates.append((depth, name))
    if not candidates:
        return None, None
    candidates.sort()
    _, path = candidates[0]
    return path, zf.read(path)


def list_files_in_zip(zf, config_path, strip_folder):
    """List of file paths (relative to config) in ZIP after stripping enclosing folder."""
    prefix = ""
    if strip_folder:
        top = config_path.split("/")[0]
        prefix = top + "/"
    out = []
    for name in zf.namelist():
        if name.endswith("/"):
            continue
        rel = name[len(prefix):] if prefix and name.startswith(prefix) else name
        if not rel:
            continue
        # Keep nested paths (sounds/click.ogg) but store as relative
        out.append(rel)
    return out


def strip_enclosing_folder(zf, config_path):
    """If everything in the ZIP lives under a single top-level folder that
    also contains the config, return names relative to that folder. Otherwise
    return False."""
    names = [n for n in zf.namelist() if not n.endswith("/")]
    if not names:
        return False
    first_slash = names[0].find("/")
    if first_slash < 0:
        return False
    top = names[0][:first_slash]
    if not all(n.startswith(top + "/") for n in names):
        return False
    if not config_path.startswith(top + "/"):
        return False
    return True


def validate_v2(cfg):
    """Return None on success, or a short human-readable error reason."""
    if not cfg.get("name"):
        return "no name"
    if not cfg.get("author"):
        return "no author"
    if not (cfg.get("definitions") or cfg.get("defs")):
        return "no key definitions"
    if cfg.get("definition_method") not in ("single", "multi"):
        return "definition_method must be single or multi"
    if cfg.get("definition_method") == "single" and not cfg.get("audio_file"):
        return "single method needs audio_file"
    return None


def import_zip(zip_path):
    try:
        zf = zipfile.ZipFile(zip_path, "r")
    except zipfile.BadZipFile:
        ext = os.path.splitext(zip_path)[1].lower()
        if ext in NON_ZIP_EXTS:
            return None, f"Not a ZIP — this is {ext}. Re-compress as .zip."
        return None, "Not a valid ZIP — file may be corrupted."
    except Exception as e:
        return None, f"Can't read file: {_short(e, 40)}"
    with zf:
        config_path, config_bytes = find_config_in_zip(zf)
        if not config_path:
            return None, "No config.json — not a soundpack."

        try:
            cfg = json.loads(config_bytes)
        except Exception:
            return None, "config.json damaged — re-download pack."

        strip_folder = strip_enclosing_folder(zf, config_path)
        available_files = list_files_in_zip(zf, config_path, strip_folder)
        if not available_files:
            return None, "ZIP is empty or damaged."

        zip_slug, zip_label = derive_name_from_zip(zip_path)
        if looks_auto_generated_id(cfg.get("id")):
            cfg["id"] = zip_slug
        if looks_auto_generated_id(cfg.get("name")) or not cfg.get("name"):
            cfg["name"] = zip_label
        cfg["id"] = re.sub(r"[^a-zA-Z0-9_-]+", "-", str(cfg["id"])).strip("-").lower()
        if not cfg["id"]:
            cfg["id"] = zip_slug

        soundpack_id = cfg["id"]
        install_dir = os.path.join(SOUNDPACKS, "keyboard", soundpack_id)

        zip_prefix = ""
        if strip_folder:
            zip_prefix = config_path.split("/")[0] + "/"

        if is_v1_config(cfg):
            cfg = convert_v1_to_v2(cfg, available_files, install_dir, zf, zip_prefix)
        else:
            err = validate_v2(cfg)
            if err:
                return None, f"Bad config: {err}"

        # Validate audio files exist before destroying old install (for both methods)
        if cfg.get("definition_method") == "single":
            audio_rel = cfg.get("audio_file", "")
            if audio_rel and audio_rel not in available_files and not _case_insensitive_match(audio_rel, available_files):
                return None, f"Missing audio file: {audio_rel}"
        elif cfg.get("definition_method") == "multi":
            missing = sorted({d.get("audio_file") for d in cfg.get("definitions", {}).values() if d.get("audio_file") and d.get("audio_file") not in available_files and not _case_insensitive_match(d.get("audio_file"), available_files)})
            if missing:
                if len(missing) == 1:
                    return None, f"Missing audio: {missing[0]}"
                names = ", ".join(missing[:2])
                if len(missing) > 2:
                    names += f" +{len(missing) - 2} more"
                return None, f"Missing audio: {names}"

        import shutil, pathlib
        # Zip bomb guard: cap total uncompressed
        try:
            total = sum(zf.getinfo(n).file_size for n in zf.namelist())
            if total > MAX_PACK_SIZE:
                return None, f"Pack too big: {total // (1024 * 1024)}MB (max {MAX_PACK_SIZE // (1024 * 1024)}MB)."
        except Exception:
            pass
        tmp_dir = install_dir + ".tmp." + str(os.getpid())
        if os.path.exists(tmp_dir):
            shutil.rmtree(tmp_dir)
        os.makedirs(tmp_dir, exist_ok=True)
        try:
            prefix = ""
            if strip_folder:
                top = config_path.split("/")[0]
                prefix = top + "/"
            for name in zf.namelist():
                if name.endswith("/"):
                    continue
                rel = name[len(prefix):] if prefix and name.startswith(prefix) else name
                if not rel:
                    continue
                # ZIP traversal guard
                if rel.startswith("/") or ".." in pathlib.PurePosixPath(rel).parts or "\\" in rel:
                    continue
                target = os.path.join(tmp_dir, rel)
                # ensure target stays under tmp_dir
                if not os.path.abspath(target).startswith(os.path.abspath(tmp_dir)):
                    continue
                os.makedirs(os.path.dirname(target), exist_ok=True)
                import shutil as _sh
                with zf.open(name) as src, open(target, "wb") as dst:
                    _sh.copyfileobj(src, dst, length=64*1024)
            with open(os.path.join(tmp_dir, "config.json"), "w") as f:
                json.dump(cfg, f, indent=4, sort_keys=False)
                f.write("\n")
            if os.path.exists(install_dir):
                shutil.rmtree(install_dir)
            os.rename(tmp_dir, install_dir)
        except Exception:
            if os.path.exists(tmp_dir):
                shutil.rmtree(tmp_dir, ignore_errors=True)
            raise
        return soundpack_id, None


def cli_main():
    if len(sys.argv) < 2:
        print("Usage: sorakey-import-pack.py <path-to-zip>")
        sys.exit(1)
    path = sys.argv[1]
    if not os.path.isfile(path):
        print(f"ERROR:File not found: {path}", flush=True)
        sys.exit(1)
    try:
        soundpack_id, err = import_zip(path)
    except Exception as e:
        print(f"ERROR:Import failed: {_short(e)}", flush=True)
        sys.exit(1)
    if err:
        print(f"ERROR:{err}", flush=True)
        sys.exit(1)
    print(f"OK:{soundpack_id}", flush=True)
    sys.exit(0)


def gui_main():
    if Gtk is None:
        print("ERROR:GTK 4 missing — file dialog can't open", flush=True)
        sys.exit(1)
    dialog = Gtk.FileDialog(title="Import Soundpack")
    filter_zip = Gtk.FileFilter()
    filter_zip.set_name("Soundpack ZIP")
    filter_zip.add_pattern("*.zip")
    filter_all = Gtk.FileFilter()
    filter_all.set_name("All Files")
    filter_all.add_pattern("*")
    filters = Gio.ListStore.new(Gtk.FileFilter)
    filters.append(filter_zip)
    filters.append(filter_all)
    dialog.set_filters(filters)
    dialog.set_default_filter(filter_zip)

    loop = GLib.MainLoop()

    def fail_and_quit(msg):
        print(f"ERROR:{msg}", flush=True)
        loop.quit()

    def succeed_and_quit(msg):
        print(f"OK:{msg}", flush=True)
        loop.quit()

    def on_done(source, result, user_data=None):
        try:
            file = dialog.open_finish(result)
        except Exception as e:
            err = str(e)
            if "Dismissed" in err or "cancel" in err.lower():
                fail_and_quit("Cancelled")
            else:
                fail_and_quit(f"Dialog error: {e}")
            return
        if file is None:
            fail_and_quit("No file selected")
            return
        path = file.get_path()
        try:
            sid, err = import_zip(path)
        except Exception as e:
            fail_and_quit(f"Import failed: {_short(e)}")
            return
        if err:
            fail_and_quit(err)
        else:
            succeed_and_quit(sid)

    def launch_dialog():
        dialog.open(None, None, on_done, None)

    GLib.idle_add(launch_dialog)
    loop.run()


if __name__ == "__main__":
    if len(sys.argv) > 1:
        cli_main()
    else:
        gui_main()
