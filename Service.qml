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
  property bool exporting: false
  property string lastExportResult: ""
  property string lastExportError: ""
  // sticky stop: true if the user explicitly stopped the daemon (Panel writes the flag)
  readonly property bool stoppedFlag: Qt.fileExists("file:///" + Quickshell.env("HOME") + "/.local/share/sorakey/stopped")

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

  function exportLogs() {
    if (exporting) return
    if (!pluginDir) {
      lastExportError = "Service pluginDir is empty"
      lastExportResult = ""
      return
    }
    if (!exportHelper.running) {
      exporting = true
      lastExportError = ""
      lastExportResult = ""
      exportHelper.command = ["/usr/bin/python3",
        pluginDir + "/scripts/sorakey-export-logs.py"]
      exportHelper.running = true
    }
  }

  Timer { id: clearImportTimer; interval: 10000; onTriggered: { root.lastImportResult = ""; root.lastExportResult = "" } }

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

  Process {
    id: exportHelper
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { waitForEnd: true }
    onExited: function(exitCode) {
      root.exporting = false
      var output = String(stdout.text || "").trim()
      var errOutput = String(stderr.text || "").trim()
      var lines = output.split("\n")
      var last = lines[lines.length - 1]
      if (last.startsWith("OK:")) {
        root.lastExportResult = last.substring(3).trim()
        root.lastExportError = ""
        root.notify("Logs exported", root.lastExportResult)
      } else if (last.startsWith("ERROR:")) {
        var msg = last.substring(6).trim()
        if (msg === "Cancelled" || msg.toLowerCase().indexOf("cancel") !== -1) {
          root.lastExportError = ""
          root.lastExportResult = ""
          return
        }
        root.lastExportError = msg
        root.lastExportResult = ""
        root.notify("Export failed", msg)
      } else if (exitCode !== 0) {
        root.lastExportError = errOutput || "Export failed — try again."
        root.lastExportResult = ""
        root.notify("Export failed", root.lastExportError)
      } else {
        root.lastExportError = "Export failed — try again."
        root.lastExportResult = ""
        root.notify("Export failed", root.lastExportError)
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
    // sticky stop: only auto-start if the user hasn't explicitly stopped the daemon
    if (!startProc.running && !root.stoppedFlag) {
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
      stdout.text = ""
      stderr.text = ""
      Quickshell.execDetached(["systemctl", "--user", "restart", "sorakey"])
    }
  }

  Component.onDestruction: {
    // only shell instance owns daemon lifecycle — panel copies come and go
    if (!root.shell) return
    // stop on disable/remove/reload
    Quickshell.execDetached(["systemctl", "--user", "stop", "sorakey"])
    Quickshell.execDetached(["systemctl", "--user", "disable", "sorakey"])
    // exact-path match (the daemon's cmdline is its ExecStart): -x would hit
    // any other process that happens to be named "sorakey"; the daemon writes
    // no PID file so there is nothing tighter than this
    Quickshell.execDetached(["pkill", "-f", Quickshell.env("HOME") + "/.local/bin/sorakey"])
  }
}
