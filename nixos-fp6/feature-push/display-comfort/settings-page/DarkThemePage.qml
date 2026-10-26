/*
 * Display Comfort > Dark theme schedule.
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
    objectName: "darkThemePage"
    title: i18n.tr("Dark Theme")
    flickable: flick

    GSettings {
        id: gs
        schema.id: "com.fairphone6.display-comfort"
    }
    readonly property var modes: ["off", "on", "follow-night-light", "sunset-sunrise", "custom"]
    readonly property var labels: [i18n.tr("Off"), i18n.tr("Always"), i18n.tr("With night light"),
                                   i18n.tr("Sunset to sunrise"), i18n.tr("Custom")]

    Component { id: timeDialog; TimeDialog {} }

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

            SettingsItemTitle { text: i18n.tr("Use the dark theme") }
            OptionSelector {
                objectName: "darkModeSelector"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                model: page.labels
                selectedIndex: Math.max(0, page.modes.indexOf(gs.darkThemeMode))
                onDelegateClicked: gs.darkThemeMode = page.modes[index]
            }
            Note {
                text: {
                    switch (gs.darkThemeMode) {
                    case "off": return i18n.tr("No schedule: the Dark mode switch in Background & Appearance stays as you set it.")
                    case "follow-night-light": return i18n.tr("Dark while the night light schedule is active (also when night light itself is off).")
                    case "sunset-sunrise": return i18n.tr("Uses the location settings of the night light.")
                    default: return ""
                    }
                }
            }
            Column {
                visible: gs.darkThemeMode === "custom"
                anchors.left: parent.left
                anchors.right: parent.right
                SettingsListItems.SingleValueProgression {
                    objectName: "darkFrom"
                    text: i18n.tr("From")
                    value: gs.darkThemeFrom
                    onClicked: PopupUtils.open(timeDialog, page, { title: i18n.tr("From"), time: gs.darkThemeFrom,
                                                                   onAccepted: function(t) { gs.darkThemeFrom = t } })
                }
                SettingsListItems.SingleValueProgression {
                    objectName: "darkTo"
                    text: i18n.tr("To")
                    value: gs.darkThemeTo <= gs.darkThemeFrom ? i18n.tr("%1 next day").arg(gs.darkThemeTo) : gs.darkThemeTo
                    onClicked: PopupUtils.open(timeDialog, page, { title: i18n.tr("To"), time: gs.darkThemeTo,
                                                                   onAccepted: function(t) { gs.darkThemeTo = t } })
                }
            }

            ListItem.ThinDivider {}
            SettingSwitch {
                objectName: "darkShell"
                text: i18n.tr("Also switch the shell")
                settings: gs
                key: "darkThemeShell"
            }
            Note {
                text: i18n.tr("Light panel, launcher and menus during the day; dark at night. Off: the shell keeps its own setting (dark by default).")
            }
            ListItem.ThinDivider {}
            Note {
                text: i18n.tr("Lomiri apps switch right away. GTK apps (Firefox, Fragments) follow when they are next opened; Qt 6 apps keep their own theme.")
            }
        }
    }
}
