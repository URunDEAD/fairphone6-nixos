/*
 * Battery > Battery usage: measured drain (screen on/off, GPS), on-time of
 * the big consumers (screen, cellular, GPS, Wi-Fi, Bluetooth) and CPU time
 * and wakeups per app/service (cgroup accounting by fp6-batteryd).
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: root
    objectName: "fp6UsagePage"
    title: i18n.tr("Battery usage")
    flickable: flick
    property var m: null
    property bool week: false
    readonly property var u: m.history ? (week ? m.history.usageWeek : m.history.usageDay) : null

    Component.onCompleted: { m.wantHistory = true; m.refresh() }
    Component.onDestruction: m.wantHistory = false

    function onTime(key) {
        return u && u.resources[key] ? m.duration(u.resources[key]) : m.duration(0)
    }
    function drain(key) {
        var d = u ? u.drain[key] : null
        return d && d.seconds > 300 ? i18n.tr("%1 mA average over %2").arg(Math.round(d.mA)).arg(m.duration(d.seconds))
                                    : i18n.tr("not enough time on battery yet")
    }
    function kindName(k) {
        return k === "app" ? i18n.tr("App") : k === "user-service" ? i18n.tr("Session service")
             : k === "system" ? i18n.tr("System service") : k === "kernel" ? i18n.tr("Kernel") : i18n.tr("Other")
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        boundsBehavior: (contentHeight > root.height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds

        Column {
            id: content
            anchors { left: parent.left; right: parent.right }

            Row {
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: units.gu(1)
                topPadding: units.gu(1)
                Button {
                    objectName: "fp6UsageDay"
                    text: i18n.tr("24 hours")
                    color: !root.week ? theme.palette.normal.positive : theme.palette.normal.base
                    onClicked: root.week = false
                }
                Button {
                    objectName: "fp6UsageWeek"
                    text: i18n.tr("7 days")
                    color: root.week ? theme.palette.normal.positive : theme.palette.normal.base
                    onClicked: root.week = true
                }
            }

            SettingsItemTitle { text: i18n.tr("Measured drain on battery") }
            SettingsListItems.SingleValue { text: i18n.tr("Screen on"); value: root.drain("screen-on") }
            SettingsListItems.SingleValue { text: i18n.tr("Screen off"); value: root.drain("screen-off") }
            SettingsListItems.SingleValue { text: i18n.tr("With GPS on"); value: root.drain("gps-on"); visible: u && u.drain["gps-on"] !== undefined }

            SettingsItemTitle { text: i18n.tr("Biggest consumers (time on)") }
            SettingsListItems.SingleValue { objectName: "fp6UsageScreen"; text: i18n.tr("Screen"); value: root.onTime("screen") }
            SettingsListItems.SingleValue { text: i18n.tr("Cellular radio"); value: root.onTime("cellular") }
            SettingsListItems.SingleValue { text: i18n.tr("GPS"); value: root.onTime("gps") }
            SettingsListItems.SingleValue { text: i18n.tr("Wi-Fi connected"); value: root.onTime("wifi") }
            SettingsListItems.SingleValue { text: i18n.tr("Bluetooth"); value: root.onTime("bluetooth") }

            SettingsItemTitle { text: i18n.tr("Apps and services (CPU time)") }
            Repeater {
                model: u ? u.apps : []
                delegate: ListItem.Subtitled {
                    objectName: "fp6UsageApp" + index
                    text: modelData.name
                    subText: i18n.tr("%1 · %2 s CPU (%3 %) · %4 wakeups")
                        .arg(root.kindName(modelData.kind)).arg((modelData.cpuMs / 1000).toFixed(0))
                        .arg((modelData.share * 100).toFixed(1)).arg(modelData.wakeups)
                    progression: false
                    Rectangle {
                        anchors { left: parent.left; bottom: parent.bottom; leftMargin: units.gu(2); bottomMargin: units.dp(2) }
                        height: units.dp(3)
                        width: (parent.width - units.gu(4)) * modelData.share
                        color: theme.palette.normal.activity
                    }
                }
            }
            ListItem.Caption {
                text: !u || u.apps.length === 0 ? i18n.tr("Collecting data… usage appears after a few minutes.")
                    : i18n.tr("CPU time per app is measured; wakeups count how often its threads were scheduled. The screen, radios and GPS do not show up as CPU time; the measured drain above includes them.")
            }
        }
    }
}
