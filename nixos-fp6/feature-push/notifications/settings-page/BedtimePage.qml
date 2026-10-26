/*
 * Do Not Disturb > Bedtime mode: at a set time DND on, grey screen, dimmed
 * wallpaper. Alarms still ring.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "bedtimePage"
    property var dm
    readonly property var gs: dm.settings
    title: i18n.tr("Bedtime mode")
    flickable: flick

    Component { id: timeDialog; TimeDialog {} }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        Column {
            id: content
            anchors { left: parent.left; right: parent.right }
            spacing: units.gu(1)

            Label {
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                topPadding: units.gu(1)
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Winds the phone down for the night: Do Not Disturb, a grey screen and a darker wallpaper. Alarms still ring. \"Bedtime mode\" in the status-bar menu starts or ends it by hand.")
            }

            SettingsListItems.Standard {
                text: i18n.tr("On a schedule")
                Switch {
                    objectName: "bedtimeEnabled"
                    property bool serverChecked: gs.bedtimeEnabled
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.bedtimeEnabled = checked
                }
            }
            SettingsListItems.Standard {
                text: i18n.tr("Bedtime now")
                Switch {
                    objectName: "bedtimeNow"
                    property bool serverChecked: gs.bedtimeActive
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: page.dm.setBedtimeManual(checked)
                }
            }
            DaysRow {
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                days: gs.bedtimeDays || []
                names: page.dm.dayNames
                onToggled: gs.bedtimeDays = days
            }
            SettingsListItems.SingleValueProgression {
                objectName: "bedtimeStart"
                text: i18n.tr("Start")
                value: gs.bedtimeStart
                onClicked: PopupUtils.open(timeDialog, page, { title: i18n.tr("Start"), time: gs.bedtimeStart, onAccepted: function(t) { gs.bedtimeStart = t } })
            }
            SettingsListItems.SingleValueProgression {
                objectName: "bedtimeEnd"
                text: i18n.tr("End")
                value: gs.bedtimeEnd <= gs.bedtimeStart ? i18n.tr("%1 next day").arg(gs.bedtimeEnd) : gs.bedtimeEnd
                onClicked: PopupUtils.open(timeDialog, page, { title: i18n.tr("End"), time: gs.bedtimeEnd, onAccepted: function(t) { gs.bedtimeEnd = t } })
            }

            SettingsItemTitle { text: i18n.tr("During bedtime") }
            SettingsListItems.Standard {
                text: i18n.tr("Do Not Disturb")
                Switch {
                    objectName: "bedtimeDnd"
                    property bool serverChecked: gs.bedtimeDnd
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.bedtimeDnd = checked
                }
            }
            SettingsListItems.Standard {
                text: i18n.tr("Greyscale screen")
                Switch {
                    objectName: "bedtimeGreyscale"
                    property bool serverChecked: gs.bedtimeGreyscale
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.bedtimeGreyscale = checked
                }
            }
            SettingsListItems.Standard {
                text: i18n.tr("Dim the wallpaper")
                Switch {
                    objectName: "bedtimeDim"
                    property bool serverChecked: gs.bedtimeDimWallpaper
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.bedtimeDimWallpaper = checked
                }
            }
            Slider {
                objectName: "bedtimeDimLevel"
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                visible: gs.bedtimeDimWallpaper
                minimumValue: 0.1
                maximumValue: 0.9
                live: false
                value: gs.bedtimeDimLevel
                function formatValue(v) { return Math.round(v * 100) + " %" }
                onValueChanged: if (Math.abs(value - gs.bedtimeDimLevel) > 0.005) gs.bedtimeDimLevel = value
            }
        }
    }
}
