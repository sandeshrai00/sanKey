#!/usr/bin/env python3
"""Export sorakey logs — shows save dialog, writes file."""

import json
import os
import re
import subprocess
import sys

# Same self-reload root fix as the importer: a launched script's own
# __pycache__ entry would be written into the watched plugin dir on first
# tap (shell reloads → bar blink). Canonical flag — see
# sorakey-import-pack.py for why the no-underscore alias is a no-op.
sys.dont_write_bytecode = True

try:
    import gi
    gi.require_version("Gtk", "4.0")
    from gi.repository import Gtk, Gio, GLib
except Exception:
    Gtk = None

# Detached-run support: see sorakey-import-pack.py — result line goes to
# --result-file too, so Service.qml can poll it after a plugin reload.
RESULT_FILE = None


def emit(line):
    print(line, flush=True)
    if RESULT_FILE:
        try:
            with open(RESULT_FILE, "w") as f:
                f.write(line + "\n")
        except Exception:
            pass


def take_result_file_argv():
    global RESULT_FILE
    args = []
    skip_next = False
    for i in range(len(sys.argv)):
        if skip_next:
            skip_next = False
            continue
        if sys.argv[i] == "--result-file" and i + 1 < len(sys.argv):
            RESULT_FILE = sys.argv[i + 1]
            skip_next = True
        else:
            args.append(sys.argv[i])
    sys.argv = args


def safe_filename(name):
    """Daemon-controlled name -> basename with no separators or dot-dots."""
    name = os.path.basename(str(name or "sorakey-log.txt"))
    name = re.sub(r"[^A-Za-z0-9_.-]+", "_", name).strip("._") or "sorakey-log.txt"
    return name


def get_log_contents():
    bin_path = os.path.expanduser("~/.local/bin/sorakey")
    try:
        result = subprocess.run(
            [bin_path, "ctl", '{"cmd":"export_logs"}'],
            capture_output=True, text=True, timeout=5
        )
        if result.returncode != 0:
            return None, (result.stderr.strip().split("\n")[-1] if result.stderr.strip() else "daemon error")
        data = json.loads(result.stdout.strip().split("\n")[-1])
        if data.get("ok"):
            return data.get("contents", ""), safe_filename(data.get("name", "sorakey-log.txt"))
        return None, data.get("error", "failed")
    except Exception as e:
        return None, str(e)


def gui_main():
    if Gtk is None:
        emit("ERROR:GTK 4 missing — file dialog can't open")
        sys.exit(1)

    contents, name_or_err = get_log_contents()
    if contents is None:
        emit(f"ERROR:{name_or_err}")
        sys.exit(1)

    # Portal-native FileDialog — the OS default file manager dialog, so it
    # always matches whatever the user has set as default. Parent-less, as
    # originally: it now runs detached (reload-proof), so a plugin reload
    # can no longer SIGTERM it mid-dialog.
    dialog = Gtk.FileDialog(title="Save Error Logs")
    dialog.set_initial_name(name_or_err)

    filter_txt = Gtk.FileFilter()
    filter_txt.set_name("Text Files")
    filter_txt.add_pattern("*.txt")
    filter_all = Gtk.FileFilter()
    filter_all.set_name("All Files")
    filter_all.add_pattern("*")
    filters = Gio.ListStore.new(Gtk.FileFilter)
    filters.append(filter_txt)
    filters.append(filter_all)
    dialog.set_filters(filters)
    dialog.set_default_filter(filter_txt)

    loop = GLib.MainLoop()

    def fail_and_quit(msg):
        emit(f"ERROR:{msg}")
        loop.quit()

    def succeed_and_quit(path):
        emit(f"OK:{path}")
        loop.quit()

    def on_done(source, result, user_data=None):
        try:
            file = dialog.save_finish(result)
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
        if not path:
            fail_and_quit("No file selected")
            return
        try:
            with open(path, "w", encoding="utf-8") as fh:
                fh.write(contents)
        except Exception as e:
            fail_and_quit(f"Could not write {path}: {e}")
            return
        succeed_and_quit(path)

    def launch_dialog():
        dialog.save(None, None, on_done, None)

    GLib.idle_add(launch_dialog)
    loop.run()


def cli_main():
    contents, name_or_err = get_log_contents()
    if contents is None:
        emit(f"ERROR:{name_or_err}")
        sys.exit(1)
    # cli mode: write to Downloads directly
    downloads = os.path.expanduser("~/Downloads")
    os.makedirs(downloads, exist_ok=True)
    path = os.path.join(downloads, name_or_err)
    with open(path, "w", encoding="utf-8") as f:
        f.write(contents)
    emit(f"OK:{path}")
    sys.exit(0)


if __name__ == "__main__":
    take_result_file_argv()
    if len(sys.argv) > 1:
        cli_main()
    else:
        gui_main()
