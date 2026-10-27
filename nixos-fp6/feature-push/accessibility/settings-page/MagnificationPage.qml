/*
 * Settings > Accessibility > Magnification (FP6).
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "magnificationPage"
    title: i18n.tr("Magnification")
    flickable: flick
    property var settings

    readonly property int taps: settings && settings.magnificationTapCount ? settings.magnificationTapCount : 3
    readonly property var tapWords: ["", "", i18n.tr("Double-tap"), i18n.tr("Triple-tap"), i18n.tr("Tap four times")]

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

            SettingsItemTitle { text: i18n.tr("Start magnification") }
            SettingSwitch {
                objectName: "magnificationTap"
                text: i18n.tr("%1 the screen").arg(page.tapWords[page.taps])
                settings: page.settings
                key: "magnificationTap"
            }
            Note {
                text: i18n.tr("Taps reach apps a moment later while this is on (they could be the start of the gesture).")
            }
            ListItem.ItemSelector {
                objectName: "tapCount"
                text: i18n.tr("Number of taps")
                model: [i18n.tr("2 taps"), i18n.tr("3 taps"), i18n.tr("4 taps")]
                selectedIndex: page.taps - 2
                onDelegateClicked: page.settings.magnificationTapCount = index + 2
            }
            SettingSwitch {
                objectName: "magnificationHold"
                text: i18n.tr("Magnify while held")
                settings: page.settings
                key: "magnificationHold"
            }
            Note {
                text: i18n.tr("Tap %1 times and keep the finger down on the last tap: the screen is magnified until you lift it; move the finger to look around.").arg(page.taps)
            }
            SettingSwitch {
                objectName: "magnificationShortcut"
                text: i18n.tr("Hold both volume keys")
                settings: page.settings
                key: "magnificationShortcut"
            }
            Note {
                text: i18n.tr("Press and hold volume up and down together for 2 seconds to start or stop magnification. The Magnification tile in the quick settings works too.")
            }

            ListItem.ThinDivider {}
            SettingsItemTitle { text: i18n.tr("Zoom") }
            SettingsListItems.Standard {
                text: i18n.tr("Zoom level")
                showDivider: false
                Label {
                    text: i18n.tr("%1×").arg(zoom.value.toFixed(1))
                    color: theme.palette.normal.backgroundSecondaryText
                }
            }
            Slider {
                id: zoom
                objectName: "zoomLevel"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                minimumValue: 1.25
                maximumValue: 8
                live: false
                function formatValue(v) { return v.toFixed(1) + "×"; }
                property real serverValue: page.settings && page.settings.magnificationLevel ? page.settings.magnificationLevel : 2
                onServerValueChanged: value = serverValue
                Component.onCompleted: value = serverValue
                onValueChanged: if (Math.abs(value - serverValue) > 0.01) page.settings.magnificationLevel = value
            }
            Item { width: 1; height: units.gu(1) }

            ListItem.ThinDivider {}
            SettingsItemTitle { text: i18n.tr("While magnified") }
            Note {
                text: i18n.tr("• Drag with two fingers to move around.\n• Pinch with two fingers to change the zoom (it is remembered).\n• One finger uses the apps as usual.\n• %1 again to stop.\n• An orange frame shows that the screen is magnified.").arg(page.tapWords[page.taps])
            }
            Item { width: 1; height: units.gu(2) }
        }
    }
}
