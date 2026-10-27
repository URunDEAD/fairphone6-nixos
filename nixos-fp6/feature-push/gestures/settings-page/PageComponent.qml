/*
 * System Settings > Gesture Shortcuts (FP6): shake, power-key
 * double/triple press, pocket mode, flip to silence, raise to wake, back tap
 * and volume-key track skipping. Backend: fp6-gestures (user service), GSettings
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

            // App switcher close all (nixos-fp6/feature-push/spread-close-all):
            // com.fairphone6.app-switcher, read live by Lomiri. Hidden when that
            // module (and its schema) is not installed.
            Column {
                objectName: "appSwitcherSection"
                visible: appSwitcher.schema.isValid
                anchors { left: parent.left; right: parent.right }
                spacing: units.gu(1)

                GSettings {
                    id: appSwitcher
                    schema.id: "com.fairphone6.app-switcher"
                }

                SettingsItemTitle { text: i18n.tr("App switcher") }

                SettingsListItems.Standard {
                    text: i18n.tr("Swipe right at the first app to close all apps")
                    Switch {
                        objectName: "closeAllGestureSwitch"
                        property bool serverChecked: appSwitcher.closeAllGesture !== false
                        onServerCheckedChanged: checked = serverChecked
                        Component.onCompleted: checked = serverChecked
                        onTriggered: appSwitcher.closeAllGesture = checked
                    }
                    showDivider: false
                }
                Label {
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    wrapMode: Text.WordWrap
                    fontSize: "small"
                    text: i18n.tr("In the app switcher, with the list at its first app, drag from left to right and keep going: \"Release to close all apps\" appears with a short vibration; let go to close them, or go back to cancel. A drag that starts further along the list only scrolls. The Clear all button at the bottom does the same.")
                }

                SettingsListItems.Standard {
                    text: i18n.tr("Keep 'ready' apps when closing all")
                    Switch {
                        objectName: "closeAllKeepReadySwitch"
                        property bool serverChecked: appSwitcher.closeAllKeepReady !== false
                        onServerCheckedChanged: checked = serverChecked
                        Component.onCompleted: checked = serverChecked
                        onTriggered: appSwitcher.closeAllKeepReady = checked
                    }
                    showDivider: false
                }
                Label {
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    wrapMode: Text.WordWrap
                    fontSize: "small"
                    text: appSwitcher.closeAllKeepReady !== false
                        ? i18n.tr("Apps you keep ready or start at login (App Launch) stay open, like pinned apps. The dialer is never closed during a call.")
                        : i18n.tr("Close all also closes the apps you keep ready (App Launch); they start again normally next time. The dialer is never closed during a call.")
                }

                ListItem.ThinDivider {}
            }

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
                text: i18n.tr("Works with the screen off too. The flashlight toggles without unlocking; an app opens after you unlock; custom commands only run while unlocked. While a double or triple press is assigned, a single press turns the screen off or on about a third of a second later. The emergency (SOS) five presses still work; with a triple press assigned they also run it once.")
            }

            ListItem.ThinDivider {}

            SettingsItemTitle { text: i18n.tr("Pocket and table") }

            SettingsListItems.Standard {
                text: i18n.tr("Pocket mode")
                Switch {
                    objectName: "pocketModeSwitch"
                    property bool serverChecked: gs.pocketMode
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.pocketMode = checked
                }
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("While the proximity sensor is covered and it is dark (in a pocket or bag) for more than a second, the power key, double tap and notifications do not turn the screen on, and touches are ignored. Incoming calls still turn it on. Press the power key twice in a row to turn the screen on anyway.")
            }
            SettingsListItems.Standard {
                text: i18n.tr("Flip to silence")
                Switch {
                    objectName: "flipSwitch"
                    property bool serverChecked: gs.flipToSilence
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.flipToSilence = checked
                }
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Put the phone face down on a table to turn silent mode on; pick it up to turn it off again. Silent mode you set yourself is left alone.")
            }
            SettingsListItems.Standard {
                text: i18n.tr("Raise to wake")
                Switch {
                    objectName: "raiseSwitch"
                    property bool serverChecked: gs.raiseToWake
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.raiseToWake = checked
                }
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("While the screen is off, picking the phone up from a table (or from lying still) and looking at it shows the lock screen. Not in a pocket and not while walking. The screen turns off again after 10 seconds if you do not touch it.")
            }

            ListItem.ThinDivider {}

            SettingsItemTitle { text: i18n.tr("Back tap (experimental)") }

            ActionSettings {
                title: i18n.tr("Double tap on the back")
                settings: gs
                prefix: "backDoubleTap"
                apps: root.apps
            }

            ActionSettings {
                title: i18n.tr("Triple tap on the back")
                settings: gs
                prefix: "backTripleTap"
                apps: root.apps
            }

            Column {
                visible: gs.backDoubleTapAction !== "none" || gs.backTripleTapAction !== "none"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                spacing: units.gu(1)
                Label { text: i18n.tr("Back tap sensitivity"); font.weight: Font.Medium }
                OptionSelector {
                    objectName: "backTapSensitivitySelector"
                    width: parent.width
                    readonly property var values: ["low", "medium", "high"]
                    model: [i18n.tr("Low (firm taps)"), i18n.tr("Medium"), i18n.tr("High (light taps)")]
                    selectedIndex: Math.max(0, values.indexOf(gs.backTapSensitivity))
                    onDelegateClicked: gs.backTapSensitivity = values[index]
                }
            }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Tap the back of the phone firmly with a fingertip, two or three times in quick succession, while holding it still. Taps on the screen are ignored. The accelerometer only wakes up when the phone moves. Same rules as the power key: the flashlight works locked, apps open after unlocking.")
            }

            ListItem.ThinDivider {}

            SettingsItemTitle { text: i18n.tr("Volume keys") }

            SettingsListItems.Standard {
                text: i18n.tr("Long press for next / previous track")
                Switch {
                    objectName: "volumeTracksSwitch"
                    property bool serverChecked: gs.volumeLongPressTracks
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.volumeLongPressTracks = checked
                }
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("With the screen off, hold Volume Up for the next track or Volume Down for the previous one (about two thirds of a second). The volume stays as it was. Short presses still change the volume.")
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
                text: i18n.tr("About custom commands: a command runs with your user's full rights (your files, network, messages), not in a sandbox. Anyone who can shake or tap the phone or press the power key while it is unlocked can run it. Only enter commands you understand and trust.")
            }
        }
    }
}
