/*
 * Battery > Charging feedback (nixos-fp6/feature-push/charging-feedback):
 * what the phone does when a charger is connected. GSettings
 * com.fairphone6.charging-feedback, read live by Lomiri.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import GSettings 1.0
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: root
    objectName: "fp6ChargingFeedbackPage"
    title: i18n.tr("Charging feedback")
    flickable: flick
    property var m: null

    GSettings {
        id: cf
        schema.id: "com.fairphone6.charging-feedback"
    }

    component Toggle: SettingsListItems.Standard {
        id: row
        property string key
        property alias checked: sw.checked
        property bool serverChecked: cf[key] !== false
        enabled: cf.schema.isValid
        onServerCheckedChanged: sw.checked = serverChecked
        Switch {
            id: sw
            objectName: "fp6Cf_" + row.key
            SlotsLayout.position: SlotsLayout.Trailing
            Component.onCompleted: checked = row.serverChecked
            onTriggered: cf[row.key] = checked
        }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        boundsBehavior: (contentHeight > root.height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds

        Column {
            id: content
            anchors { left: parent.left; right: parent.right }

            SettingsItemTitle { text: i18n.tr("When a charger is connected") }

            Toggle {
                id: animationRow
                key: "animation"
                text: i18n.tr("Charging animation")
            }
            ListItem.Caption {
                text: i18n.tr("Shows the battery level, the charging speed and the time to full for a moment: full screen on the lock screen, a small banner while you use the phone.")
            }
            Toggle {
                key: "wakeScreen"
                text: i18n.tr("Turn the screen on to show it")
                enabled: cf.schema.isValid && animationRow.checked
            }
            ListItem.Caption {
                text: i18n.tr("The screen goes off again after a few seconds if you don't touch it. Not in a pocket, during a call or while Do Not Disturb keeps the screen dark.")
            }
            Toggle {
                key: "sound"
                text: i18n.tr("Charging sound")
            }
            Toggle {
                key: "vibrate"
                text: i18n.tr("Vibrate on plug")
            }
            ListItem.Caption {
                text: i18n.tr("Also when the charger is disconnected. Silent mode turns the sound off; Do Not Disturb turns off both.")
            }

            SettingsItemTitle { text: i18n.tr("Lock screen") }

            Toggle {
                key: "lockscreenInfo"
                text: i18n.tr("Show charging info on lock screen")
            }
            ListItem.Caption {
                text: i18n.tr("While charging, a line under the clock shows the charging speed, the level and the time to full, or why charging is paused (charge limit, adaptive charging).")
            }

            ListItem.Empty {
                height: units.gu(8)
                highlightWhenPressed: false
                Button {
                    objectName: "fp6CfPreview"
                    anchors.centerIn: parent
                    width: Math.min(parent.width - units.gu(4), units.gu(30))
                    text: i18n.tr("Preview")
                    enabled: cf.schema.isValid
                    onClicked: cf.previewRequest = (cf.previewRequest || 0) + 1
                }
            }
        }
    }
}
