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

  // enable daemon when plugin is on — mirrors onDestruction teardown
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
    freshnessCheck.running = true
  }

  // after update, rebuild or fetch prebuilt and restart if needed
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
    // only shell instance owns daemon lifecycle — panel copies come and go
    if (!root.shell) return
    // stop on disable/remove/reload
    Quickshell.execDetached(["systemctl", "--user", "stop", "sorakey"])
    Quickshell.execDetached(["systemctl", "--user", "disable", "sorakey"])
    Quickshell.execDetached(["pkill", "-x", "sorakey"])
  }
}
