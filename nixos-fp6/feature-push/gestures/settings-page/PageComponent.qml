/*
 * System Settings > Gesture Shortcuts (FP6): shake and power-key
 * double/triple press. Backend: fp6-gestures (user service), GSettings
 * com.fairphone6.gestures.
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
    objectName: "gestureShortcutsPage"
    title: i18n.tr("Gesture Shortcuts")
    flickable: flick

    GSettings {
        id: gs
        schema.id: "com.fairphone6.gestures"
    }

    // "id TAB name TAB icon" entries kept by fp6-gestures.
    readonly property var apps: {
        var list = gs.installedApps || []
        var out = []
        for (var i = 0; i < list.length; i++) {
            var p = String(list[i]).split("\t")
            if (p.length >= 2) out.push({ id: p[0], name: p[1], icon: p.length > 2 ? p[2] : "" })
        }
        return out
    }

    // Ask the daemon for a fresh app list whenever the page opens.
    Component.onCompleted: gs.appListRequest = (gs.appListRequest + 1) % 1000000

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
            spacing: units.gu(1)

            SettingsItemTitle { text: i18n.tr("Shake") }

            SettingsListItems.Standard {
                text: i18n.tr("Shake to run a shortcut")
                Switch {
                    objectName: "shakeEnabledSwitch"
                    property bool serverChecked: gs.shakeEnabled
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.shakeEnabled = checked
                }
                showDivider: false
            }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Shake the phone firmly back and forth two or three times. The accelerometer is only read while this is on.")
            }

            ActionSettings {
                visible: gs.shakeEnabled
                settings: gs
                prefix: "shake"
                allowNone: false
                apps: root.apps
            }

            Column {
                visible: gs.shakeEnabled
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                spacing: units.gu(1)
                Label { text: i18n.tr("Sensitivity"); font.weight: Font.Medium }
                OptionSelector {
                    objectName: "sensitivitySelector"
                    width: parent.width
                    readonly property var values: ["low", "medium", "high"]
                    model: [i18n.tr("Low (harder shake)"), i18n.tr("Medium"), i18n.tr("High (lighter shake)")]
                    selectedIndex: Math.max(0, values.indexOf(gs.shakeSensitivity))
                    onDelegateClicked: gs.shakeSensitivity = values[index]
                }
            }

            SettingsListItems.Standard {
                visible: gs.shakeEnabled
                text: i18n.tr("Also when the screen is off or locked")
                Switch {
                    objectName: "shakeLockedSwitch"
                    property bool serverChecked: gs.shakeWhenLocked
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.shakeWhenLocked = checked
                }
                showDivider: false
            }

            Label {
                visible: gs.shakeEnabled
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: gs.shakeWhenLocked
                    ? i18n.tr("Locked: the flashlight toggles right away; an app opens after you unlock; custom commands never run.")
                    : i18n.tr("Only while the phone is unlocked and the screen is on.")
            }

            ListItem.ThinDivider {}

            SettingsItemTitle { text: i18n.tr("Power key") }

            ActionSettings {
                title: i18n.tr("Double press")
                settings: gs
                prefix: "doublePress"
                apps: root.apps
            }

            ActionSettings {
                title: i18n.tr("Triple press")
                settings: gs
                prefix: "triplePress"
                apps: root.apps
            }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Works with the screen off too. The flashlight toggles without unlocking; an app opens after you unlock; custom commands only run while unlocked. While a double or triple press is assigned, a single press turns the screen off or on about a third of a second later.")
            }

            ListItem.ThinDivider {}

            SettingsListItems.Standard {
                text: i18n.tr("Vibrate when a shortcut runs")
                Switch {
                    objectName: "vibrateSwitch"
                    property bool serverChecked: gs.vibrate
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.vibrate = checked
                }
                showDivider: false
            }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("About custom commands: a command runs with your user's full rights (your files, network, messages), not in a sandbox. Anyone who can shake the phone or press the power key while it is unlocked can run it. Only enter commands you understand and trust.")
            }
        }
    }
}
