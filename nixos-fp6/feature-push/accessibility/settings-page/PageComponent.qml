/*
 * System Settings > Accessibility (FP6): magnification, colour inversion,
 * colour correction, mono audio and balance. Backend: GSettings
 * com.fairphone6.accessibility, read live by the Lomiri shell (magnifier,
 * colour filter) and fp6-accessibility-audio (WirePlumber).
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems
import GSettings 1.0

ItemPage {
    id: root
    objectName: "accessibilityPage"
    title: i18n.tr("Accessibility")
    flickable: flick

    GSettings {
        id: gs
        schema.id: "com.fairphone6.accessibility"
    }

    readonly property var corrections: ["none", "deuteranomaly", "protanomaly", "tritanomaly", "greyscale"]
    readonly property var correctionLabels: [i18n.tr("Off"), i18n.tr("Red-green (deuteranomaly)"),
        i18n.tr("Red-green (protanomaly)"), i18n.tr("Blue-yellow (tritanomaly)"), i18n.tr("Greyscale")]
    readonly property bool magnificationOn: gs.magnificationTap === true || gs.magnificationShortcut === true

    function balanceText(b) {
        if (Math.abs(b) < 0.005) return i18n.tr("Centre");
        var p = Math.round(Math.abs(b) * 100);
        return b < 0 ? i18n.tr("%1 % left").arg(p) : i18n.tr("%1 % right").arg(p);
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

            SettingsItemTitle { text: i18n.tr("Vision") }

            SettingsListItems.SingleValueProgression {
                objectName: "magnification"
                text: i18n.tr("Magnification")
                value: root.magnificationOn ? i18n.tr("On") : i18n.tr("Off")
                onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("MagnificationPage.qml"), { settings: gs })
            }
            SettingSwitch {
                objectName: "colourInversion"
                text: i18n.tr("Colour inversion")
                settings: gs
                key: "colourInversion"
            }
            Note {
                text: i18n.tr("Light colours become dark and dark ones light; colours keep their hue. Also in the quick settings.")
            }
            SettingsListItems.SingleValueProgression {
                objectName: "colourCorrection"
                text: i18n.tr("Colour correction")
                value: root.correctionLabels[Math.max(0, root.corrections.indexOf(gs.colourCorrection || "none"))]
                onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("ColourCorrectionPage.qml"), { settings: gs })
            }
            SettingsListItems.StandardProgression {
                objectName: "displayComfort"
                text: i18n.tr("Text size, bold text, display size, night light")
                onClicked: Qt.openUrlExternally("settings:///system/display-comfort")
            }

            ListItem.ThinDivider {}
            SettingsItemTitle { text: i18n.tr("Hearing") }

            SettingSwitch {
                objectName: "monoAudio"
                text: i18n.tr("Mono audio")
                settings: gs
                key: "monoAudio"
            }
            Note {
                text: i18n.tr("Plays the same sound on the left and the right, on every stereo output (speaker, headphones, Bluetooth).")
            }
            SettingsListItems.Standard {
                objectName: "balanceTitle"
                text: i18n.tr("Balance")
                showDivider: false
                Label {
                    text: root.balanceText(balance.value)
                    color: theme.palette.normal.backgroundSecondaryText
                }
            }
            Item {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                height: balance.height
                Label {
                    id: leftLabel
                    anchors { left: parent.left; verticalCenter: parent.verticalCenter }
                    text: i18n.tr("L")
                }
                Slider {
                    id: balance
                    objectName: "balanceSlider"
                    anchors { left: leftLabel.right; right: rightLabel.left; leftMargin: units.gu(1); rightMargin: units.gu(1) }
                    minimumValue: -1
                    maximumValue: 1
                    live: false
                    function formatValue(v) { return Math.round(v * 100) + " %"; }
                    property real serverValue: gs.audioBalance !== undefined ? gs.audioBalance : 0
                    onServerValueChanged: value = serverValue
                    Component.onCompleted: value = serverValue
                    // live: false, so this runs on release (and on outside changes,
                    // which match serverValue and write nothing); snaps to the centre
                    onValueChanged: {
                        var v = Math.abs(value) < 0.05 ? 0 : value;
                        if (Math.abs(v - serverValue) > 0.001) gs.audioBalance = v;
                        if (v !== value) value = v;
                    }
                }
                Label {
                    id: rightLabel
                    anchors { right: parent.right; verticalCenter: parent.verticalCenter }
                    text: i18n.tr("R")
                }
            }
            Item { width: 1; height: units.gu(1) }

            ListItem.ThinDivider {}
            SettingsItemTitle { text: i18n.tr("Interaction") }
            Note {
                text: i18n.tr("Larger text and larger touch targets: Display Comfort > Text and display size (display size scales every control).")
            }
            Item { width: 1; height: units.gu(2) }
        }
    }
}
