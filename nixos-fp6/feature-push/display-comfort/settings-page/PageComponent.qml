/*
 * System Settings > Display Comfort (FP6): night light, greyscale, dark
 * theme schedule, text and display size. Backend: GSettings
 * com.fairphone6.display-comfort, fp6-display-comfort (user service) and
 * Lomiri's colour filter.
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
    objectName: "displayComfortPage"
    title: i18n.tr("Display Comfort")
    flickable: flick

    GSettings {
        id: gs
        schema.id: "com.fairphone6.display-comfort"
    }

    readonly property var darkModes: ["off", "on", "follow-night-light", "sunset-sunrise", "custom"]
    readonly property var darkModeLabels: [i18n.tr("Off"), i18n.tr("Always"), i18n.tr("With night light"),
                                           i18n.tr("Sunset to sunrise"), i18n.tr("Custom")]

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

            SettingsItemTitle { text: i18n.tr("Colours") }

            SettingSwitch {
                objectName: "nightLight"
                text: i18n.tr("Night light")
                settings: gs
                key: "nightLightEnabled"
            }
            Note { text: gs.nightLightEnabled ? gs.status : "" }
            SettingsListItems.StandardProgression {
                objectName: "nightLightOptions"
                text: i18n.tr("Night light options")
                onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("NightLightPage.qml"))
            }
            SettingSwitch {
                objectName: "greyscale"
                text: i18n.tr("Greyscale")
                settings: gs
                key: "greyscaleActive"
            }
            Note {
                text: i18n.tr("Shows the whole screen without colour. Bedtime mode may switch this too.")
            }

            ListItem.ThinDivider {}
            SettingsItemTitle { text: i18n.tr("Theme") }

            SettingsListItems.SingleValueProgression {
                objectName: "darkTheme"
                text: i18n.tr("Dark theme schedule")
                value: root.darkModeLabels[Math.max(0, root.darkModes.indexOf(gs.darkThemeMode))]
                onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("DarkThemePage.qml"))
            }

            ListItem.ThinDivider {}
            SettingsItemTitle { text: i18n.tr("Size") }

            SettingsListItems.StandardProgression {
                objectName: "textAndDisplaySize"
                text: i18n.tr("Text and display size")
                onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("ScalePage.qml"))
            }
            Note {
                text: i18n.tr("Display %1 %, text %2 %").arg(Math.round(gs.interfaceScale * 100 / 24)).arg(gs.fontScale)
            }
        }
    }
}
