/*
 * Settings > Accessibility > Colour correction (FP6). The filter is applied
 * to the whole screen at once, so the colour strip below shows the effect.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "colourCorrectionPage"
    title: i18n.tr("Colour correction")
    flickable: flick
    property var settings

    readonly property var modes: ["none", "deuteranomaly", "protanomaly", "tritanomaly", "greyscale"]
    readonly property var labels: [i18n.tr("Off"), i18n.tr("Deuteranomaly (red-green)"),
        i18n.tr("Protanomaly (red-green)"), i18n.tr("Tritanomaly (blue-yellow)"), i18n.tr("Greyscale")]
    readonly property string mode: settings && settings.colourCorrection ? settings.colourCorrection : "none"

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

            // colour strip (seen through the live filter)
            Item {
                anchors { left: parent.left; right: parent.right }
                height: units.gu(12)
                Row {
                    id: strip
                    anchors.centerIn: parent
                    readonly property var colours: ["#E53935", "#FB8C00", "#FDD835", "#7CB342", "#2E7D32",
                                                    "#00ACC1", "#1E88E5", "#3949AB", "#8E24AA", "#D81B60"]
                    Repeater {
                        model: strip.colours
                        Rectangle {
                            width: (content.width - units.gu(4)) / strip.colours.length
                            height: units.gu(8)
                            color: modelData
                        }
                    }
                }
            }

            SettingsItemTitle { text: i18n.tr("Correction") }
            Repeater {
                model: page.modes.length
                SettingsListItems.Standard {
                    objectName: "mode_" + page.modes[index]
                    text: page.labels[index]
                    onClicked: page.settings.colourCorrection = page.modes[index]
                    Icon {
                        width: units.gu(2.5); height: width
                        name: "tick"
                        visible: page.mode === page.modes[index]
                        color: theme.palette.normal.positive
                    }
                }
            }

            ListItem.ThinDivider {}
            SettingsListItems.Standard {
                text: i18n.tr("Intensity")
                showDivider: false
                enabled: page.mode !== "none"
                Label {
                    text: Math.round(strength.value * 100) + " %"
                    color: theme.palette.normal.backgroundSecondaryText
                }
            }
            Slider {
                id: strength
                objectName: "strength"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                enabled: page.mode !== "none"
                minimumValue: 0
                maximumValue: 1
                live: false
                function formatValue(v) { return Math.round(v * 100) + " %"; }
                property real serverValue: page.settings && page.settings.colourCorrectionStrength !== undefined ? page.settings.colourCorrectionStrength : 1
                onServerValueChanged: value = serverValue
                Component.onCompleted: value = serverValue
                onValueChanged: if (Math.abs(value - serverValue) > 0.005) page.settings.colourCorrectionStrength = value
            }
            Note {
                text: i18n.tr("Shifts colours that are hard to tell apart with the chosen colour vision deficiency towards ones that are easier to see (the method Android uses). Works together with colour inversion and night light.")
            }
            Item { width: 1; height: units.gu(2) }
        }
    }
}
