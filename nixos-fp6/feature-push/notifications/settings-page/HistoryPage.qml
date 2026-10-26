/*
 * Do Not Disturb > Notification history: off by default; app, title and
 * time (the text only if chosen), kept on the phone for N days.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "notificationHistoryPage"
    property var dm
    readonly property var gs: dm.settings
    readonly property var entries: dm.status && dm.status.history ? dm.status.history.entries : []
    title: i18n.tr("Notification history")
    flickable: list

    readonly property var retentionDays: [1, 7, 30, 90]

    function iconFor(icon) {
        if (!icon) return "image://theme/notification"
        if (icon.indexOf("/") === 0) return "file://" + icon
        if (icon.indexOf("://") > 0) return icon
        return "image://theme/" + icon
    }
    function when(t) {
        var d = new Date(t * 1000)
        var today = new Date()
        if (d.toDateString() === today.toDateString()) return Qt.formatTime(d, "hh:mm")
        return Qt.formatDateTime(d, "dd.MM. hh:mm")
    }

    Component {
        id: clearDialog
        Dialog {
            id: d
            title: i18n.tr("Clear notification history?")
            Button {
                objectName: "clearConfirm"
                text: i18n.tr("Clear")
                color: theme.palette.normal.negative
                onClicked: { gs.historyClear = (gs.historyClear + 1) % 1000000; PopupUtils.close(d) }
            }
            Button { text: i18n.tr("Cancel"); onClicked: PopupUtils.close(d) }
        }
    }

    ListView {
        id: list
        anchors.fill: parent
        model: gs.historyEnabled ? page.entries : []
        header: Column {
            width: list.width
            spacing: units.gu(1)

            SettingsListItems.Standard {
                text: i18n.tr("Keep a notification history")
                Switch {
                    objectName: "historyEnabled"
                    property bool serverChecked: gs.historyEnabled
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.historyEnabled = checked
                }
            }
            Label {
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Records which app notified, the title and the time, on this phone only. Turning it off deletes the history.")
            }
            SettingsListItems.Standard {
                visible: gs.historyEnabled
                text: i18n.tr("Also keep the text")
                Switch {
                    objectName: "historyBody"
                    property bool serverChecked: gs.historyBody
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.historyBody = checked
                }
            }
            Label {
                visible: gs.historyEnabled
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("The text can contain message contents. Turning this off removes the stored texts.")
            }
            OptionSelector {
                objectName: "historyRetention"
                visible: gs.historyEnabled
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                text: i18n.tr("Keep for")
                model: [i18n.tr("1 day"), i18n.tr("7 days"), i18n.tr("30 days"), i18n.tr("90 days")]
                selectedIndex: Math.max(0, page.retentionDays.indexOf(gs.historyRetentionDays))
                onDelegateClicked: gs.historyRetentionDays = page.retentionDays[index]
            }
            Button {
                objectName: "historyClear"
                visible: gs.historyEnabled
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                text: i18n.tr("Clear history")
                enabled: page.entries.length > 0
                onClicked: PopupUtils.open(clearDialog, page)
            }
            Repeater {
                model: gs.historyEnabled ? (gs.historyExcludeApps || []) : []
                delegate: SettingsListItems.Standard {
                    text: i18n.tr("Not recorded: %1").arg(modelData)
                    Button {
                        text: i18n.tr("Record again")
                        onClicked: gs.historyExcludeApps = page.dm.listWith(gs.historyExcludeApps, modelData, false)
                    }
                }
            }
            SettingsItemTitle {
                visible: gs.historyEnabled
                text: page.entries.length === 0 ? i18n.tr("No notifications yet") : i18n.tr("Latest first")
            }
        }

        delegate: ListItem {
            objectName: "historyEntry" + index
            height: layout.height + (divider.visible ? divider.height : 0)
            ListItemLayout {
                id: layout
                title.text: modelData.summary || modelData.app
                subtitle.text: modelData.app + " · " + page.when(modelData.t) + (modelData.tags && modelData.tags.indexOf("dnd") >= 0 ? " · " + i18n.tr("during Do Not Disturb") : "")
                summary.text: modelData.body || ""
                summary.maximumLineCount: 3
                Image {
                    SlotsLayout.position: SlotsLayout.Leading
                    width: units.gu(4); height: width
                    sourceSize.width: width; sourceSize.height: height
                    fillMode: Image.PreserveAspectFit
                    source: page.iconFor(modelData.icon)
                }
            }
            trailingActions: ListItemActions {
                actions: [
                    Action {
                        iconName: "notification"
                        text: i18n.tr("Don't record this app")
                        onTriggered: gs.historyExcludeApps = page.dm.listWith(gs.historyExcludeApps || [], modelData.app, true)
                    }
                ]
            }
        }
    }
}
