/*
 * Battery > Battery saver: switch, auto-on level and what it does (CPU max
 * frequency, brightness cap, Waydroid, background apps, sync, extras).
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: root
    objectName: "fp6SaverPage"
    title: i18n.tr("Battery saver")
    flickable: flick
    property var m: null
    readonly property var autoLevels: [0, 5, 10, 15, 20, 25, 30]
    readonly property var caps: [100, 80, 60, 40]

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        boundsBehavior: (contentHeight > root.height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds

        Column {
            id: content
            anchors { left: parent.left; right: parent.right }

            SettingsListItems.Standard {
                text: i18n.tr("Battery saver")
                Switch {
                    id: saverSwitch
                    objectName: "fp6SaverSwitch"
                    property bool serverChecked: m.saver ? m.saver.active : m.settings.saverEnabled
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: {
                        if (!checked && m.saver && m.saver.auto)
                            m.settings.saverDismiss = (m.settings.saverDismiss + 1) % 1000000
                        m.settings.saverEnabled = checked
                    }
                }
            }
            ListItem.Caption {
                visible: m.saver !== null && m.saver.auto
                text: i18n.tr("Turned on because the battery is low. It turns off when you plug in the phone.")
            }

            SettingsItemTitle { text: i18n.tr("Turn on automatically:") }
            ListItem.ItemSelector {
                objectName: "fp6SaverAutoSelector"
                expanded: true
                highlightWhenPressed: false
                model: [ i18n.tr("Never"), "5 %", "10 %", "15 %", "20 %", "25 %", "30 %" ]
                delegate: OptionSelectorDelegate { text: modelData }
                selectedIndex: Math.max(0, root.autoLevels.indexOf(m.settings.saverAutoLevel))
                onDelegateClicked: m.settings.saverAutoLevel = root.autoLevels[index]
            }

            SettingsItemTitle { text: i18n.tr("While battery saver is on:") }

            SettingsListItems.Standard {
                text: i18n.tr("Limit CPU speed to %1 %").arg(m.settings.saverCpuPercent)
                Switch {
                    objectName: "fp6SaverCpu"
                    property bool serverChecked: m.settings.saverCpuLimit
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: m.settings.saverCpuLimit = checked
                }
            }
            SettingsListItems.Standard {
                text: i18n.tr("Stop Android apps (Waydroid)")
                Switch {
                    objectName: "fp6SaverWaydroid"
                    property bool serverChecked: m.settings.saverStopWaydroid
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: m.settings.saverStopWaydroid = checked
                }
            }

            SettingsItemTitle { text: i18n.tr("Maximum brightness:") }
            ListItem.ItemSelector {
                objectName: "fp6SaverBrightness"
                expanded: true
                highlightWhenPressed: false
                model: [ i18n.tr("No limit"), "80 %", "60 %", "40 %" ]
                delegate: OptionSelectorDelegate { text: modelData }
                selectedIndex: Math.max(0, root.caps.indexOf(m.settings.saverBrightnessCap))
                onDelegateClicked: m.settings.saverBrightnessCap = root.caps[index]
            }

            ListItem.Caption {
                text: i18n.tr("Also: apps in the background are suspended, Android apps and the GPS assistance download wait, and gesture sensors (shake) are off.")
            }

            SettingsItemTitle {
                visible: m.saver !== null && m.saver.active && m.saver.effects.length > 0
                text: i18n.tr("Applied:")
            }
            Repeater {
                model: m.saver && m.saver.active ? m.saver.effects : []
                delegate: ListItem.Caption { text: modelData }
            }
        }
    }
}
