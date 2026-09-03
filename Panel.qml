import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import qs.Ui
import qs.Commons
import "Model.js" as Model

// Sorakey panel — status, mute/volume, soundpacks, install controls.
// Polls `sorakey ctl status` every second (open or closed); ctl/systemctl are one-shot.
Panel {
  id: root
  moduleName: "io.github.sandeshrai00.sorakey"
  ipcTarget: "io.github.sandeshrai00.sorakey"

  readonly property string home: Quickshell.env("HOME")
  readonly property string sorakeyBin: home + "/.local/bin/sorakey"
  readonly property string pluginDir: home + "/.config/omarchy/plugins/io.github.sandeshrai00.sorakey"
  readonly property string setupPath: pluginDir + "/scripts/sorakey-setup"

  // shell-managed service — survives panel rebuilds
  readonly property var service: bar?.shell?.firstPartyServiceFor("io.github.sandeshrai00.sorakey")
  readonly property string pluginVersion: service && service.manifest && service.manifest.version ? String(service.manifest.version) : ""
  property string pluginCommit: ""

  property bool importing: service ? service.importing : false
  property string importStatus: {
    if (importing) return "Importing…"
    if (!service) return ""
    if (service.lastImportError) return service.lastImportError
    if (service.lastImportResult) return "Imported: " + service.lastImportResult
    return ""
  }

  function triggerImport() {
    if (!service) return
    // close first — dialog is below the overlay
    root.close()
    service.importSoundpack()
  }

  property bool exporting: service ? service.exporting : false
  property string exportStatus: {
    if (exporting) return "Exporting…"
    if (!service) return ""
    if (service.lastExportError) return service.lastExportError
    if (service.lastExportResult) return "Saved to " + service.lastExportResult
    return ""
  }

  function triggerExport() {
    if (!service) return
    root.close()
    service.exportLogs()
  }

  // last reading
  property bool installed: false
  property bool running: false
  property bool muted: false
  property real volume: 100
  property string keyboardPack: ""
  property var keyboardPacks: []
  property real perPackVolume: 100
  property string deleteConfirmId: ""
  property bool deleting: false
  property string errorToast: ""
  // single persistent result slot: every result feed mirrors here, so the
  // panel shows the latest outcome until the next one (no auto-clear)
  property string lastResult: ""
  onImportStatusChanged: if (root.importStatus !== "") root.lastResult = root.importStatus
  onExportStatusChanged: if (root.exportStatus !== "") root.lastResult = root.exportStatus
  onErrorToastChanged: if (root.errorToast !== "") root.lastResult = root.errorToast
  onUpdateStatusChanged: if (root.updateStatus !== "") root.lastResult = root.updateStatus
  property string pendingCtlCmd: ""
  property var audioDevices: []
  property string audioDeviceSelected: ""
  Timer { id: clearErrorToast; interval: 5000; onTriggered: root.errorToast = "" }

  readonly property string statusText: {
    if (setupBusy) return "Installing…"
    if (!root.installed) return "Not installed"
    if (!root.running) return "Stopped"
    if (root.keyboardPack === "" && root.keyboardPacks.length === 0) return "No soundpack"
    return root.muted ? "Muted" : "Playing"
  }

  property bool setupBusy: false
  property bool settingsOpen: false
  property bool uninstallArmed: false
  Timer { id: disarmUninstall; interval: 5000; onTriggered: root.uninstallArmed = false }
  onSettingsOpenChanged: {
    if (!settingsOpen) root.uninstallArmed = false
    if (settingsOpen && root.pluginCommit === "" && !commitProc.running)
      commitProc.running = true
  }

  property bool updateBusy: false
  property string updateStatus: ""

  // bar uses implicit size
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  function sendCtl(obj) {
    if (!root.installed) return
    if (ctlProc.running) return
    pendingCtlCmd = String(obj && obj.cmd ? obj.cmd : "")
    ctlProc.command = [root.sorakeyBin, "ctl", JSON.stringify(obj)]
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

  function setPerPackVolume(v) {
    root.perPackVolume = v
    if (root.keyboardPack) root.sendCtl({ cmd: "per_pack_volume", id: root.keyboardPack, value: v })
    else root.sendCtl({ cmd: "volume", value: v })
  }

  function resetVolume() {
    if (!root.keyboardPack) return
    root.sendCtl({ cmd: "reset_volume", id: root.keyboardPack })
  }

  function deletePack(id) {
    if (!id || root.deleting) return
    root.deleting = true
    root.sendCtl({ cmd: "delete_pack", id: id })
  }

  function pickRandomPack() {
    if (!root.keyboardPacks || root.keyboardPacks.length === 0) return
    var pool = root.keyboardPacks
    if (pool.length > 1 && root.keyboardPack) pool = pool.filter(function(id){ return id !== root.keyboardPack })
    var pick = pool[Math.floor(Math.random()*pool.length)]
    if (pick) root.setKeyboardPack(pick)
  }

  function startDaemon() {
    if (stopFlagProc.running) return
    stopFlagProc.command = ["/usr/bin/bash", "-c", "rm -f " + root.home + "/.local/share/sorakey/stopped"]
    stopFlagProc.running = true
    root.runService(["start", "sorakey"])
  }
  function stopDaemon() {
    // sticky stop — Service must not auto-restart what the user stopped
    if (stopFlagProc.running) return
    stopFlagProc.command = ["/usr/bin/bash", "-c", "mkdir -p " + root.home + "/.local/share/sorakey && printf stopped > " + root.home + "/.local/share/sorakey/stopped"]
    stopFlagProc.running = true
    root.runService(["stop", "sorakey"])
  }
  function restartDaemon() {
    if (stopFlagProc.running) return
    stopFlagProc.command = ["/usr/bin/bash", "-c", "rm -f " + root.home + "/.local/share/sorakey/stopped"]
    stopFlagProc.running = true
    root.runService(["restart", "sorakey"])
  }
  function doUpdate() { if (root.updateBusy) return; root.updateBusy=true; root.updateStatus="Updating…"; updateProc.command=["omarchy","plugin","update","io.github.sandeshrai00.sorakey","--yes"]; updateProc.running=true }

  function install() {
    if (setupBusy) return
    setupBusy = true
    setupProc.command = ["/usr/bin/bash", root.setupPath]
    setupProc.running = true
  }

  function openCustomFolder() {
    var path = home + "/.local/share/sorakey/soundpacks"
    Quickshell.execDetached(["xdg-open", path])
  }

  readonly property string currentBarSection: {
    var cfg = root.bar && root.bar.shell ? root.bar.shell.shellConfig : null
    var layout = cfg && cfg.bar && cfg.bar.layout ? cfg.bar.layout : null
    if (!layout) return "right"
    var id = "io.github.sandeshrai00.sorakey"
    for (var s of ["left","center","right"]) {
      var arr = layout[s]
      if (!Array.isArray(arr)) continue
      for (var i=0;i<arr.length;i++) if (arr[i] && arr[i].id===id) return s
    }
    return "right"
  }

  function moveToSection(section) {
    if (["left","center","right"].indexOf(section)===-1) return
    Quickshell.execDetached(["omarchy","plugin","enable","io.github.sandeshrai00.sorakey","--section",section])
    // save choice — Omarchy resets to right on re-enable
    if (!sectionWrite.running) {
      sectionWrite.command = [root.sorakeyBin, "ctl", "{\"cmd\":\"set_bar_section\",\"section\":\"" + section + "\"}"]
      sectionWrite.running = true
    }
  }

  // restore saved bar section
  Process {
    id: sectionRead
    command: [root.sorakeyBin, "ctl", "{\"cmd\":\"get_bar_section\"}"]
    stdout: StdioCollector { waitForEnd: true }
    onExited: function(exitCode) {
      try {
        var resp = JSON.parse(String(stdout.text || "").trim())
        if (resp.ok && resp.section && resp.section !== root.currentBarSection)
          root.moveToSection(resp.section)
      } catch(e) {}
    }
  }

  Process {
    id: sectionWrite
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { waitForEnd: true }
  }

  Process {
    id: commitProc
    command: ["git", "-C", root.pluginDir, "rev-parse", "--short", "HEAD"]
    stdout: StdioCollector { waitForEnd: true }
    onExited: function(exitCode) {
      if (exitCode === 0) root.pluginCommit = String(stdout.text || "").trim()
    }
  }

  Process {
    id: updateProc
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { waitForEnd: true }
    onExited: function(exitCode) {
      root.updateBusy = false
      var err = String(stderr.text || "").trim()
      if (exitCode === 0) root.updateStatus = String(stdout.text || "").trim().split("\n").pop()
      else root.updateStatus = err !== "" ? err : "Update failed."
      clearUpdateTimer.restart()
    }
  }



  Timer {
    id: clearUpdateTimer
    interval: 5000
    onTriggered: root.updateStatus = ""
  }

  // auto-select imported pack
  Connections {
    target: root.service
    function onPacksImported(packId) {
      root.refreshPacks()
      if (packId) root.setKeyboardPack("keyboard/" + packId)
    }
  }

  function remove() {
    // remove via CLI + systemd
    Quickshell.execDetached(["systemctl", "--user", "disable", "--now", "sorakey"])
    Quickshell.execDetached(["omarchy", "plugin", "remove", "io.github.sandeshrai00.sorakey", "--yes"])
    root.installed = false
    root.running = false
  }

  function applyStatus(text) {
    var o = Model.parseStatus(text)
    if (!o) {
      // no reading
      return
    }
    if (o.ok === true) {
      var daemonJustUp = !root.running
      root.running = true
      root.installed = true
      root.muted = o.muted === true
      root.volume = (typeof o.volume === "number") ? o.volume : root.volume
      root.perPackVolume = (typeof o.per_pack_volume === "number") ? o.per_pack_volume : root.perPackVolume
      root.keyboardPack = String(o.keyboard_pack || "")
      if (typeof o.audio_device !== "undefined") root.audioDeviceSelected = o.audio_device ? String(o.audio_device) : ""
      // daemon just came (back) up: ctl works now, so (re)load the device list
      if (daemonJustUp) root.refreshAudioDevices()
    } else {
      root.running = false
    }
  }

  function refreshStatus() {
    if (statusProc.running) return
    statusProc.running = true
  }

  function refreshPacks() {
    if (!root.installed) return
    if (packsProc.running) return
    packsProc.running = true
  }

  function refreshAudioDevices() {
    if (!root.installed) return
    if (devicesProc.running) return
    devicesProc.running = true
  }

  function setAudioDevice(id) {
    // empty string = system default
    root.audioDeviceSelected = id
    root.sendCtl({ cmd: "select_device", id: id === "" ? null : id })
  }

  property bool automaticSetupAttempted: false

  Component.onCompleted: {
    installCheck.running = true
    root.refreshStatus()
    root.refreshAudioDevices()
    sectionRead.running = true
  }

  onOpenedChanged: {
      if (root.opened) {
        root.refreshStatus()
        root.refreshPacks()
        root.settingsOpen = false
        // clear the typing test box on every open
        Qt.callLater(function() { if (testType) testType.text = "" })
      }
   }

  // Detect (re)install without a daemon: is the binary there?
  Process {
    id: installCheck
    command: ["test", "-x", root.sorakeyBin]
    onExited: function(exitCode) {
      root.installed = (exitCode === 0)
      if (root.installed) {
        root.refreshStatus()
        root.refreshAudioDevices()
      } else if (!root.automaticSetupAttempted && !setupBusy) {
        root.automaticSetupAttempted = true
        // Auto-run setup on first enable after URL install (like Spotify)
        Qt.callLater(function(){ root.install() })
      }
    }
  }

  Process {
    id: statusProc
    command: [root.sorakeyBin, "ctl", "{\"cmd\":\"status\"}"]
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
    command: [root.sorakeyBin, "ctl", "{\"cmd\":\"packs\"}"]
    onExited: function(exitCode) {
      if (exitCode !== 0) {
        if (root.deleting) { root.deleting = false }
        return
      }
      var p = Model.parsePacks(stdout.text)
      root.keyboardPacks = p.keyboard
      if (root.deleting) {
        root.deleting = false
        root.deleteConfirmId = ""
      }
    }
    stdout: StdioCollector { waitForEnd: true }
  }

  // Audio output devices
  Process {
    id: devicesProc
    command: [root.sorakeyBin, "ctl", "{\"cmd\":\"audio_devices\"}"]
    running: false
    onExited: function(exitCode, exitStatus) {
      var out = String(devicesProc.stdout || "").trim()
      var opts = null
      try {
        var r = JSON.parse(out)
        var devs = (r && r.ok && Array.isArray(r.devices)) ? r.devices : []
        // normalize to [{value,label}] + prepend System default
        opts = devs.map(function(d){ return { value: String(d.id), label: String(d.name) } })
      } catch (e) {
        opts = []
      }
      opts.unshift({ value: "", label: "System default" })
      root.audioDevices = opts
    }
  }

  Process {
    id: ctlProc
    command: [root.sorakeyBin, "ctl", "{}"]
    onExited: function() {
      root.refreshStatus()
      // show ctl failures the daemon reports
      if (stdout.text.indexOf("\"ok\":false") !== -1) {
        try {
          var e = JSON.parse(String(stdout.text || "").trim())
          if (e && e.ok === false) {
            root.errorToast = (root.pendingCtlCmd !== "" ? root.pendingCtlCmd + ": " : "") + String(e.error || "command failed")
            clearErrorToast.restart()
          }
        } catch(err) {}
      }
      root.pendingCtlCmd = ""
      if (root.deleting || (root.deleteConfirmId !== "" && stdout.text.indexOf("deleted") !== -1)) {
        try {
          var o = JSON.parse(String(stdout.text || "").trim())
          if (o && o.deleted) {
            var delPretty = Model.prettyPackName(String(o.deleted))
            var fb = o.fallback ? String(o.fallback) : ""
            if (fb) root.errorToast = "Deleted \"" + delPretty + "\" → \"" + Model.prettyPackName(fb) + "\""
            else root.errorToast = "Deleted \"" + delPretty + "\""
            clearErrorToast.restart()
          }
        } catch(e) {}
        root.refreshPacks()
      }
    }
    stdout: StdioCollector { waitForEnd: true }
  }

  Process {
    id: svcProc
    command: ["true"]
    onExited: function() { root.refreshStatus() }
    stdout: StdioCollector { waitForEnd: true }
  }

  Process {
    id: stopFlagProc
    command: ["true"]
  }

  // runs sorakey-setup in background
  Process {
    id: setupProc
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { waitForEnd: true }
    onExited: function(exitCode) {
      root.setupBusy = false
      if (exitCode === 0) {
        root.installed = true
        root.refreshStatus()
        root.refreshPacks()
      }
    }
  }

  // poll when open: status every second, packs every 30s (packs change only on
  // import/delete)
  Timer {
    interval: 1000
    repeat: true
    running: root.installed && root.opened
    onTriggered: root.refreshStatus()
  }
  Timer {
    interval: 30000
    repeat: true
    running: root.installed && root.opened
    onTriggered: root.refreshPacks()
  }
  // light poll when closed (must NOT require root.running: this is the timer
  // that detects the daemon coming up after install/enable/restart)
  Timer {
    interval: 1000
    repeat: true
    running: root.installed && !root.opened
    onTriggered: root.refreshStatus()
  }

  // recheck binary for auto-setup
  Timer {
    interval: 5000
    repeat: true
    running: !root.installed && !setupBusy
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
    tooltipText: "Sorakey — " + root.statusText + "\nRight-click: Mute\nCtrl+Alt+M: Global mute"
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
      var cur = root.keyboardPack ? root.perPackVolume : root.volume
      var v = Math.max(0, Math.min(100, cur + step))
      if (root.keyboardPack) root.setPerPackVolume(v)
      else root.setVolume(v)
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    gap: Style.gapsOut + Style.space(6)
    contentWidth: panel.fittedContentWidth(Style.space(400))
    contentHeight: panel.fittedContentHeight(panelColumn.implicitHeight, Style.space(520))
    focusTarget: keyCatcher

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }

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

        // header
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
              text: "Sorakey"
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
            enabled: root.running
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

        // settings
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
            PanelSectionHeader { text: "PANEL"; foreground: root.bar.foreground }
            Dropdown {
              width: parent.width
              label: "Bar section"
              value: root.currentBarSection
              options: [{value:"left",label:"Left"},{value:"center",label:"Center"},{value:"right",label:"Right"}]
              foreground: root.bar.foreground
              popupBorder: "transparent"
              opacity: root.muted ? 0.5 : 1.0
              onChanged: function(v){ root.moveToSection(v) }
            }
            PanelSeparator { foreground: root.bar.foreground }
            PanelSectionHeader { text: "AUDIO OUTPUT"; foreground: root.bar.foreground }
            Dropdown {
              width: parent.width
              label: "Device"
              value: root.audioDeviceSelected
              options: root.audioDevices
              foreground: root.bar.foreground
              popupBorder: "transparent"
              opacity: root.muted ? 0.5 : 1.0
              onChanged: function(v){ root.setAudioDevice(v) }
            }
            Button {
              text: "Rescan devices"
              foreground: root.bar.foreground
              width: parent.width
              onClicked: root.refreshAudioDevices()
            }
            PanelSeparator { foreground: root.bar.foreground }
            PanelSectionHeader { text: "MAINTENANCE"; foreground: root.bar.foreground }
            Button {
              text: root.exporting ? "Exporting…" : "Export error logs"
              iconText: root.exporting ? "󰑐" : "󰈯"
              iconSpinning: root.exporting
              foreground: root.bar.foreground
              width: parent.width
              tooltipText: "Save a report of recent errors to a file"
              enabled: !root.exporting && root.installed
              onClicked: root.triggerExport()
            }
            Button {
              text: root.updateBusy ? "Updating…" : "Update"
              iconText: root.updateBusy ? "󰑐" : "󰚰"
              iconSpinning: root.updateBusy
              foreground: root.bar.foreground
              width: parent.width
              tooltipText: "Update Sorakey plugin"
              enabled: !root.updateBusy
              onClicked: root.doUpdate()
            }
            Text {
              visible: root.pluginVersion !== ""
              width: parent.width
              horizontalAlignment: Text.AlignHCenter
              text: root.pluginCommit !== "" ? "v" + root.pluginVersion + " · " + root.pluginCommit : "v" + root.pluginVersion
              color: root.bar.foreground
              opacity: 0.45
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.caption
            }
            Text {
              visible: root.lastResult !== ""
              width: parent.width
              horizontalAlignment: Text.AlignHCenter
              text: "> " + root.lastResult
              color: root.bar.foreground
              opacity: 0.6
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }
            PanelSeparator { foreground: root.bar.foreground }
            PanelSectionHeader { text: "DANGER"; foreground: root.bar.foreground }
            Button {
              text: root.uninstallArmed ? "Tap again to confirm" : "Uninstall Sorakey"
              iconText: "✕"
              foreground: "#ff6b6b"
              width: parent.width
              tooltipText: "Remove the plugin and stop the daemon"
              onClicked: {
                if (!root.uninstallArmed) { root.uninstallArmed = true; disarmUninstall.restart() }
                else { root.uninstallArmed = false; root.remove() }
              }
            }
          }
        }

        // install prompt
        Item {
          visible: !root.installed && !root.settingsOpen
          width: parent.width
          implicitHeight: installButton.implicitHeight

          Button {
            id: installButton
            text: setupBusy ? "Installing…" : "Install Sorakey"
            iconText: setupBusy ? "󰑐" : "󰎓"
              iconSpinning: setupBusy
            foreground: root.bar.foreground
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            enabled: !setupBusy
            onClicked: root.install()
          }
        }

        // controls
        Column {
          visible: root.installed && !root.settingsOpen
          width: parent.width
          spacing: Style.space(14)

          PanelSeparator { foreground: root.bar.foreground }

          // keyboard volume — per pack
          Column {
            width: parent.width
            spacing: Style.space(6)

            Row {
              width: parent.width
              PanelSectionHeader {
                text: "KEYBOARD VOLUME: "
                foreground: root.bar.foreground
                anchors.verticalCenter: parent.verticalCenter
              }
              Item { width: 1 }
              Item {
                  implicitWidth: volLabel.implicitWidth
                  implicitHeight: Style.spacing.controlHeight
                  Text {
                    id: volLabel
                    anchors.left: parent.left
                    anchors.verticalCenter: parent.verticalCenter
                    text: Math.round(root.perPackVolume) + "%"
                    color: root.bar.foreground
                    opacity: volHover.containsMouse ? 1.0 : (root.keyboardPack !== "" ? 0.85 : 0.6)
                    font.family: root.bar.fontFamily
                    font.pixelSize: Style.font.body
                    font.bold: true
                  }
                  MouseArea {
                    id: volHover
                    anchors.fill: parent
                    anchors.margins: -Style.space(4)
                    visible: root.keyboardPack !== ""
                    cursorShape: Qt.PointingHandCursor
                    hoverEnabled: true
                    ToolTip.text: "Reset to pack default"
                    ToolTip.visible: containsMouse
                    ToolTip.delay: 400
                    onClicked: root.resetVolume()
                  }
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
                value: root.perPackVolume
                enabled: root.running && root.keyboardPack !== ""
                onReleased: root.setPerPackVolume(liveValue)
              }
            }
          }

          // Soundpacks
          PanelSeparator { foreground: root.bar.foreground }

          Column {
            width: parent.width
            spacing: Style.space(10)

            PanelSectionHeader { text: "SOUNDPACKS"; foreground: root.bar.foreground }

            Text {
              visible: root.keyboardPack === "" && root.keyboardPacks.length === 0
              width: parent.width
              text: "Import a pack to get started"
              color: root.bar.foreground
              opacity: 0.6
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.caption
            }

            Row {
              width: parent.width
              spacing: Style.space(8)
              SearchablePackDropdown {
                id: kbPack
                width: parent.width - randomButton.width - parent.spacing
                label: "Keyboard"
                value: root.keyboardPack
                options: Model.packOptions(root.keyboardPacks)
                foreground: root.bar.foreground
                popupBorder: "transparent"
                opacity: root.muted ? 0.5 : 1.0
                rowHeight: Style.spacing.controlHeight
                placeholderText: "Search packs…"
                deleteConfirmId: root.deleteConfirmId
                deleting: root.deleting
                toast: root.errorToast
                onChanged: function(v) { root.setKeyboardPack(v) }
                onDeleteRequested: function(v) { root.deleteConfirmId = v }
                onConfirmDelete: function(v) { root.deletePack(v) }
                onCancelDelete: function() { root.deleteConfirmId = "" }
              }
              Button {
                id: randomButton
                text: "Random"
                iconText: "󰒝"
                foreground: root.bar.foreground
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
              visible: root.lastResult !== ""
              width: parent.width
              text: "> " + root.lastResult
              color: root.bar.foreground
              opacity: 0.6
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }

            // typing test — the daemon listens system-wide, so physical
            // keystrokes while the panel is open play through this box
            Column {
               width: parent.width
               spacing: Style.space(6)
                PanelSectionHeader { text: "TEST TYPING"; foreground: root.bar.foreground }
                TextField {
                  id: testType
                  width: parent.width
                  text: ""
                  placeholderText: "Click here and type — hear keys"
                  font.family: root.bar.fontFamily
                  font.pixelSize: Style.font.caption
                }
              }
          }

          PanelSeparator { foreground: root.bar.foreground }

          Row {
            width: parent.width
            spacing: Style.space(8)

              BorderSurface {
                // divider-colored ring (same 12% tint as PanelSeparator)
                radius: Style.cornerRadius
                color: "transparent"
                borderSpec: Border.flat(Qt.rgba(root.bar.foreground.r, root.bar.foreground.g, root.bar.foreground.b, 0.12), 1)
                implicitWidth: startStopButton.implicitWidth + 2
                implicitHeight: startStopButton.implicitHeight + 2
                Button {
                  id: startStopButton
                  anchors.fill: parent
                  anchors.margins: 1
                  text: root.running ? "Stop" : "Start"
                  iconText: root.running ? "󰓛" : "󰐊"
                  foreground: root.bar.foreground
                  onClicked: root.running ? root.stopDaemon() : root.startDaemon()
                }
              }

              BorderSurface {
                // divider-colored ring (same 12% tint as PanelSeparator)
                radius: Style.cornerRadius
                color: "transparent"
                borderSpec: Border.flat(Qt.rgba(root.bar.foreground.r, root.bar.foreground.g, root.bar.foreground.b, 0.12), 1)
                implicitWidth: restartButton.implicitWidth + 2
                implicitHeight: restartButton.implicitHeight + 2
                Button {
                  id: restartButton
                  anchors.fill: parent
                  anchors.margins: 1
                  text: "Restart"
                  iconText: "󰑐"
                  foreground: root.bar.foreground
                  tooltipText: "Restart sorakey"
                  onClicked: root.restartDaemon()
                }
              }

           }

        }
      }
    }
    }
  }
}