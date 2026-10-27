/*
 * Backup > Restore > backup: what the archive holds (read from its index)
 * and a selective restore. The whole archive is verified before anything
 * is written back; current contacts/messages databases are copied to
 * ~/.local/share/fp6-backup/pre-restore first.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import Lomiri.Components.Popups 1.3
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "fp6PreviewPage"
    title: label
    flickable: flick

    property var sm
    property string file: ""
    property string label: ""
    property var info: null
    property bool needsPass: false
    property string error: ""
    property bool loading: false
    property var chosen: []
    property string passphrase: ""
    property var result: null
    readonly property var job: sm ? sm.job : null
    readonly property bool restoring: !!(sm && sm.busy && job.kind === "restore")

    readonly property var names: ({
        "settings": i18n.tr("Settings"), "contacts": i18n.tr("Contacts"), "messages": i18n.tr("Messages and call history"),
        "photos": i18n.tr("Photos and videos"), "documents": i18n.tr("Documents"), "wifi": i18n.tr("Wi-Fi networks")
    })
    readonly property var order: ["settings", "contacts", "messages", "photos", "documents", "wifi"]
    readonly property var cats: {
        var out = []
        if (!info) return out
        for (var i = 0; i < order.length; i++)
            if (info.categories[order[i]]) out.push(order[i])
        return out
    }

    function open(pass) {
        loading = true
        error = ""
        sm.call("backup.preview", { file: file, passphrase: pass || null }, function(e, v) {
            loading = false
            if (e) { error = e; return }
            needsPass = v.needsPassphrase
            if (!v.needsPassphrase) {
                info = v
                passphrase = pass || ""
                chosen = cats.slice()
            }
        })
    }
    Component.onCompleted: open("")

    Connections {
        target: sm
        function onJobFinished(j) {
            if (j.kind === "restore") page.result = j
        }
    }

    function toggle(c, on) {
        var l = chosen.slice()
        var i = l.indexOf(c)
        if (on && i < 0) l.push(c)
        if (!on && i >= 0) l.splice(i, 1)
        chosen = l
    }

    Component {
        id: confirmRestore
        Dialog {
            id: d
            title: i18n.tr("Restore from this backup?")
            text: i18n.tr("Settings, contacts and messages on the phone are replaced by the backup's (a copy of the current contacts and messages is kept). Photos and documents are added; a file with the same name but other content is kept and the restored one is saved next to it. Wi-Fi networks that exist already are left as they are.")
            Button {
                objectName: "fp6ConfirmRestore"
                text: i18n.tr("Restore")
                color: theme.palette.normal.negative
                onClicked: {
                    page.result = null
                    sm.call("backup.restore", { file: page.file, passphrase: page.passphrase || null, categories: page.chosen },
                            function(e) { if (e) page.error = e })
                    PopupUtils.close(d)
                }
            }
            Button { text: i18n.tr("Cancel"); onClicked: PopupUtils.close(d) }
        }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        boundsBehavior: (contentHeight > page.height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick

        Column {
            id: content
            anchors.left: parent.left
            anchors.right: parent.right
            spacing: units.gu(1)

            Item { width: 1; height: units.gu(1) }

            ActivityIndicator {
                anchors.horizontalCenter: parent.horizontalCenter
                running: page.loading
                visible: running
            }

            Column {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                spacing: units.gu(1)
                visible: page.needsPass
                Label {
                    width: parent.width
                    wrapMode: Text.WordWrap
                    text: i18n.tr("This backup was made with another key. Enter its passphrase (fp6-backup-key.age must be in the same folder).")
                }
                TextField {
                    id: pass
                    objectName: "fp6PreviewPass"
                    width: parent.width
                    echoMode: TextInput.Password
                    placeholderText: i18n.tr("Passphrase")
                }
                Button {
                    objectName: "fp6PreviewOpen"
                    text: i18n.tr("Open")
                    enabled: pass.text.length > 0 && !page.loading
                    onClicked: page.open(pass.text)
                }
            }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: page.error !== ""
                wrapMode: Text.WordWrap
                color: theme.palette.normal.negative
                text: page.error
            }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!page.info
                wrapMode: Text.WordWrap
                text: page.info ? i18n.tr("Made %1 · %2 files").arg(sm.when(page.info.created)).arg(page.info.files) : ""
            }

            SettingsItemTitle { text: i18n.tr("Restore"); visible: !!page.info }

            Repeater {
                model: page.cats
                delegate: Column {
                    readonly property var c: page.info.categories[modelData]
                    anchors { left: parent.left; right: parent.right }
                    ListItem.Standard {
                        objectName: "fp6RestoreCat_" + modelData
                        text: page.names[modelData] || modelData
                        showDivider: false
                        control: CheckBox {
                            property bool serverChecked: page.chosen.indexOf(modelData) >= 0
                            onServerCheckedChanged: checked = serverChecked
                            Component.onCompleted: checked = serverChecked
                            onTriggered: page.toggle(modelData, checked)
                        }
                    }
                    Label {
                        anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                        wrapMode: Text.WordWrap
                        fontSize: "small"
                        color: theme.palette.normal.backgroundSecondaryText
                        text: (parent.c.detail ? parent.c.detail + " · " : "") + i18n.tr("%1 files, %2").arg(parent.c.files).arg(sm.size(parent.c.bytes))
                    }
                }
            }

            Column {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                spacing: units.gu(0.5)
                visible: page.restoring
                Label { text: i18n.tr("Checking and restoring…") }
                ProgressBar {
                    width: parent.width
                    minimumValue: 0
                    maximumValue: 1
                    value: job && job.progress ? job.progress : 0
                    showProgressPercentage: false
                }
            }

            Button {
                objectName: "fp6RestoreSelected"
                anchors { left: parent.left; leftMargin: units.gu(2) }
                visible: !!page.info
                enabled: page.chosen.length > 0 && !(sm && sm.busy)
                text: i18n.tr("Restore selected")
                color: theme.palette.normal.positive
                onClicked: PopupUtils.open(confirmRestore, page)
            }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!page.result
                wrapMode: Text.WordWrap
                color: page.result && page.result.state === "failed" ? theme.palette.normal.negative : theme.palette.normal.positive
                text: !page.result ? "" : page.result.state === "failed"
                    ? i18n.tr("Restore failed: %1").arg(page.result.error)
                    : i18n.tr("Restored. %1 unchanged, %2 kept side by side.%3")
                        .arg(page.result.result.unchanged).arg(page.result.result.keptBoth)
                        .arg(page.result.result.warnings.length ? " " + page.result.result.warnings.join("; ") : "")
            }
        }
    }
}
