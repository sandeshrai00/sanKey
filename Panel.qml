import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import qs.Ui
import qs.Commons
import "Model.js" as Model

// Sankey: keyboard sounds from the sankeyd daemon.
//
// The bar icon is the entry point and also a fast mute (right click) and a
// volume wheel. Left click opens this panel: live status, mute, volume,
// soundpack picker, and install/start/stop/remove.
//
// State comes from one `sankeyd ctl status` every 5 s plus a refresh on open;
// commands are fire-and-forget one-shot `ctl`/`systemctl` runs. No daemon
// state is kept in this file beyond what the last reading says.
Panel {
  id: root
  moduleName: "io.github.sandeshrai00.sankey"
  ipcTarget: "io.github.sandeshrai00.sankey"

  readonly property string home: Quickshell.env("HOME")
  readonly property string sankeydBin: home + "/.local/bin/sankeyd"
  readonly property string pluginDir: home + "/.config/omarchy/plugins/io.github.sandeshrai00.sankey"
  readonly property string setupPath: pluginDir + "/scripts/sankey-setup"

  // ---- Background service (import, future backend logic) ----
  property var service: Service { pluginDir: root.pluginDir }

  property bool importing: service ? service.importing : false
  property string importStatus: {
    if (importing) return "Importing…"
    if (!service) return ""
    if (service.lastImportError) return service.lastImportError
    if (service.lastImportResult) return "Imported: " + service.lastImportResult
    return ""
  }

  function triggerImport() {
    if (service) service.importSoundpack()
  }

  // When import completes, refresh the pack list so the new pack appears.
  Connections {
    target: service
    function onPacksImported(packId) { root.refreshPacks() }
    function onImportFailed(reason) { }
  }

  // ---- State from the last reading ----
  property bool installed: false
  property bool running: false
  property bool muted: false
  property real volume: 100
  property string keyboardPack: ""
  property var keyboardPacks: []

  readonly property string statusText: {
    if (!root.installed) return "Not installed"
    if (!root.running) return "Stopped"
    return root.muted ? "Muted" : "Playing"
  }

  property bool settingsOpen: false

  // The bar sizes widgets by their implicit size; the base Panel is a plain
  // Item that does not inherit it, so report the button's.
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  // ---- Commands ----
  function sendCtl(obj) {
    if (!root.installed) return
    if (ctlProc.running) return
    ctlProc.command = [root.sankeydBin, "ctl", JSON.stringify(obj)]
    ctlProc.running = true
  }

  function runService(args) {
    if (svcProc.running) return
    svcProc.command = ["systemctl", "--user"].concat(args)
    svcProc.running = true
  }

  function setMuted(on) {
    root.muted = on
    root.sendCtl({ cmd: "mute", muted: on })
  }

  function setVolume(v) {
    root.volume = v
    root.sendCtl({ cmd: "volume", value: v })
  }

  function setKeyboardPack(id) {
    root.keyboardPack = id
    root.sendCtl({ cmd: "keyboard_pack", id: id })
  }

  function pickRandomPack() {
    if (!root.keyboardPacks || root.keyboardPacks.length === 0) return
    var pool = root.keyboardPacks
    if (pool.length > 1 && root.keyboardPack) pool = pool.filter(function(id){ return id !== root.keyboardPack })
    var pick = pool[Math.floor(Math.random()*pool.length)]
    if (pick) root.setKeyboardPack(pick)
  }

  function startDaemon() { root.runService(["start", "sankey"]); root.refreshStatus() }
  function stopDaemon()  { root.runService(["stop", "sankey"]);  root.refreshStatus() }
  function restartDaemon() { root.runService(["restart", "sankey"]); root.refreshStatus() }

  function install() {
    if (root.bar) root.bar.run("omarchy-launch-floating-terminal-with-presentation "
      + Util.shellQuote(root.setupPath))
  }

  function openCustomFolder() {
    var path = home + "/.local/share/sankey/soundpacks"
    Quickshell.execDetached(["xdg-open", path])
  }

  readonly property string currentBarSection: {
    var cfg = root.bar && root.bar.shell ? root.bar.shell.shellConfig : null
    var layout = cfg && cfg.bar && cfg.bar.layout ? cfg.bar.layout : null
    if (!layout) return "right"
    var id = "io.github.sandeshrai00.sankey"
    for (var s of ["left","center","right"]) {
      var arr = layout[s]
      if (!Array.isArray(arr)) continue
      for (var i=0;i<arr.length;i++) if (arr[i] && arr[i].id===id) return s
    }
    return "right"
  }

  function moveToSection(section) {
    if (["left","center","right"].indexOf(section)===-1) return
    Quickshell.execDetached(["omarchy","plugin","enable","io.github.sandeshrai00.sankey","--section",section])
  }

  function remove() {
    // Omarchy-native: plugin removal via CLI, daemon via systemd — no bar.run rm -rf
    Quickshell.execDetached(["systemctl", "--user", "disable", "--now", "sankey"])
    Quickshell.execDetached(["omarchy", "plugin", "remove", "io.github.sandeshrai00.sankey", "--yes"])
    root.installed = false
    root.running = false
  }

  // ---- Readings ----
  function applyStatus(text) {
    var o = Model.parseStatus(text)
    if (!o) {
      // No reading: the daemon is up only if the binary answered at all.
      return
    }
    if (o.ok === true) {
      root.running = true
      root.installed = true
      root.muted = o.muted === true
      root.volume = (typeof o.volume === "number") ? o.volume : root.volume
      root.keyboardPack = String(o.keyboard_pack || "")
    } else {
      root.running = false
    }
  }

  function refreshStatus() {
    if (statusProc.running) return
    // Always try status — if daemon responds, applyStatus will set installed=true
    statusProc.running = true
  }

  function refreshPacks() {
    if (!root.installed) return
    if (packsProc.running) return
    packsProc.running = true
  }

  property bool automaticSetupAttempted: false

  Component.onCompleted: {
    installCheck.running = true
    root.refreshStatus()
  }

  onOpenedChanged: {
    if (root.opened) {
      root.refreshStatus()
      root.refreshPacks()
    }
  }

  // Detect (re)install without a daemon: is the binary there?
  Process {
    id: installCheck
    command: ["test", "-x", root.sankeydBin]
    onExited: function(exitCode) {
      root.installed = (exitCode === 0)
      if (root.installed) root.refreshStatus()
      else if (!root.automaticSetupAttempted) {
        root.automaticSetupAttempted = true
        // Auto-run setup on first enable after URL install (like Spotify)
        Qt.callLater(function(){ root.install() })
      }
    }
  }

  Process {
    id: statusProc
    command: [root.sankeydBin, "ctl", "{\"cmd\":\"status\"}"]
    onExited: function(exitCode) {
      if (exitCode === 0) {
        root.applyStatus(stdout.text)
      } else {
        // Either not installed or stopped.
        var o = Model.parseStatus(stdout.text)
        if (o && o.ok === false) root.running = false
      }
    }
    stdout: StdioCollector { waitForEnd: true }
  }

  Process {
    id: packsProc
    command: [root.sankeydBin, "ctl", "{\"cmd\":\"packs\"}"]
    onExited: function(exitCode) {
      if (exitCode !== 0) return
      var p = Model.parsePacks(stdout.text)
      root.keyboardPacks = p.keyboard
    }
    stdout: StdioCollector { waitForEnd: true }
  }

  // Fire-and-forget command channel to the daemon.
  Process {
    id: ctlProc
    command: [root.sankeydBin, "ctl", "{}"]
    onExited: function() { root.refreshStatus() }
    stdout: StdioCollector { waitForEnd: true }
  }

  // systemctl channel for start/stop/enable.
  Process {
    id: svcProc
    command: ["true"]
    onExited: function() { root.refreshStatus() }
    stdout: StdioCollector { waitForEnd: true }
  }

  // Poll only when panel open (or running needs refresh) — 0 forks when idle.
  Timer {
    interval: 5000
    repeat: true
    running: root.installed && root.opened
    onTriggered: {
      root.refreshStatus()
      root.refreshPacks()
    }
  }
  // Light background poll when installed but panel closed — 30s, not 5s.
  Timer {
    interval: 30000
    repeat: true
    running: root.installed && !root.opened && root.running
    onTriggered: root.refreshStatus()
  }

  // Re-check the binary quickly so auto-setup flips "Not installed" to live.
  Timer {
    interval: 5000
    repeat: true
    running: !root.installed
    onTriggered: {
      installCheck.running = true
      root.refreshStatus()
    }
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: "󰌌"
    dimmed: !root.running
    active: root.running && root.muted
    tooltipText: "Sankey — " + root.statusText
    onPressed: function(b) {
      if (b === Qt.RightButton) {
        if (root.running) root.setMuted(!root.muted)
      } else {
        root.toggle()
      }
    }
    onWheelMoved: function(delta) {
      if (!root.running) return
      var step = delta > 0 ? 5 : -5
      root.setVolume(Math.max(0, Math.min(100, root.volume + step)))
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    gap: Style.gapsOut + Style.space(6)
    contentWidth: panel.fittedContentWidth(Style.space(340))
    contentHeight: panel.fittedContentHeight(panelColumn.implicitHeight, Style.space(520))

    ScrollView {
      id: scrollArea
      anchors.fill: parent
      clip: true
      ScrollBar.horizontal.policy: ScrollBar.AlwaysOff
      ScrollBar.vertical.policy: panelColumn.implicitHeight > height ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff

      Column {
        id: panelColumn
        width: scrollArea.availableWidth
        spacing: Style.space(14)

        // ---------- Hero ----------
        Item {
          visible: !root.settingsOpen
          width: parent.width
          implicitHeight: Math.max(heroIcon.implicitHeight, heroLabels.implicitHeight, Math.max(muteSwitch.implicitHeight, settingsButton.implicitHeight))

          Text {
            id: heroIcon
            text: "󰌌"
            color: root.bar.foreground
            font.family: root.bar.fontFamily
            font.pixelSize: Style.font.display
            opacity: root.muted ? 0.5 : 1.0
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
          }

          Column {
            id: heroLabels
            anchors.left: heroIcon.right
            anchors.leftMargin: Style.space(14)
            anchors.right: settingsButton.left
            anchors.rightMargin: Style.space(12)
            anchors.verticalCenter: parent.verticalCenter
            spacing: Style.space(2)

            Text {
              text: "Sankey"
              color: root.bar.foreground
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.title
              font.bold: true
            }
            Text {
              text: root.statusText
              color: root.bar.foreground
              opacity: 0.6
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.caption
            }
          }

          ToggleSwitch {
            id: muteSwitch
            checked: root.running && !root.muted
            foreground: root.bar.foreground
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            onToggled: root.setMuted(!root.muted)
          }

          Button {
            id: settingsButton
            text: ""
            iconText: "󰒓"
            foreground: root.bar.foreground
            opacity: 0.8
            anchors.right: muteSwitch.left
            anchors.rightMargin: Style.space(8)
            anchors.verticalCenter: parent.verticalCenter
            tooltipText: "Settings"
            onClicked: root.settingsOpen = !root.settingsOpen
          }
        }

        // ---------- Settings (only view) ----------
        Column {
          visible: root.settingsOpen
          width: parent.width
          spacing: Style.space(10)
          Row {
            width: parent.width
            spacing: Style.space(8)
            Button {
              text: ""
              iconText: "󰅖"
              foreground: root.bar.foreground
              tooltipText: "Back"
              onClicked: root.settingsOpen = false
            }
            Text {
              text: "Settings"
              color: root.bar.foreground
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.title
              font.bold: true
              anchors.verticalCenter: parent.verticalCenter
            }
          }
          PanelSeparator { foreground: root.bar.foreground }
          Column {
            width: parent.width
            spacing: Style.space(6)
            PanelSectionHeader { text: "POSITION"; foreground: root.bar.foreground }
            Dropdown {
              width: parent.width
              label: "Bar section"
              value: root.currentBarSection
              options: [{value:"left",label:"Left"},{value:"center",label:"Center"},{value:"right",label:"Right"}]
              foreground: root.bar.foreground
              onChanged: function(v){ root.moveToSection(v) }
            }
          }
        }

        // ---------- Install (pre-first-run) ----------
        Item {
          visible: !root.installed && !root.settingsOpen
          width: parent.width
          implicitHeight: installButton.implicitHeight

          Button {
            id: installButton
            text: "Install Sankey"
            iconText: "󰎓"
            foreground: root.bar.foreground
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            onClicked: root.install()
          }
        }

        // ---------- Live controls (only when installed) ----------
        Column {
          visible: root.installed && !root.settingsOpen
          width: parent.width
          spacing: Style.space(14)

          PanelSeparator { foreground: root.bar.foreground }

          // Keyboard volume
          Column {
            width: parent.width
            spacing: Style.space(6)

            Row {
              width: parent.width
              PanelSectionHeader {
                text: "KEYBOARD VOLUME"
                foreground: root.bar.foreground
                anchors.verticalCenter: parent.verticalCenter
              }
              Item { width: 1 }
              Text {
                anchors.verticalCenter: parent.verticalCenter
                text: Math.round(root.volume) + "%"
                color: root.bar.foreground
                opacity: 0.6
                font.family: root.bar.fontFamily
                font.pixelSize: Style.font.caption
              }
            }

            Item {
              width: parent.width
              implicitHeight: Style.spacing.controlHeight
              PanelSlider {
                id: kbSlider
                bar: root.bar
                anchors.fill: parent
                minimum: 0
                maximum: 100
                integer: true
                value: root.volume
                enabled: root.running
                onReleased: root.setVolume(liveValue)
              }
            }
          }

          // Soundpacks
          PanelSeparator { foreground: root.bar.foreground }

          Column {
            width: parent.width
            spacing: Style.space(10)

            PanelSectionHeader { text: "SOUNDPACKS"; foreground: root.bar.foreground }

            Row {
              width: parent.width
              spacing: Style.space(8)
              Dropdown {
                id: kbPack
                width: parent.width - randomButton.width - parent.spacing
                label: "Keyboard"
                value: root.keyboardPack
                options: Model.packOptions(root.keyboardPacks)
                foreground: root.bar.foreground
                rowHeight: Style.spacing.controlHeight
                enabled: root.running
                onChanged: function(v) { root.setKeyboardPack(v) }
              }
              Button {
                id: randomButton
                text: "Random"
                iconText: "󰒝"
                foreground: root.bar.foreground
                bordered: true
                opacity: 0.9
                y: Style.space(18)
                height: Style.spacing.controlHeight
                verticalPadding: Style.spacing.controlPaddingY
                horizontalPadding: Style.spacing.controlPaddingX
                enabled: root.running && root.keyboardPacks.length > 1
                onClicked: root.pickRandomPack()
              }
            }

            Row {
              width: parent.width
              spacing: Style.space(8)
              Button {
                id: importButton
                text: root.importing ? "Importing…" : "Import pack…"
                iconText: "+"
                foreground: root.bar.foreground
                opacity: root.importing ? 0.5 : 1.0
                enabled: !root.importing
                onClicked: root.triggerImport()
              }
              Button {
                id: openFolderButton
                text: "Open folder"
                iconText: "󰉋"
                foreground: root.bar.foreground
                opacity: 0.7
                onClicked: root.openCustomFolder()
              }
            }

            Text {
              visible: root.importStatus !== ""
              text: root.importStatus
              color: root.bar.foreground
              opacity: 0.6
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }
          }

          // Position
          // Actions
          PanelSeparator { foreground: root.bar.foreground }

          Row {
            width: parent.width
            spacing: Style.space(8)

            Button {
              id: startStopButton
              text: root.running ? "Stop" : "Start"
              iconText: root.running ? "󰓛" : "󰐊"
              foreground: root.bar.foreground
              bordered: true
              onClicked: root.running ? root.stopDaemon() : root.startDaemon()
            }

            Button {
              id: restartButton
              text: "Restart"
              iconText: "󰑐"
              foreground: root.bar.foreground
              bordered: true
              tooltipText: "Restart sankeyd"
              onClicked: root.restartDaemon()
            }

            Item { width: Math.max(0, parent.width - startStopButton.width - restartButton.width - removeButton.width - parent.spacing*2 - Style.space(8)); height: 1 }

            Button {
              id: removeButton
              text: ""
              iconText: ""
              foreground: root.bar.foreground
              opacity: 0.7
              tooltipText: "Uninstall"
              onClicked: root.remove()
            }
          }
        }
      }
    }
  }
}