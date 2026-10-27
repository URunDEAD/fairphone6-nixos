/*
 * System Settings > System update (FP6): the running version, Check now,
 * the available release (changelog, download size), Download / Install now
 * / Install tonight while charging, automatic check and download with the
 * Wi-Fi and charging policy, the previous version and rollback, the last
 * check and the last start check. Backend: fp6-updated (root, system bus).
 * Every partition write is confirmed in a dialog first.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import Lomiri.Components.Popups 1.3
import SystemSettings.ListItems 1.0 as SettingsListItems
import Fp6.Update 1.0

ItemPage {
    id: root
    objectName: "fp6UpdatePage"
    title: i18n.tr("System update")
    flickable: flick

    property UpdateModel um: UpdateModel {}
    readonly property var st: um.status
    readonly property var run: um.running
    readonly property var av: um.available
    readonly property var pend: um.pending
    readonly property var job: um.job
    readonly property bool ab: !!(st && st.abMode)
    readonly property bool jobRunning: !!(job && job.state === "running")
    readonly property var lastInstall: st && st.history && st.history.length > 0 ? st.history[0] : null
    property string message: ""

    function act(method, args) {
        root.message = ""
        um.call(method, args || {}, function(err) { if (err) root.message = err })
    }
    function reasons(list) { return list && list.length ? list.join(", ") : "" }
    function opName(op) {
        return op === "check" ? i18n.tr("Checking for updates…")
            : op === "download" ? i18n.tr("Downloading…")
            : op === "install" ? i18n.tr("Installing…")
            : op === "rollback" ? i18n.tr("Restoring the previous version…")
            : i18n.tr("Working…")
    }

    Connections {
        target: um
        function onJobFinished(j) {
            if (j.state === "failed" || j.state === "cancelled") root.message = j.error
        }
    }

    Component {
        id: confirmDialog
        Dialog {
            id: d
            property string method: ""
            property var args: ({})
            property string action: ""
            title: ""
            text: ""
            Button {
                objectName: "fp6UpdateConfirm"
                text: d.action
                color: theme.palette.normal.positive
                onClicked: { root.act(d.method, d.args); PopupUtils.close(d) }
            }
            Button { text: i18n.tr("Cancel"); onClicked: PopupUtils.close(d) }
        }
    }

    function confirmInstall() {
        PopupUtils.open(confirmDialog, root, {
            title: i18n.tr("Install release %1?").arg(av.release),
            text: ab
                ? i18n.tr("The new version is written to the other slot's boot partition and that slot becomes active. After the restart a start check runs; if the new version does not pass it, the phone returns to this version by itself.")
                : i18n.tr("The boot partition of this slot is backed up and then rewritten. Automatic rollback is off: if the new version does not start, restoring the previous one needs a computer. You can roll back from this page as long as the new version starts."),
            method: "install", args: { when: "now", confirm: true }, action: i18n.tr("Install")
        })
    }
    function confirmTonight() {
        PopupUtils.open(confirmDialog, root, {
            title: i18n.tr("Install tonight?"),
            text: i18n.tr("Between 02:00 and 05:00, while the phone is charging, release %1 is installed and the phone restarts by itself.").arg(av.release)
                + (av.downloaded ? "" : " " + i18n.tr("It is downloaded first (Wi-Fi rules apply).")),
            method: "install", args: { when: "tonight", confirm: true }, action: i18n.tr("Install tonight")
        })
    }
    function confirmRestart() {
        PopupUtils.open(confirmDialog, root, {
            title: i18n.tr("Restart now?"),
            text: i18n.tr("The phone restarts into the installed version."),
            method: "reboot", args: { confirm: true }, action: i18n.tr("Restart")
        })
    }
    function confirmRollback() {
        var undo = !!(pend && pend.kind === "install")
        PopupUtils.open(confirmDialog, root, {
            title: undo ? i18n.tr("Undo the installation?") : i18n.tr("Return to the previous version?"),
            text: undo ? i18n.tr("The installed update is removed from the boot partition again; this version stays.")
                : (ab ? i18n.tr("The other slot (%1) becomes active again. Restart afterwards to use it.").arg(st.slot === "_a" ? "B" : "A")
                      : i18n.tr("The boot partition is restored from the backup taken before the last update. Restart afterwards to use it.")),
            method: "rollback", args: { confirm: true }, action: undo ? i18n.tr("Undo") : i18n.tr("Roll back")
        })
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
                visible: !um.ok && um.error !== ""
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: theme.palette.normal.negative
                text: i18n.tr("The update service is not available: %1").arg(um.error)
            }

            // ------------------------------------------------ this phone
            SettingsItemTitle { text: i18n.tr("This phone") }
            Label {
                objectName: "fp6UpdateVersion"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WrapAnywhere
                text: run ? run.version : i18n.tr("Unknown version")
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: theme.palette.normal.backgroundSecondaryText
                text: run ? i18n.tr("Release %1 · channel %2 · kernel %3 · slot %4")
                            .arg(run.release).arg(run.channel).arg(run.kernel || "?").arg(st.slot ? st.slot.replace("_", "").toUpperCase() : "?") : ""
            }
            Label {
                objectName: "fp6UpdateMode"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!st
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: ab ? theme.palette.normal.backgroundSecondaryText : theme.palette.normal.activity
                text: ab ? i18n.tr("Updates go to the other slot. If a new version fails its start check, the phone returns to this one by itself.")
                    : i18n.tr("Automatic rollback is off. Updates rewrite this slot's boot partition after a backup; if a new version does not start, restoring needs a computer.")
            }

            ListItem.ThinDivider {}

            // ------------------------------------------------ status
            Label {
                objectName: "fp6UpdateLastCheck"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                color: st && st.lastCheck && !st.lastCheck.ok ? theme.palette.normal.negative : theme.palette.normal.backgroundText
                text: !st ? "" : !st.lastCheck ? i18n.tr("Not checked yet.")
                    : i18n.tr("Last check %1: %2").arg(um.when(st.lastCheck.time)).arg(st.lastCheck.message)
            }
            Column {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                spacing: units.gu(0.5)
                visible: root.jobRunning
                Label {
                    text: job ? (job.phase && job.op !== "download" ? job.phase : root.opName(job.op)) : ""
                }
                ProgressBar {
                    objectName: "fp6UpdateProgress"
                    width: parent.width
                    indeterminate: !(job && job.total > 0)
                    minimumValue: 0
                    maximumValue: 1
                    value: job && job.total > 0 ? job.done / job.total : 0
                    showProgressPercentage: false
                }
                Label {
                    visible: !!(job && job.total > 0)
                    fontSize: "small"
                    text: job ? i18n.tr("%1 of %2").arg(um.size(job.done)).arg(um.size(job.total)) : ""
                }
            }
            Row {
                anchors { left: parent.left; leftMargin: units.gu(2) }
                spacing: units.gu(1)
                Button {
                    objectName: "fp6UpdateCheck"
                    text: i18n.tr("Check now")
                    enabled: !!st && !um.busy && !pend
                    onClicked: root.act("check")
                }
                Button {
                    objectName: "fp6UpdateCancel"
                    visible: root.jobRunning && job.op === "download"
                    text: i18n.tr("Cancel")
                    onClicked: root.act("cancel")
                }
            }
            Label {
                objectName: "fp6UpdateMessage"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: root.message !== ""
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: theme.palette.normal.negative
                text: root.message
            }

            // ------------------------------------------------ installed, restart
            Column {
                anchors { left: parent.left; right: parent.right }
                spacing: units.gu(1)
                visible: !!pend
                ListItem.ThinDivider {}
                SettingsItemTitle { text: pend && pend.kind === "rollback" ? i18n.tr("Previous version restored") : i18n.tr("Update installed") }
                Label {
                    objectName: "fp6UpdatePending"
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    wrapMode: Text.WordWrap
                    text: !pend ? "" : pend.kind === "rollback" ? i18n.tr("Restart to return to the previous version.")
                        : i18n.tr("Release %1 is installed. Restart to start using it.").arg(pend.release)
                          + (pend.mode === "ab" ? " " + i18n.tr("It has %1 start attempts before the phone returns to this version.").arg(pend.maxAttempts) : "")
                }
                Row {
                    anchors { left: parent.left; leftMargin: units.gu(2) }
                    spacing: units.gu(1)
                    Button {
                        objectName: "fp6UpdateRestart"
                        text: i18n.tr("Restart now")
                        color: theme.palette.normal.positive
                        enabled: !um.busy
                        onClicked: root.confirmRestart()
                    }
                    Button {
                        objectName: "fp6UpdateUndo"
                        visible: !!pend && pend.kind === "install"
                        text: i18n.tr("Undo")
                        enabled: !um.busy
                        onClicked: root.confirmRollback()
                    }
                }
            }

            // ------------------------------------------------ available
            Column {
                anchors { left: parent.left; right: parent.right }
                spacing: units.gu(1)
                visible: !!av && !pend
                ListItem.ThinDivider {}
                SettingsItemTitle { text: i18n.tr("Update available") }
                Label {
                    objectName: "fp6UpdateAvailable"
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    wrapMode: Text.WrapAnywhere
                    text: av ? i18n.tr("Release %1: %2").arg(av.release).arg(av.version) : ""
                }
                Label {
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    wrapMode: Text.WordWrap
                    fontSize: "small"
                    color: theme.palette.normal.backgroundSecondaryText
                    text: !av ? "" : (av.downloaded ? i18n.tr("Published %1 · downloaded, ready to install").arg(av.date.replace("T", " ").replace("Z", " UTC"))
                        : i18n.tr("Published %1 · download %2 (%3 on the phone)").arg(av.date.replace("T", " ").replace("Z", " UTC"))
                            .arg(um.size(av.downloadBytes)).arg(um.size(av.unpackedBytes)))
                        + (av.kernel && av.kernel.release ? " · " + i18n.tr("kernel %1").arg(av.kernel.release) : "")
                }
                Label {
                    objectName: "fp6UpdateChangelog"
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    visible: !!av && av.changelog !== ""
                    wrapMode: Text.WordWrap
                    fontSize: "small"
                    text: av ? av.changelog : ""
                }
                Label {
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    visible: !!(av && av.kernel && av.kernel.notes)
                    wrapMode: Text.WordWrap
                    fontSize: "x-small"
                    color: theme.palette.normal.backgroundTertiaryText
                    text: av && av.kernel ? av.kernel.notes : ""
                }
                Flow {
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    spacing: units.gu(1)
                    Button {
                        objectName: "fp6UpdateDownload"
                        visible: !!av && !av.downloaded
                        text: i18n.tr("Download")
                        color: theme.palette.normal.positive
                        enabled: !um.busy && !!um.blocked && um.blocked.download.length === 0
                        onClicked: root.act("download")
                    }
                    Button {
                        objectName: "fp6UpdateInstall"
                        visible: !!av && av.downloaded
                        text: i18n.tr("Install now")
                        color: theme.palette.normal.positive
                        enabled: !um.busy && !!um.blocked && um.blocked.install.length === 0
                        onClicked: root.confirmInstall()
                    }
                    Button {
                        objectName: "fp6UpdateTonight"
                        visible: !!av && !(st && st.installTonight)
                        text: i18n.tr("Install tonight while charging")
                        enabled: !um.busy
                        onClicked: root.confirmTonight()
                    }
                }
                Label {
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    visible: !!av && !!um.blocked && ((!av.downloaded && um.blocked.download.length > 0) || (av.downloaded && um.blocked.install.length > 0))
                    wrapMode: Text.WordWrap
                    fontSize: "small"
                    color: theme.palette.normal.activity
                    text: !av || !um.blocked ? "" : (av.downloaded ? i18n.tr("Not now: %1").arg(root.reasons(um.blocked.install))
                                                                  : i18n.tr("Not now: %1").arg(root.reasons(um.blocked.download)))
                }
                SettingsListItems.Standard {
                    objectName: "fp6UpdateTonightSet"
                    visible: !!(st && st.installTonight)
                    text: i18n.tr("Installs tonight (02:00-05:00, while charging)")
                    showDivider: false
                    Button {
                        text: i18n.tr("Cancel")
                        onClicked: root.act("cancelTonight")
                    }
                }
            }

            // ------------------------------------------------ last install result
            Label {
                objectName: "fp6UpdateOutcome"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!lastInstall && !pend && ["failed", "rolled-back", "unhealthy"].indexOf(lastInstall.outcome) >= 0
                wrapMode: Text.WordWrap
                color: theme.palette.normal.negative
                text: !lastInstall ? "" : (lastInstall.outcome === "unhealthy"
                        ? i18n.tr("Release %1 started but failed its start check: %2").arg(lastInstall.release).arg(lastInstall.note)
                        : i18n.tr("Release %1 did not pass its start check; the phone returned to the previous version. %2").arg(lastInstall.release).arg(lastInstall.note))
            }

            ListItem.ThinDivider {}

            // ------------------------------------------------ automatic
            SettingsItemTitle { text: i18n.tr("Automatic updates") }
            Repeater {
                model: [
                    ["autoCheck", i18n.tr("Check for updates automatically")],
                    ["autoDownload", i18n.tr("Download updates automatically")],
                    ["wifiOnly", i18n.tr("Only on Wi-Fi")],
                    ["chargingOnly", i18n.tr("Download automatically only while charging")]
                ]
                delegate: SettingsListItems.Standard {
                    text: modelData[1]
                    showDivider: false
                    Switch {
                        objectName: "fp6Update_" + modelData[0]
                        property bool serverChecked: !!(um.settings && um.settings[modelData[0]])
                        onServerCheckedChanged: checked = serverChecked
                        Component.onCompleted: checked = serverChecked
                        enabled: !!um.settings && (modelData[0] !== "chargingOnly" || um.settings.autoDownload)
                        onTriggered: {
                            var a = {}
                            a[modelData[0]] = checked
                            root.act("setSettings", a)
                        }
                    }
                }
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: theme.palette.normal.backgroundSecondaryText
                text: i18n.tr("Nothing is downloaded while battery saver is on. Updates are only installed after you choose Install now or Install tonight.")
            }

            ListItem.ThinDivider {}

            // ------------------------------------------------ previous version
            SettingsItemTitle { text: i18n.tr("Previous version") }
            Label {
                objectName: "fp6UpdatePrevious"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WrapAnywhere
                fontSize: "small"
                text: !st ? "" : ab ? (st.otherSlotSystem ? i18n.tr("Slot %1: %2").arg(st.slot === "_a" ? "B" : "A").arg(st.otherSlotSystem.replace(/^\/nix\/store\/[a-z0-9]+-/, ""))
                                                          : i18n.tr("The other slot holds no NixOS system."))
                    : (st.previous ? i18n.tr("%1 (boot partition backup kept)").arg(st.previous.version || st.previous.toplevel.replace(/^\/nix\/store\/[a-z0-9]+-/, ""))
                                   : i18n.tr("No backup of a previous version yet: one is made before each update."))
            }
            Button {
                objectName: "fp6UpdateRollback"
                anchors { left: parent.left; leftMargin: units.gu(2) }
                text: i18n.tr("Roll back…")
                enabled: !!st && !um.busy && !pend && (ab ? !!st.otherSlotSystem : !!st.previous)
                onClicked: root.confirmRollback()
            }

            ListItem.ThinDivider {}

            // ------------------------------------------------ start check, server
            SettingsItemTitle { text: i18n.tr("Start check") }
            Label {
                objectName: "fp6UpdateHealth"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: st && st.lastHealth && !st.lastHealth.passed ? theme.palette.normal.negative : theme.palette.normal.backgroundText
                text: !st || !st.lastHealth ? i18n.tr("No start check has run yet.")
                    : (st.lastHealth.passed ? i18n.tr("Passed at %1 (display, Lomiri, critical services, SSH%2).")
                                                  .arg(um.when(st.lastHealth.time)).arg(st.lastHealth.checks.some(function(c) { return c.name === "modem" && c.status === "pass" }) ? i18n.tr(", modem") : "")
                       : i18n.tr("Failed at %1: %2").arg(um.when(st.lastHealth.time))
                             .arg(st.lastHealth.checks.filter(function(c) { return c.critical && c.status === "fail" }).map(function(c) { return c.detail }).join("; ")))
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WrapAnywhere
                fontSize: "x-small"
                color: theme.palette.normal.backgroundTertiaryText
                text: !st ? "" : st.server ? i18n.tr("Update server: %1 (channel %2)").arg(st.server).arg(st.channel)
                                           : i18n.tr("No update server is configured.")
            }
        }
    }
}
