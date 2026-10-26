/*
 * A list item with a switch bound to one boolean GSettings key.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import Lomiri.Components 1.3
import SystemSettings.ListItems 1.0 as SettingsListItems

SettingsListItems.Standard {
    id: item
    property var settings
    property string key
    property alias checked: sw.checked
    showDivider: false
    Switch {
        id: sw
        objectName: item.objectName + "Switch"
        property bool serverChecked: item.settings ? item.settings[item.key] : false
        onServerCheckedChanged: checked = serverChecked
        Component.onCompleted: checked = serverChecked
        onTriggered: item.settings[item.key] = checked
    }
}
