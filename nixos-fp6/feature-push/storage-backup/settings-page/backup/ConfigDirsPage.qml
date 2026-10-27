/*
 * Backup > App settings folders: ~/.config subfolders saved with Settings.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "fp6ConfigDirsPage"
    title: i18n.tr("App settings folders")
    flickable: list

    property var sm
    property var dirs: []
    readonly property var chosen: sm && sm.backup ? sm.backup.config.selection.configDirs : []

    Component.onCompleted: sm.call("backup.configDirs", {}, function(e, v) { if (!e) page.dirs = v })

    function toggle(name, on) {
        var l = chosen.slice()
        var i = l.indexOf(name)
        if (on && i < 0) l.push(name)
        if (!on && i >= 0) l.splice(i, 1)
        sm.call("backup.setConfig", { selection: { configDirs: l } }, function() { sm.refresh() })
    }

    ListView {
        id: list
        anchors.fill: parent
        model: page.dirs
        header: Item {
            width: list.width
            height: hdr.height + units.gu(2)
            Label {
                id: hdr
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2); top: parent.top; topMargin: units.gu(1) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Folders in ~/.config saved with Settings. Restoring replaces their files.")
            }
        }
        delegate: ListItem.Standard {
            text: modelData.name + "  ·  " + sm.size(modelData.bytes)
            control: CheckBox {
                objectName: "fp6Dir_" + modelData.name
                property bool serverChecked: page.chosen.indexOf(modelData.name) >= 0
                onServerCheckedChanged: checked = serverChecked
                Component.onCompleted: checked = serverChecked
                onTriggered: page.toggle(modelData.name, checked)
            }
        }
    }
}
