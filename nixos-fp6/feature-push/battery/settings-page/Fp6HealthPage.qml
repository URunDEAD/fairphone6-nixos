/*
 * Battery > Battery health and history: capacity vs design, cycles,
 * temperature, voltage; charge level over 24 h / 7 days; charging sessions.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: root
    objectName: "fp6HealthPage"
    title: i18n.tr("Battery health")
    flickable: flick
    property var m: null
    property bool week: false
    readonly property var hl: m.health

    Component.onCompleted: { m.wantHistory = true; m.refresh() }
    Component.onDestruction: m.wantHistory = false

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        boundsBehavior: (contentHeight > root.height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds

        Column {
            id: content
            anchors { left: parent.left; right: parent.right }

            SettingsListItems.SingleValue {
                objectName: "fp6HealthCapacity"
                text: i18n.tr("Capacity")
                value: hl ? i18n.tr("%1 mAh of %2 mAh design (%3 %)").arg(hl.fullMah).arg(hl.designMah).arg(hl.percent) : ""
            }
            SettingsListItems.SingleValue {
                text: i18n.tr("State of health (fuel gauge)")
                value: hl ? hl.stateOfHealth + " % · " + hl.health : ""
            }
            SettingsListItems.SingleValue {
                objectName: "fp6HealthCycles"
                text: i18n.tr("Charge cycles")
                value: hl ? String(hl.cycles) : ""
            }
            SettingsListItems.SingleValue {
                text: i18n.tr("Temperature")
                value: hl ? hl.temperature.toFixed(1) + " °C" : ""
            }
            SettingsListItems.SingleValue {
                text: i18n.tr("Voltage")
                value: hl ? (m.liveVoltage > 0 ? (m.liveVoltage / 1e6).toFixed(3) : (hl.millivolts / 1000).toFixed(3)) + " V" : ""
            }
            SettingsListItems.SingleValue {
                text: i18n.tr("Current")
                value: (m.liveCurrent / 1000).toFixed(0) + " mA" + (m.liveCurrent > 0 ? i18n.tr(" (charging)") : "")
            }
            SettingsListItems.SingleValue {
                text: i18n.tr("Internal resistance")
                value: hl && hl.resistanceMilliohm > 0 ? hl.resistanceMilliohm + " mΩ" : ""
                visible: value !== ""
            }
            SettingsListItems.SingleValue {
                text: i18n.tr("Battery")
                value: hl ? hl.technology : ""
            }

            SettingsItemTitle { text: i18n.tr("Charge level") }

            Row {
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: units.gu(1)
                Button {
                    objectName: "fp6HistoryDay"
                    text: i18n.tr("24 hours")
                    color: !root.week ? theme.palette.normal.positive : theme.palette.normal.base
                    onClicked: root.week = false
                }
                Button {
                    objectName: "fp6HistoryWeek"
                    text: i18n.tr("7 days")
                    color: root.week ? theme.palette.normal.positive : theme.palette.normal.base
                    onClicked: root.week = true
                }
            }

            Fp6LevelGraph {
                objectName: "fp6LevelGraph"
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                span: root.week ? 7 * 86400 : 86400
                now: m.history ? m.history.time : Date.now() / 1000
                points: m.history ? (root.week ? m.history.week : m.history.day) : []
                onSpanChanged: requestPaint()
            }

            SettingsItemTitle { text: i18n.tr("Charging sessions") }

            Repeater {
                model: m.history ? m.history.sessions : []
                delegate: SettingsListItems.SingleValue {
                    readonly property var s: modelData.session ? modelData.session : modelData
                    text: Qt.formatDateTime(new Date(s.start * 1000), "ddd d MMM hh:mm")
                          + (modelData.ongoing ? i18n.tr(" (now)") : "")
                    value: i18n.tr("%1 → %2 %, %3, max %4%5")
                        .arg(s.startLevel).arg(s.endLevel).arg(m.duration(s.end - s.start))
                        .arg(m.watts(s.maxMilliwatts / 1000))
                        .arg(s.limit < 100 ? i18n.tr(", limit %1 %").arg(s.limit) : "")
                }
            }
            ListItem.Caption {
                visible: !m.history || m.history.sessions.length === 0
                text: i18n.tr("No charging sessions recorded yet.")
            }
        }
    }
}
