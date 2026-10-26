/*
 * Battery > Charge limit: stop charging at 80/85/90/95 % (qcom_battmgr
 * charge_control thresholds, applied by fp6-batteryd) and adaptive
 * charging (hold overnight, finish before the next Clock alarm).
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: root
    objectName: "fp6ChargingPage"
    title: i18n.tr("Charge limit")
    flickable: flick
    property var m: null
    readonly property var limits: [80, 85, 90, 95, 100]
    readonly property var holds: [70, 75, 80, 85, 90]

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        boundsBehavior: (contentHeight > root.height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds

        Column {
            id: content
            anchors { left: parent.left; right: parent.right }

            SettingsItemTitle { text: i18n.tr("Stop charging at:") }

            ListItem.ItemSelector {
                id: limitSelector
                objectName: "fp6LimitSelector"
                expanded: true
                highlightWhenPressed: false
                model: [ "80 %", "85 %", "90 %", "95 %", i18n.tr("100 % (no limit)") ]
                delegate: OptionSelectorDelegate { text: modelData }
                selectedIndex: Math.max(0, root.limits.indexOf(m.settings.chargeLimit))
                onDelegateClicked: m.settings.chargeLimit = root.limits[index]
            }

            ListItem.Caption {
                text: i18n.tr("Keeping the battery below 100 % slows its wear. At the limit the charger stops and the phone runs from the charger; charging resumes 5 % below the limit.")
            }

            SettingsListItems.SingleValue {
                visible: m.limit !== null && !m.limit.supported
                text: i18n.tr("Not supported by this kernel")
                value: ""
            }
            SettingsListItems.SingleValue {
                visible: m.limit !== null && m.limit.error !== ""
                text: i18n.tr("Could not apply")
                value: m.limit ? m.limit.error : ""
            }

            SettingsItemTitle { text: i18n.tr("Adaptive charging") }

            SettingsListItems.Standard {
                text: i18n.tr("Finish charging just before the alarm")
                Switch {
                    objectName: "fp6AdaptiveSwitch"
                    property bool serverChecked: m.settings.adaptiveCharging
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: m.settings.adaptiveCharging = checked
                }
                showDivider: false
            }

            ListItem.Caption {
                text: i18n.tr("When the phone is plugged in in the evening or at night and an alarm is set in Clock for 3 to 14 hours later, it holds at the level below and charges to the limit above so that it is done shortly before the alarm.")
            }

            SettingsItemTitle {
                visible: m.settings.adaptiveCharging
                text: i18n.tr("Hold at:")
            }
            ListItem.ItemSelector {
                visible: m.settings.adaptiveCharging
                objectName: "fp6HoldSelector"
                expanded: true
                highlightWhenPressed: false
                model: [ "70 %", "75 %", "80 %", "85 %", "90 %" ]
                delegate: OptionSelectorDelegate { text: modelData }
                selectedIndex: Math.max(0, root.holds.indexOf(m.settings.adaptiveHoldLevel))
                onDelegateClicked: m.settings.adaptiveHoldLevel = root.holds[index]
            }

            SettingsListItems.SingleValue {
                objectName: "fp6NextAlarm"
                text: i18n.tr("Next alarm")
                value: m.limit && m.limit.alarm > 0
                       ? Qt.formatDateTime(new Date(m.limit.alarm * 1000), "ddd hh:mm")
                       : i18n.tr("None")
            }
            SettingsListItems.SingleValue {
                objectName: "fp6ChargingState"
                text: i18n.tr("Now")
                value: {
                    if (!m.statusOk) return i18n.tr("Battery service not running")
                    if (!m.plugged) return i18n.tr("Not plugged in")
                    var l = m.limit
                    if (l.mode === "hold")
                        return i18n.tr("Holding at %1 % until %2").arg(m.charging.target).arg(m.clock(l.holdUntil))
                    if (l.mode === "topup")
                        return i18n.tr("Charging to %1 % before the alarm").arg(m.charging.target)
                    return m.charging.target < 100 ? i18n.tr("Charging to %1 %").arg(m.charging.target)
                                                    : i18n.tr("Charging normally")
                }
            }
            SettingsListItems.SingleValue {
                text: i18n.tr("Charger thresholds")
                value: m.limit && m.limit.end !== null ? i18n.tr("resume %1 %, stop %2 %").arg(m.limit.start).arg(m.limit.end) : ""
                visible: m.limit !== null && m.limit.applied !== null
            }
        }
    }
}
