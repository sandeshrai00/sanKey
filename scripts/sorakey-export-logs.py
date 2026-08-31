#!/usr/bin/env python3
"""Export sorakey logs — shows save dialog, writes file."""

import json
import os
import subprocess
import sys

try:
    import gi
    gi.require_version("Gtk", "4.0")
    from gi.repository import Gtk, Gio, GLib
except ImportError:
    Gtk = None


def get_log_contents():
    bin_path = os.path.expanduser("~/.local/bin/sorakey")
    try:
        result = subprocess.run(
            [bin_path, "ctl", '{"cmd":"export_logs"}'],
            capture_output=True, text=True, timeout=5
        )
        data = json.loads(result.stdout.strip().split("\n")[-1])
        if data.get("ok"):
            return data.get("contents", ""), data.get("name", "sorakey-log.txt")
        return None, data.get("error", "failed")
    except Exception as e:
        return None, str(e)


def gui_main():
    if Gtk is None:
        print("ERROR:GTK 4 missing — file dialog can't open", flush=True)
        sys.exit(1)

    contents, name_or_err = get_log_contents()
    if contents is None:
        print(f"ERROR:{name_or_err}", flush=True)
        sys.exit(1)

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
        print(f"ERROR:{msg}", flush=True)
        loop.quit()

    def succeed_and_quit(path):
        print(f"OK:{path}", flush=True)
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
        try:
            with open(path, "w") as f:
                f.write(contents)
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
        print(f"ERROR:{name_or_err}", flush=True)
        sys.exit(1)
    # cli mode: write to Downloads directly
    downloads = os.path.expanduser("~/Downloads")
    os.makedirs(downloads, exist_ok=True)
    path = os.path.join(downloads, name_or_err)
    with open(path, "w") as f:
        f.write(contents)
    print(f"OK:{path}", flush=True)
    sys.exit(0)


if __name__ == "__main__":
    if len(sys.argv) > 1:
        cli_main()
    else:
        gui_main()
