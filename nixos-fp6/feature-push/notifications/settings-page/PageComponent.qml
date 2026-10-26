/*
 * System Settings > Do Not Disturb (FP6): switch, schedules, exceptions,
 * behaviour, bedtime mode and the notification history.
 * Backend: fp6-notifyd (user service) + GSettings com.fairphone6.notifications;
 * Lomiri and the telephony daemons follow the effective state.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: root
    objectName: "doNotDisturbPage"
    title: i18n.tr("Do Not Disturb")
    flickable: flick

    DndModel { id: dm }
    readonly property var gs: dm.settings

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

            SettingsListItems.Standard {
                text: i18n.tr("Do Not Disturb")
                Switch {
                    objectName: "dndSwitch"
                    property bool serverChecked: gs.dndActive
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: dm.setManual(checked)
                }
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Now: %1. Silenced calls, messages and notifications do not ring, vibrate or turn the screen on; you still find them in the status-bar menus. Alarms always ring. Switching by hand lasts until the next schedule starts or ends.").arg(dm.reasonText())
            }

            SettingsItemTitle { text: i18n.tr("When") }

            SettingsListItems.SingleValueProgression {
                objectName: "schedulesItem"
                text: i18n.tr("Schedules")
                value: {
                    var n = 0
                    for (var i = 0; i < dm.schedules.length; i++) if (dm.schedules[i].enabled) n++
                    return n === 0 ? i18n.tr("None") : i18n.tr("%1 on").arg(n)
                }
                onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("SchedulesPage.qml"), { dm: dm })
            }
            SettingsListItems.SingleValueProgression {
                objectName: "bedtimeItem"
                text: i18n.tr("Bedtime mode")
                value: gs.bedtimeActive ? i18n.tr("On now") : gs.bedtimeEnabled ? i18n.tr("%1 – %2").arg(gs.bedtimeStart).arg(gs.bedtimeEnd) : i18n.tr("Off")
                onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("BedtimePage.qml"), { dm: dm })
            }

            SettingsItemTitle { text: i18n.tr("What gets through") }

            SettingsListItems.SingleValueProgression {
                objectName: "exceptionsItem"
                text: i18n.tr("Calls, messages and apps")
                value: {
                    var names = { everyone: i18n.tr("Everyone"), contacts: i18n.tr("Contacts"), favourites: i18n.tr("Favourites"), nobody: i18n.tr("Nobody") }
                    return names[gs.dndCallsFrom] || ""
                }
                onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("ExceptionsPage.qml"), { dm: dm })
            }

            SettingsItemTitle { text: i18n.tr("Silenced notifications") }

            SettingsListItems.Standard {
                text: i18n.tr("Hide notification bubbles")
                Switch {
                    objectName: "hidePopupsSwitch"
                    property bool serverChecked: gs.dndHidePopups
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.dndHidePopups = checked
                }
            }
            SettingsListItems.Standard {
                text: i18n.tr("Don't turn the screen on")
                Switch {
                    objectName: "noWakeSwitch"
                    property bool serverChecked: gs.dndNoWake
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.dndNoWake = checked
                }
            }

            SettingsItemTitle { text: i18n.tr("History") }

            SettingsListItems.SingleValueProgression {
                objectName: "historyItem"
                text: i18n.tr("Notification history")
                value: gs.historyEnabled ? i18n.tr("On") : i18n.tr("Off")
                onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("HistoryPage.qml"), { dm: dm })
            }

            Item { width: 1; height: units.gu(2); visible: !dm.statusOk }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !dm.statusOk
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: theme.palette.normal.negative
                text: i18n.tr("The fp6-notifyd service is not running: schedules, exceptions and the history do not work.")
            }
        }
    }
}
