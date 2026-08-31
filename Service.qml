import QtQuick
import Quickshell
import Quickshell.Io

Item {
  id: root

  visible: false
  width: 0
  height: 0

  property var shell: null
  property var manifest: null
  readonly property string pluginDir: manifest && manifest.__sourceDir
    ? String(manifest.__sourceDir)
    : Quickshell.env("HOME") + "/.config/omarchy/plugins/io.github.sandeshrai00.sorakey"

  readonly property string pluginId: manifest && manifest.id
    ? String(manifest.id) : "io.github.sandeshrai00.sorakey"

  property bool importing: false
  property string lastImportResult: ""
  property string lastImportError: ""

  signal packsImported(string packId)
  // ponytail: importFailed was never connected — lastImportError is single source
  function notify(title, msg) { Quickshell.execDetached(["notify-send","-a","Sorakey", title, msg]); clearImportTimer.restart() }

  function importSoundpack() {
    if (importing) return
    if (!pluginDir) {
      lastImportError = "Service pluginDir is empty (manifest not injected)"
      lastImportResult = ""
      return
    }
    if (!importHelper.running) {
      importing = true
      lastImportError = ""
      lastImportResult = ""
      importHelper.command = ["/usr/bin/python3",
        pluginDir + "/scripts/sorakey-import-pack.py"]
      importHelper.running = true
    }
  }

  Timer { id: clearImportTimer; interval: 4000; onTriggered: { root.lastImportError = ""; root.lastImportResult = "" } }

  Process {
    id: importHelper
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { waitForEnd: true }
    onExited: function(exitCode) {
      root.importing = false
      var output = String(stdout.text || "").trim()
      var errOutput = String(stderr.text || "").trim()
      var lines = output.split("\n")
      var last = lines[lines.length - 1]
      if (last.startsWith("OK:")) {
        root.lastImportResult = last.substring(3).trim()
        root.lastImportError = ""
        root.packsImported(root.lastImportResult)
        root.notify("Soundpack imported", root.lastImportResult)
      } else if (last.startsWith("ERROR:")) {
        var msg = last.substring(6).trim()
        if (msg === "Cancelled" || msg.toLowerCase().indexOf("cancel") !== -1) {
          root.lastImportError = ""
          root.lastImportResult = ""
          return
        }
        root.lastImportError = msg
        root.lastImportResult = ""
        root.notify("Import failed", msg)
      } else if (exitCode !== 0) {
        root.lastImportError = errOutput || "Import failed — try again."
        root.lastImportResult = ""
        root.notify("Import failed", root.lastImportError)
      } else {
        root.lastImportError = "Import failed — try again."
        root.lastImportResult = ""
        root.notify("Import failed", root.lastImportError)
      }
    }
  }

  // (Re)enable = the plugin is on in shell.json, so the daemon should be
  // running. Mirrors the teardown in onDestruction below: the instance is
  // dropped on disable, remove, and plugin reload, and recreated on
  // (re)enable and shell start. On a fresh install the unit does not exist
  // yet (setup has not run); the call fails harmlessly and sorakey-setup
  // starts the service itself.
  Process {
    id: startProc
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { waitForEnd: true }
  }

  Component.onCompleted: {
    if (!startProc.running) {
      startProc.command = ["systemctl", "--user", "enable", "--now", "sorakey"]
      startProc.running = true
    }
    // ponytail: single enable, retry via systemd Restart=on-failure, not 3s timer
    freshnessCheck.running = true
  }

  // After `omarchy plugin update` the daemon source may be newer than the
  // installed binary (or a release prebuilt may be available for it). Re-run
  // the build script — it downloads a verified prebuilt when one matches the
  // source, else builds — and reload the daemon if a new binary landed.
  Process {
    id: freshnessCheck
    command: ["/usr/bin/bash", root.pluginDir + "/scripts/build-sorakey.sh"]
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { waitForEnd: true }
    onExited: function(exitCode) {
      if (exitCode !== 0) return
      var out = String(stdout.text || "").trim()
      var lines = out.split("\n")
      var line = lines[lines.length - 1]
      console.info("sorakey freshness: " + line)
      if (line.indexOf("up to date") !== -1) return
      Quickshell.execDetached(["systemctl", "--user", "restart", "sorakey"])
    }
  }

  Component.onDestruction: {
    // Only the shell-managed instance (one with `shell` injected by the shell)
    // owns the daemon lifecycle. The panel inlines this component for the
    // import feature, and its instances come and go with every bar rebuild —
    // they must not stop the daemon.
    if (!root.shell) return
    // Stop the sorakey daemon when the plugin instance goes away (disabled,
    // removed, or reloaded from disk). Component.onCompleted restores it
    // whenever the plugin is enabled again.
    Quickshell.execDetached(["systemctl", "--user", "stop", "sorakey"])
    Quickshell.execDetached(["systemctl", "--user", "disable", "sorakey"])
    // Fallback: kill any running sorakey process directly
    Quickshell.execDetached(["pkill", "-x", "sorakey"])
  }
}
