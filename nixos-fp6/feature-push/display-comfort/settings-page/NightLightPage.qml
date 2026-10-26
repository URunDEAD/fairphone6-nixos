/*
 * Display Comfort > Night light: strength, schedule, location, transition.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems
import GSettings 1.0

ItemPage {
    id: page
    objectName: "nightLightPage"
    title: i18n.tr("Night Light")
    flickable: flick

    GSettings {
        id: gs
        schema.id: "com.fairphone6.display-comfort"
    }

    readonly property var schedules: ["sunset-sunrise", "custom", "always"]
    readonly property bool located: gs.useLocation && gs.locatedTime > 0

    Component { id: timeDialog; TimeDialog {} }
    Component { id: coordDialog; CoordinatesDialog {} }

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

            SettingSwitch {
                objectName: "nightLightEnabled"
                text: i18n.tr("Night light")
                settings: gs
                key: "nightLightEnabled"
            }
            Note { text: gs.nightLightEnabled ? gs.status : "" }

            SettingsItemTitle { text: i18n.tr("Colour temperature") }
            // Left = warmer (stronger filter).
            Slider {
                id: tempSlider
                objectName: "temperatureSlider"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                minimumValue: 1800
                maximumValue: 6000
                stepSize: 100
                live: true
                function formatValue(v) { return i18n.tr("%1 K").arg(Math.round(v)) }
                property int serverValue: gs.nightLightTemperature
                onServerValueChanged: if (!pressed) value = serverValue
                Component.onCompleted: value = serverValue
                onPressedChanged: if (!pressed) gs.nightLightTemperature = Math.round(value / 100) * 100
            }
            Row {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                Label { width: parent.width / 2; text: i18n.tr("Warmer"); fontSize: "small" }
                Label { width: parent.width / 2; text: i18n.tr("Cooler"); fontSize: "small"; horizontalAlignment: Text.AlignRight }
            }
            // Preview swatch of the chosen white point.
            Rectangle {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                height: units.gu(3)
                radius: units.gu(0.5)
                border.color: theme.palette.normal.base
                color: {
                    // Same fit as the shell filter (normalised to 6500 K).
                    function ch(k) {
                        var t = k / 100
                        var r = t <= 66 ? 255 : 329.698727446 * Math.pow(t - 60, -0.1332047592)
                        var g = t <= 66 ? 99.4708025861 * Math.log(t) - 161.1195681661 : 288.1221695283 * Math.pow(t - 60, -0.0755148492)
                        var b = t >= 66 ? 255 : (t <= 19 ? 0 : 138.5177312231 * Math.log(t - 10) - 305.0447927307)
                        return [r, g, b]
                    }
                    var w = ch(6500), c = ch(tempSlider.value)
                    return Qt.rgba(Math.min(1, c[0] / w[0]), Math.min(1, c[1] / w[1]), Math.min(1, c[2] / w[2]), 1)
                }
                Label {
                    anchors.centerIn: parent
                    text: i18n.tr("%1 K").arg(Math.round(tempSlider.value))
                    color: "black"
                    fontSize: "small"
                }
            }

            ListItem.ThinDivider {}
            SettingsItemTitle { text: i18n.tr("Schedule") }
            OptionSelector {
                objectName: "scheduleSelector"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                model: [i18n.tr("Sunset to sunrise"), i18n.tr("Custom hours"), i18n.tr("Always on")]
                selectedIndex: Math.max(0, page.schedules.indexOf(gs.nightLightSchedule))
                onDelegateClicked: gs.nightLightSchedule = page.schedules[index]
            }
            Column {
                visible: gs.nightLightSchedule === "custom"
                anchors.left: parent.left
                anchors.right: parent.right
                SettingsListItems.SingleValueProgression {
                    objectName: "fromTime"
                    text: i18n.tr("From")
                    value: gs.nightLightFrom
                    onClicked: PopupUtils.open(timeDialog, page, { title: i18n.tr("From"), time: gs.nightLightFrom,
                                                                   onAccepted: function(t) { gs.nightLightFrom = t } })
                }
                SettingsListItems.SingleValueProgression {
                    objectName: "toTime"
                    text: i18n.tr("To")
                    value: gs.nightLightTo <= gs.nightLightFrom ? i18n.tr("%1 next day").arg(gs.nightLightTo) : gs.nightLightTo
                    onClicked: PopupUtils.open(timeDialog, page, { title: i18n.tr("To"), time: gs.nightLightTo,
                                                                   onAccepted: function(t) { gs.nightLightTo = t } })
                }
            }
            Column {
                visible: gs.nightLightSchedule === "sunset-sunrise"
                anchors.left: parent.left
                anchors.right: parent.right
                spacing: units.gu(1)
                SettingSwitch {
                    objectName: "useLocation"
                    text: i18n.tr("Use my location")
                    settings: gs
                    key: "useLocation"
                }
                Note {
                    text: page.located
                        ? i18n.tr("Last location: %1, %2 (rounded to about 10 km).").arg(gs.locatedLatitude.toFixed(1)).arg(gs.locatedLongitude.toFixed(1))
                        : (gs.useLocation
                           ? i18n.tr("Uses the phone's location when Location is on (checked every few hours); until then the coordinates below.")
                           : i18n.tr("Uses the coordinates below."))
                }
                SettingsListItems.SingleValueProgression {
                    objectName: "fixedCoordinates"
                    text: i18n.tr("Fixed coordinates")
                    value: gs.latitude.toFixed(2) + ", " + gs.longitude.toFixed(2)
                    onClicked: PopupUtils.open(coordDialog, page, { gs: gs })
                }
            }

            ListItem.ThinDivider {}
            SettingsItemTitle { text: i18n.tr("Gradual change") }
            Slider {
                objectName: "transitionSlider"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                minimumValue: 0
                maximumValue: 60
                stepSize: 5
                live: true
                function formatValue(v) { return Math.round(v) === 0 ? i18n.tr("Instant") : i18n.tr("%1 min").arg(Math.round(v)) }
                property int serverValue: gs.transitionMinutes
                onServerValueChanged: if (!pressed) value = serverValue
                Component.onCompleted: value = serverValue
                onPressedChanged: if (!pressed) gs.transitionMinutes = Math.round(value / 5) * 5
            }
            Note {
                text: i18n.tr("Night light fades in after the start and out before the end of the schedule. Screenshots keep the true colours; screen recordings show the filter.")
            }
        }
    }
}
