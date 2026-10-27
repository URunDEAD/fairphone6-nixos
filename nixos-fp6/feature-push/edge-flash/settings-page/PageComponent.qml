/*
 * System Settings > Edge Lighting (FP6): screen-edge glow for notifications
 * while the display is off (the phone has no notification LED). Settings in
 * com.fairphone6.edge-flash, read live by Lomiri (Fp6EdgeFlash.qml).
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
    objectName: "edgeLightingPage"
    title: i18n.tr("Edge Lighting")
    flickable: flick

    GSettings {
        id: gs
        schema.id: "com.fairphone6.edge-flash"
    }

    readonly property var swatches: ["#3eb4f0", "#4cd07d", "#f5a623", "#e8505b", "#a46cf0", "#2fc7c0", "#f07ac0", "#ffffff"]
    readonly property var reminderValues: [0, 5, 10, 15, 30]
    readonly property var brightnessValues: [6, 16, 32]

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

            SettingsListItems.Standard {
                text: i18n.tr("Light the screen edges for notifications")
                Switch {
                    objectName: "edgeEnabledSwitch"
                    property bool serverChecked: gs.enabled === true
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.enabled = checked
                }
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("When a notification arrives while the screen is off, the edges of the screen glow for a moment instead of the whole screen turning on. The rest of the screen stays black and locked. Calls and alarms still turn the screen on. Do Not Disturb and bedtime apply: silenced notifications do not light up. Not in a pocket, face down or during a call.")
            }

            Column {
                anchors { left: parent.left; right: parent.right }
                spacing: units.gu(1)
                visible: gs.enabled === true

                SettingsListItems.Standard {
                    text: i18n.tr("Only for calls and messages")
                    Switch {
                        objectName: "edgeOnlyMessagesSwitch"
                        property bool serverChecked: gs.onlyCallsMessages === true
                        onServerCheckedChanged: checked = serverChecked
                        Component.onCompleted: checked = serverChecked
                        onTriggered: gs.onlyCallsMessages = checked
                    }
                    showDivider: false
                }
                Label {
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    wrapMode: Text.WordWrap
                    fontSize: "small"
                    text: i18n.tr("Missed calls, text messages and chat apps light up; other notifications turn the screen on as before.")
                }

                ListItem.ThinDivider {}
                SettingsItemTitle { text: i18n.tr("Colour") }

                OptionSelector {
                    objectName: "edgeColorMode"
                    anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                    expanded: true
                    model: [i18n.tr("The app's colour"), i18n.tr("Always the same colour")]
                    selectedIndex: gs.colorMode === "fixed" ? 1 : 0
                    onDelegateClicked: gs.colorMode = index === 1 ? "fixed" : "app"
                }
                Flow {
                    objectName: "edgeSwatches"
                    visible: gs.colorMode === "fixed"
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    spacing: units.gu(1.5)
                    Repeater {
                        model: root.swatches
                        delegate: AbstractButton {
                            objectName: "edgeSwatch" + index
                            width: units.gu(4.5)
                            height: width
                            onClicked: gs.color = modelData
                            Rectangle {
                                anchors.fill: parent
                                radius: width / 2
                                color: modelData
                                border.width: String(gs.color).toLowerCase() === modelData ? units.dp(3) : units.dp(1)
                                border.color: String(gs.color).toLowerCase() === modelData ? theme.palette.normal.backgroundText : theme.palette.normal.base
                            }
                        }
                    }
                }

                ListItem.ThinDivider {}
                SettingsItemTitle { text: i18n.tr("Brightness") }
                OptionSelector {
                    objectName: "edgeBrightness"
                    anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                    model: [i18n.tr("Dim (dark rooms)"), i18n.tr("Normal"), i18n.tr("Bright")]
                    selectedIndex: {
                        var b = gs.brightness || 16;
                        return b <= 10 ? 0 : (b <= 24 ? 1 : 2);
                    }
                    onDelegateClicked: gs.brightness = root.brightnessValues[index]
                }

                ListItem.ThinDivider {}
                SettingsItemTitle { text: i18n.tr("Reminder") }
                OptionSelector {
                    objectName: "edgeReminder"
                    anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                    model: [i18n.tr("Off"), i18n.tr("Every 5 minutes"), i18n.tr("Every 10 minutes"),
                            i18n.tr("Every 15 minutes"), i18n.tr("Every 30 minutes")]
                    selectedIndex: Math.max(0, root.reminderValues.indexOf(gs.reminderMinutes))
                    onDelegateClicked: gs.reminderMinutes = root.reminderValues[index]
                }
                Label {
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    wrapMode: Text.WordWrap
                    fontSize: "small"
                    text: i18n.tr("Glow again while notifications are unseen (the screen was not turned on since), at most %1 times.").arg(gs.reminderCount || 6)
                }

                ListItem.ThinDivider {}
                SettingsListItems.Standard {
                    text: i18n.tr("Also when battery saver is on")
                    Switch {
                        objectName: "edgeInSaverSwitch"
                        property bool serverChecked: gs.inBatterySaver === true
                        onServerCheckedChanged: checked = serverChecked
                        Component.onCompleted: checked = serverChecked
                        onTriggered: gs.inBatterySaver = checked
                    }
                    showDivider: false
                }
                Label {
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    wrapMode: Text.WordWrap
                    fontSize: "small"
                    text: i18n.tr("Each glow uses about as much battery as half a second with the screen on; 100 a day is about 0.1 % of the battery.")
                }

                Button {
                    objectName: "edgePreview"
                    anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                    text: i18n.tr("Preview")
                    onClicked: gs.previewRequest = (gs.previewRequest + 1) % 1000000
                }
            }
        }
    }
}
