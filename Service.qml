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
  property string pluginDir: ""

  readonly property string pluginId: manifest && manifest.id
    ? String(manifest.id) : "io.github.sandeshrai00.sankey"

  property bool importing: false
  property string lastImportResult: ""
  property string lastImportError: ""

  signal packsImported(string packId)
  signal importFailed(string reason)

  function importSoundpack() {
    if (importing) return
    if (!pluginDir) {
      lastImportError = "Service pluginDir is empty (manifest not injected)"
      lastImportResult = ""
      importFailed(lastImportError)
      return
    }
    if (!importHelper.running) {
      importing = true
      lastImportError = ""
      lastImportResult = ""
      importHelper.command = ["/usr/bin/python3",
        pluginDir + "/scripts/sankey-import-pack.py"]
      importHelper.running = true
    }
  }

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
      if (output === "" && exitCode === 0) { // cancelled file dialog
        root.lastImportError = ""
        root.lastImportResult = ""
        return
      }
      if (last.startsWith("OK:")) {
        root.lastImportResult = last.substring(3).trim()
        root.lastImportError = ""
        root.packsImported(root.lastImportResult)
      } else if (last.startsWith("ERROR:")) {
        root.lastImportError = last.substring(6).trim()
        root.lastImportResult = ""
        root.importFailed(root.lastImportError)
      } else if (exitCode !== 0) {
        root.lastImportError = errOutput || ("Import failed (exit " + exitCode + ")")
        root.lastImportResult = ""
        root.importFailed(root.lastImportError)
      } else {
        root.lastImportError = "Unexpected output: " + last
        root.lastImportResult = ""
        root.importFailed(root.lastImportError)
      }
    }
  }

  Component.onDestruction: {
    // Stop the sankeyd daemon when the plugin is unloaded/removed.
    // This handles both user-initiated removal via the menu and
    // programmatic removal via omarchy-plugin-remove.
    Quickshell.execDetached(["systemctl", "--user", "stop", "sankey"])
    Quickshell.execDetached(["systemctl", "--user", "disable", "sankey"])
    // Fallback: kill any running sankeyd process directly
    Quickshell.execDetached(["pkill", "-x", "sankeyd"])
  }
}
