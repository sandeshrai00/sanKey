import QtQuick
import QtQuick.Controls
import Quickshell
import Quickshell.Io
import qs.Ui
import qs.Commons
import "Model.js" as Model

// Sorakey: keyboard sounds from the sorakey daemon.
//
// The bar icon is the entry point and also a fast mute (right click) and a
// volume wheel. Left click opens this panel: live status, mute, volume,
// soundpack picker, and install/start/stop/remove.
//
// State comes from one `sorakey ctl status` every 5 s plus a refresh on open;
// commands are fire-and-forget one-shot `ctl`/`systemctl` runs. No daemon
// state is kept in this file beyond what the last reading says.
Panel {
  id: root
  moduleName: "io.github.sandeshrai00.sorakey"
  ipcTarget: "io.github.sandeshrai00.sorakey"

  readonly property string home: Quickshell.env("HOME")
  readonly property string sorakeyBin: home + "/.local/bin/sorakey"
  readonly property string pluginDir: home + "/.config/omarchy/plugins/io.github.sandeshrai00.sorakey"
  readonly property string setupPath: pluginDir + "/scripts/sorakey-setup"

  // ---- Background service: the shell-managed instance the shell creates for
  // the "service" kind, not a panel-local copy — panel instances come and go
  // with bar rebuilds, and their destruction used to stop the daemon. ----
  readonly property var service: bar?.shell?.firstPartyServiceFor("io.github.sandeshrai00.sorakey")
  // ponytail: reuse shell-injected manifest, 0 fork; commit fetched on demand when Settings opens
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
    // Close first: the GTK file dialog opens as a normal window BELOW this
    // layer-shell overlay, so the full-screen dismissArea would swallow the
    // first click meant for the dialog. Import feedback arrives as a desktop
    // notification (see Service.qml); the panel does not reopen.
    root.close()
    service.importSoundpack()
  }

  // Pack list refresh after an import is covered by the 5 s open-panel poll.

  // ---- State from the last reading ----
  property bool installed: false
  property bool running: false
  property bool muted: false
  property real volume: 100
  property string keyboardPack: ""
  property var keyboardPacks: []
  property real perPackVolume: 100
  property string deleteConfirmId: ""
  property bool deleting: false
  property string deleteToast: ""
  Timer { id: clearDeleteToast; interval: 3000; onTriggered: root.deleteToast = "" }

  readonly property string statusText: {
    if (setupBusy) return "Installing…"
    if (!root.installed) return "Not installed"
    if (!root.running) return "Stopped"
    return root.muted ? "Muted" : "Playing"
  }

  property bool setupBusy: false
  property bool settingsOpen: false
  onSettingsOpenChanged: {
    if (settingsOpen && root.pluginCommit === "" && !commitProc.running)
      commitProc.running = true
  }

  // ponytail: one fork per click, no poll - reused install pattern
  property bool updateBusy: false
  property string updateStatus: ""

  // The bar sizes widgets by their implicit size; the base Panel is a plain
  // Item that does not inherit it, so report the button's.
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  // ---- Commands ----
  function sendCtl(obj) {
    if (!root.installed) return
    if (ctlProc.running) return
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
    // Keyboard volume is per-pack
    if (root.keyboardPack) root.sendCtl({ cmd: "per_pack_volume", id: root.keyboardPack, value: v })
    else root.sendCtl({ cmd: "volume", value: v })
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

  function startDaemon() { root.runService(["start", "sorakey"]); root.refreshStatus() }
  function stopDaemon()  { root.runService(["stop", "sorakey"]);  root.refreshStatus() }
  function restartDaemon() { root.runService(["restart", "sorakey"]); root.refreshStatus() }

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
    // Remember the choice: Omarchy drops the layout entry on disable and
    // re-inserts at the manifest default (right) on re-enable, so the panel
    // restores the user's section from this file (sectionRead).
    if (!sectionWrite.running) {
      sectionWrite.command = ["/bin/sh", "-c",
        "mkdir -p " + root.home + "/.local/share/sorakey && printf %s " + section +
        " > " + root.home + "/.local/share/sorakey/bar-section"]
      sectionWrite.running = true
    }
  }

  // Restore the user's last bar section when the panel is (re)created.
  // No file = user never chose = leave the manifest default in place.
  Process {
    id: sectionRead
    command: ["/bin/sh", "-c", "cat " + root.home + "/.local/share/sorakey/bar-section 2>/dev/null"]
    stdout: StdioCollector { waitForEnd: true }
    onExited: function(exitCode) {
      var saved = String(stdout.text || "").trim()
      if (exitCode === 0 && saved && saved !== root.currentBarSection)
        root.moveToSection(saved)
    }
  }

  Process {
    id: sectionWrite
    stdout: StdioCollector { waitForEnd: true }
    stderr: StdioCollector { waitForEnd: true }
  }

  // ponytail: one git fork only when Settings opened, no poll
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
      var out = String(stdout.text || "").trim()
      var err = String(stderr.text || "").trim()
      if (out.indexOf("is up to date") !== -1) root.updateStatus = "Up to date."
      else if (out.indexOf("Updated") !== -1) root.updateStatus = "Updated."
      else if (exitCode !== 0) root.updateStatus = err !== "" ? err.split("\n").pop() : "Update failed."
      else root.updateStatus = out !== "" ? out.split("\n").pop() : "Done."
      clearUpdateTimer.restart()
    }
  }

  Timer {
    id: clearUpdateTimer
    interval: 5000
    onTriggered: root.updateStatus = ""
  }

  // Auto-select a freshly imported pack: the service already carries the id
  // from the importer's OK:<id> line, so selecting is one ctl away. The panel
  // stays closed — import feedback goes through the desktop notification
  // (see Service.qml importHelper); a cancel emits no signal at all.
  Connections {
    target: root.service
    function onPacksImported(packId) {
      root.refreshPacks()
      if (packId) root.setKeyboardPack("keyboard/" + packId)
    }
  }

  function remove() {
    // Omarchy-native: plugin removal via CLI, daemon via systemd — no bar.run rm -rf
    Quickshell.execDetached(["systemctl", "--user", "disable", "--now", "sorakey"])
    Quickshell.execDetached(["omarchy", "plugin", "remove", "io.github.sandeshrai00.sorakey", "--yes"])
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
      root.perPackVolume = (typeof o.per_pack_volume === "number") ? o.per_pack_volume : root.perPackVolume
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
    sectionRead.running = true
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
    command: ["test", "-x", root.sorakeyBin]
    onExited: function(exitCode) {
      root.installed = (exitCode === 0)
      if (root.installed) {
        root.refreshStatus()
        // The daemon may be auto-starting right now (re-enable, plugin
        // reload): poll briefly until it answers, then go quiet.
        if (!root.running) {
          root.installPollRemaining = 20
          installPollTimer.running = true
        }
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

  // Fire-and-forget command channel to the daemon.
  Process {
    id: ctlProc
    command: [root.sorakeyBin, "ctl", "{}"]
    onExited: function() {
      root.refreshStatus()
      // delete flow: parse fallback for pretty toast
      if (root.deleting || (root.deleteConfirmId !== "" && stdout.text.indexOf("deleted") !== -1)) {
        try {
          var o = JSON.parse(String(stdout.text || "").trim())
          if (o && o.deleted) {
            var delPretty = Model.prettyPackName(String(o.deleted))
            var fb = o.fallback ? String(o.fallback) : ""
            if (fb) root.deleteToast = "Deleted \"" + delPretty + "\" → \"" + Model.prettyPackName(fb) + "\""
            else root.deleteToast = "Deleted \"" + delPretty + "\""
            clearDeleteToast.restart()
          }
        } catch(e) {}
        root.refreshPacks()
      }
    }
    stdout: StdioCollector { waitForEnd: true }
  }

  // systemctl channel for start/stop/enable.
  Process {
    id: svcProc
    command: ["true"]
    onExited: function() { root.refreshStatus() }
    stdout: StdioCollector { waitForEnd: true }
  }

  // Background setup: runs sorakey-setup without opening a terminal.
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
        // Aggressive poll for 10 seconds to catch daemon startup
        root.installPollRemaining = 10
        installPollTimer.running = true
      }
    }
  }

  property int installPollRemaining: 0

  Timer {
    id: installPollTimer
    interval: 500
    repeat: true
    running: false
    onTriggered: {
      if (!root.installed || root.running) {
        running = false
        return
      }
      root.refreshStatus()
      root.installPollRemaining--
      if (root.installPollRemaining <= 0) {
        running = false
      }
    }
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
    tooltipText: "Sorakey — " + root.statusText
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
      // Keyboard volume is per-pack
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
    contentWidth: panel.fittedContentWidth(Style.space(340))
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
            PanelSeparator { foreground: root.bar.foreground }
            Button {
              text: root.updateBusy ? "Updating…" : "Update"
              iconText: root.updateBusy ? "⏳" : "󰚰"
              foreground: root.bar.foreground
              bordered: true
              width: parent.width
              tooltipText: "Update Sorakey plugin"
              enabled: !root.updateBusy
              onClicked: {
                if (root.updateBusy) return
                root.updateBusy = true
                root.updateStatus = "Updating…"
                updateProc.command = ["omarchy","plugin","update","io.github.sandeshrai00.sorakey","--yes"]
                updateProc.running = true
              }
            }
            Text {
              visible: root.updateStatus !== ""
              width: parent.width
              horizontalAlignment: Text.AlignHCenter
              text: root.updateStatus
              color: root.bar.foreground
              opacity: 0.6
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
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
          }
        }

        // ---------- Install (pre-first-run) ----------
        Item {
          visible: !root.installed && !root.settingsOpen
          width: parent.width
          implicitHeight: installButton.implicitHeight

          Button {
            id: installButton
            text: setupBusy ? "Installing…" : "Install Sorakey"
            iconText: setupBusy ? "⏳" : "󰎓"
            foreground: root.bar.foreground
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            enabled: !setupBusy
            onClicked: root.install()
          }
        }

        // ---------- Live controls (only when installed) ----------
        Column {
          visible: root.installed && !root.settingsOpen
          width: parent.width
          spacing: Style.space(14)

          PanelSeparator { foreground: root.bar.foreground }

          // Keyboard volume — per-pack (each pack remembers its own volume)
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
                text: Math.round(root.perPackVolume) + "%"
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
                rowHeight: Style.spacing.controlHeight
                placeholderText: "Search packs…"
                deleteConfirmId: root.deleteConfirmId
                deleting: root.deleting
                toast: root.deleteToast
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
              width: parent.width
              text: root.importStatus
              color: root.bar.foreground
              opacity: 0.6
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }

            // Key tester — global evdev listener already hears keys while this has focus
            Column {
              width: parent.width
              spacing: Style.space(6)
              PanelSectionHeader { text: "TEST TYPING"; foreground: root.bar.foreground }
              TextField {
                width: parent.width
                placeholderText: "Click here and type — hear keys"
                font.family: root.bar.fontFamily
                font.pixelSize: Style.font.caption
              }
              Text {
                width: parent.width
                text: "Uses system listener, no extra process."
                color: root.bar.foreground
                opacity: 0.45
                font.family: root.bar.fontFamily
                font.pixelSize: Style.font.caption
              }
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
              tooltipText: "Restart sorakey"
              onClicked: root.restartDaemon()
            }

            Button {
              id: updateButton
              text: root.updateBusy ? "Updating…" : "Update"
              iconText: root.updateBusy ? "⏳" : "󰚰"
              foreground: root.bar.foreground
              bordered: true
              tooltipText: "Update Sorakey plugin"
              enabled: !root.updateBusy
              onClicked: {
                if (root.updateBusy) return
                root.updateBusy = true
                root.updateStatus = "Updating…"
                updateProc.command = ["omarchy","plugin","update","io.github.sandeshrai00.sorakey","--yes"]
                updateProc.running = true
              }
            }

            Item { width: Math.max(0, parent.width - startStopButton.width - restartButton.width - updateButton.width - removeButton.width - parent.spacing*3 - Style.space(8)); height: 1 }

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

            Text {
              visible: root.updateStatus !== ""
              width: parent.width
              horizontalAlignment: Text.AlignHCenter
              text: root.updateStatus
              color: root.bar.foreground
              opacity: 0.6
              font.family: root.bar.fontFamily
              font.pixelSize: Style.font.caption
              wrapMode: Text.WordWrap
            }
        }
      }
    }
    }
  }
}