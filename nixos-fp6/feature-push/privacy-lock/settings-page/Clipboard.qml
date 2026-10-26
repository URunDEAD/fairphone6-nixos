/*
 * Security & Privacy > Clipboard (FP6 privacy-lock): clear the clipboard a
 * while after the last copy, and/or tell when an app pastes it.
 * Backend: fp6-privacy-clipboard (Wayland ext-data-control, which qtmir
 * offers to that one helper only).
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.4
import SystemSettings 1.0
import SystemSettings.ListItems 1.0 as SettingsListItems
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItems

ItemPage {
    id: root
    objectName: "fp6ClipboardPage"
    title: i18n.tr("Clipboard")

    property var settings
    readonly property var values: [0, 1, 2, 5, 10, 30, 60]

    Flickable {
        anchors.fill: parent
        contentHeight: column.height
        Column {
            id: column
            anchors { left: parent.left; right: parent.right }

            SettingsItemTitle { text: i18n.tr("Clear the clipboard") }
            ListItems.ItemSelector {
                objectName: "fp6ClipboardClear"
                expanded: true
                model: root.values.map(function (v) {
                    return v === 0 ? i18n.tr("Never") : i18n.tr("%1 minute after copying", "%1 minutes after copying", v).arg(v);
                })
                selectedIndex: Math.max(0, root.values.indexOf(root.settings ? (root.settings.clipboardClearMinutes || 0) : 0))
                onDelegateClicked: root.settings.clipboardClearMinutes = root.values[index]
            }

            SettingsListItems.Standard {
                text: i18n.tr("Notify when an app pastes")
                Switch {
                    objectName: "fp6ClipboardNotify"
                    property bool serverChecked: root.settings && root.settings.clipboardReadNotify === true
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: root.settings.clipboardReadNotify = checked
                }
            }
            ListItems.Caption {
                text: i18n.tr("Apps under Lomiri can only read the clipboard when they paste. With notifications on, the phone keeps the copied text itself and names the app that pastes it (text only; pictures and files are not watched).")
            }
        }
    }
}
