/*
 * System Settings > Backup (FP6): encrypted backups (age, X25519 key
 * protected by the user's passphrase) of settings, contacts, messages and
 * call history, optionally photos/videos, documents and Wi-Fi networks, to
 * a folder on the phone, a memory card or a USB drive; verified after
 * writing; preview and selective restore; optional daily timer (off).
 * Backend: fp6-storage-agent (session), Wi-Fi via fp6-storaged (root).
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import Lomiri.Components.Popups 1.3
import SystemSettings.ListItems 1.0 as SettingsListItems
import Fp6.StorageBackup 1.0

ItemPage {
    id: root
    objectName: "fp6BackupPage"
    title: i18n.tr("Backup")
    flickable: flick

    property AgentModel sm: AgentModel {}
    readonly property var b: sm.backup
    readonly property var cfg: b ? b.config : null
    readonly property var sel: cfg ? cfg.selection : null
    readonly property var job: sm.job
    readonly property bool running: sm.busy
    property var dests: []
    property string message: ""

    function setSel(key, value) {
        var s = {}
        s[key] = value
        sm.call("backup.setConfig", { selection: s }, function(err, v) { root.message = err || ""; sm.refresh() })
    }
    function loadDests() {
        sm.call("backup.destinations", {}, function(err, v) { if (!err) root.dests = v })
    }
    Component.onCompleted: loadDests()

    readonly property var last: b ? b.last : null
    function lastText() {
        if (!last) return i18n.tr("No backup yet.")
        if (!last.ok) return i18n.tr("Last backup failed (%1): %2").arg(sm.when(last.time)).arg(last.error)
        var r = last.result
        return i18n.tr("Last backup: %1, %2 in %3 files, verified.").arg(sm.when(last.time)).arg(sm.size(r.bytes)).arg(r.files)
    }

    Component {
        id: passDialog
        Dialog {
            id: d
            property bool change: false
            title: change ? i18n.tr("Change the backup passphrase") : i18n.tr("Set a backup passphrase")
            text: i18n.tr("Backups can only be opened with this passphrase. It cannot be recovered: write it down and keep it away from the phone.")
            TextField {
                id: oldField
                objectName: "fp6PassOld"
                visible: d.change
                echoMode: TextInput.Password
                placeholderText: i18n.tr("Current passphrase")
            }
            TextField {
                id: newField
                objectName: "fp6PassNew"
                echoMode: TextInput.Password
                placeholderText: i18n.tr("New passphrase (8 characters or more)")
            }
            TextField {
                id: againField
                objectName: "fp6PassAgain"
                echoMode: TextInput.Password
                placeholderText: i18n.tr("Repeat the passphrase")
            }
            Label {
                id: err
                visible: text !== ""
                wrapMode: Text.WordWrap
                color: theme.palette.normal.negative
                text: newField.text.length > 0 && newField.text.length < 8 ? i18n.tr("At least 8 characters.")
                    : (againField.text.length > 0 && againField.text !== newField.text ? i18n.tr("The passphrases differ.") : "")
            }
            Label {
                id: serverErr
                visible: text !== ""
                wrapMode: Text.WordWrap
                color: theme.palette.normal.negative
            }
            Button {
                objectName: "fp6PassSave"
                text: saving ? i18n.tr("Saving…") : i18n.tr("Save")
                property bool saving: false
                color: theme.palette.normal.positive
                enabled: !saving && newField.text.length >= 8 && againField.text === newField.text
                onClicked: {
                    saving = true
                    sm.call("backup.setPassphrase", { old: d.change ? oldField.text : null, new: newField.text }, function(e, v) {
                        saving = false
                        if (e) { serverErr.text = e; return }
                        sm.refresh()
                        PopupUtils.close(d)
                    })
                }
            }
            Button { text: i18n.tr("Cancel"); onClicked: PopupUtils.close(d) }
        }
    }

    Connections {
        target: sm
        function onJobFinished(j) {
            if (j.kind === "backup") root.message = j.state === "failed" ? j.error : ""
        }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        boundsBehavior: (contentHeight > root.height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick

        Column {
            id: content
            anchors.left: parent.left
            anchors.right: parent.right
            spacing: units.gu(1)

            Item { width: 1; height: units.gu(1) }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !sm.ok && sm.error !== ""
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: theme.palette.normal.negative
                text: i18n.tr("Backup is not available: %1").arg(sm.error)
            }

            Label {
                objectName: "fp6BackupLast"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!b
                wrapMode: Text.WordWrap
                color: last && !last.ok ? theme.palette.normal.negative : theme.palette.normal.backgroundText
                text: root.lastText()
            }

            Column {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                spacing: units.gu(0.5)
                visible: root.running && (job.kind === "backup" || job.kind === "restore")
                Label {
                    text: job && job.message === "verifying" ? i18n.tr("Verifying the backup…")
                        : (job && job.kind === "restore" ? i18n.tr("Restoring…") : i18n.tr("Backing up…"))
                }
                ProgressBar {
                    width: parent.width
                    minimumValue: 0
                    maximumValue: 1
                    value: job && job.progress ? job.progress : 0
                    showProgressPercentage: false
                }
            }

            Row {
                anchors { left: parent.left; leftMargin: units.gu(2) }
                spacing: units.gu(1)
                Button {
                    objectName: "fp6BackupNow"
                    text: i18n.tr("Back up now")
                    color: theme.palette.normal.positive
                    enabled: !!b && b.keyExists && !root.running
                    onClicked: sm.call("backup.run", {}, function(e) { root.message = e || "" })
                }
                Button {
                    objectName: "fp6RestoreOpen"
                    text: i18n.tr("Restore…")
                    enabled: !!b && !root.running
                    onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("RestorePage.qml"), { sm: root.sm })
                }
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: root.message !== ""
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: theme.palette.normal.negative
                text: root.message
            }

            ListItem.ThinDivider {}

            // ------------------------------------------------ encryption
            SettingsItemTitle { text: i18n.tr("Encryption") }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!b
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: b && b.keyExists
                    ? i18n.tr("Backups are encrypted (age). To restore on another phone you need the passphrase and the file fp6-backup-key.age, which is saved next to the backups.")
                    : i18n.tr("Set a passphrase to start. Backups are encrypted with it before they leave the phone's storage.")
            }
            Button {
                objectName: "fp6SetPassphrase"
                anchors { left: parent.left; leftMargin: units.gu(2) }
                visible: !!b
                text: b && b.keyExists ? i18n.tr("Change passphrase…") : i18n.tr("Set passphrase…")
                onClicked: PopupUtils.open(passDialog, root, { change: b.keyExists })
            }

            ListItem.ThinDivider {}

            // ------------------------------------------------ what
            SettingsItemTitle { text: i18n.tr("What to back up") }

            Repeater {
                model: [
                    ["settings", i18n.tr("Settings"), i18n.tr("System and app settings (dconf) and the app folders chosen below.")],
                    ["contacts", i18n.tr("Contacts"), ""],
                    ["messages", i18n.tr("Messages and call history"), ""],
                    ["photos", i18n.tr("Photos and videos"), i18n.tr("Pictures and Videos folders. Can be large.")],
                    ["documents", i18n.tr("Documents"), ""],
                    ["wifi", i18n.tr("Wi-Fi networks"), i18n.tr("Includes the Wi-Fi passwords; they are only stored inside the encrypted backup.")]
                ]
                delegate: Column {
                    anchors { left: parent.left; right: parent.right }
                    SettingsListItems.Standard {
                        text: modelData[1]
                        showDivider: false
                        Switch {
                            objectName: "fp6Sel_" + modelData[0]
                            property bool serverChecked: !!(root.sel && root.sel[modelData[0]])
                            onServerCheckedChanged: checked = serverChecked
                            Component.onCompleted: checked = serverChecked
                            enabled: !!root.sel
                            onTriggered: root.setSel(modelData[0], checked)
                        }
                    }
                    Label {
                        anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                        visible: modelData[2] !== ""
                        wrapMode: Text.WordWrap
                        fontSize: "small"
                        color: theme.palette.normal.backgroundSecondaryText
                        text: modelData[2]
                    }
                }
            }

            SettingsListItems.StandardProgression {
                objectName: "fp6ConfigDirs"
                text: i18n.tr("App settings folders")
                enabled: !!(root.sel && root.sel.settings)
                onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("ConfigDirsPage.qml"), { sm: root.sm })
                Label {
                    text: root.sel ? (root.sel.configDirs.length === 0 ? i18n.tr("None") : String(root.sel.configDirs.length)) : ""
                    color: theme.palette.normal.backgroundSecondaryText
                }
            }

            ListItem.ThinDivider {}

            // ------------------------------------------------ where
            SettingsItemTitle { text: i18n.tr("Where") }
            OptionSelector {
                objectName: "fp6Dest"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: root.dests.length > 0
                expanded: true
                model: root.dests.map(function(d) {
                    return d.label + " · " + i18n.tr("%1 free").arg(sm.size(d.free))
                })
                selectedIndex: {
                    if (!b) return 0
                    for (var i = 0; i < root.dests.length; i++) if (root.dests[i].path === b.folder) return i
                    return 0
                }
                onDelegateClicked: {
                    var d = root.dests[index]
                    sm.call("backup.setConfig", { folder: d.removable ? d.path : "" }, function(err) { root.message = err || ""; sm.refresh() })
                }
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!b
                wrapMode: Text.WrapAnywhere
                fontSize: "small"
                color: theme.palette.normal.backgroundTertiaryText
                text: b ? i18n.tr("Folder: %1").arg(b.folder) : ""
            }

            ListItem.ThinDivider {}

            // ------------------------------------------------ schedule
            SettingsItemTitle { text: i18n.tr("Schedule") }
            SettingsListItems.Standard {
                text: i18n.tr("Back up every day")
                showDivider: false
                Switch {
                    objectName: "fp6Daily"
                    property bool serverChecked: !!(cfg && cfg.daily)
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    enabled: !!b && b.keyExists
                    onTriggered: sm.call("backup.setConfig", { daily: checked }, function(err) {
                        root.message = err || ""
                        sm.refresh()
                    })
                }
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Off by default. When on, a backup runs at about 03:30 (or later that day if the phone was off) to the folder above; older backups beyond the number below are deleted after a new one verified.")
            }
            OptionSelector {
                objectName: "fp6Keep"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                readonly property var values: [3, 5, 10]
                model: [i18n.tr("Keep 3 backups"), i18n.tr("Keep 5 backups"), i18n.tr("Keep 10 backups")]
                selectedIndex: cfg ? Math.max(0, values.indexOf(cfg.keep)) : 1
                onDelegateClicked: sm.call("backup.setConfig", { keep: values[index] }, function() { sm.refresh() })
            }
        }
    }
}
