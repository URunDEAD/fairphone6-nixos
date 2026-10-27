/*
 * Backup > Restore: backups in the chosen folder (or another one found on
 * a memory card / USB drive), newest first.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "fp6RestorePage"
    title: i18n.tr("Restore")
    flickable: flick

    property var sm
    property var folders: []
    property var lists: ({})

    function stampText(s) {
        // YYYYmmdd-HHMMSS
        return s.length === 15 ? s.substr(6, 2) + "." + s.substr(4, 2) + "." + s.substr(0, 4) + " " + s.substr(9, 2) + ":" + s.substr(11, 2) : s
    }

    Component.onCompleted: sm.call("backup.destinations", {}, function(e, v) {
        if (e) return
        var f = [sm.backup.folder]
        for (var i = 0; i < v.length; i++) if (f.indexOf(v[i].path) < 0) f.push(v[i].path)
        page.folders = f
        for (var k = 0; k < f.length; k++) (function(folder) {
            sm.call("backup.list", { folder: folder }, function(e2, l) {
                if (e2) return
                var copy = Object.assign({}, page.lists)
                copy[folder] = l
                page.lists = copy
            })
        })(f[k])
    })

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

            Item { width: 1; height: units.gu(1) }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Choose a backup to see what it contains. You can then restore all of it or only some parts.")
            }

            Repeater {
                model: page.folders
                delegate: Column {
                    anchors { left: parent.left; right: parent.right }
                    readonly property var items: page.lists[modelData] || []
                    visible: items.length > 0 || index === 0
                    SettingsItemTitle { text: String(modelData).replace(/^\/home\/[^/]+/, "~") }
                    Label {
                        anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                        visible: parent.items.length === 0
                        fontSize: "small"
                        text: i18n.tr("No backups here.")
                    }
                    Repeater {
                        model: parent.items
                        delegate: SettingsListItems.StandardProgression {
                            objectName: "fp6Backup_" + modelData.stamp
                            text: page.stampText(modelData.stamp) + "  ·  " + sm.size(modelData.size)
                            onClicked: pageStack.addPageToNextColumn(page, Qt.resolvedUrl("PreviewPage.qml"),
                                                                     { sm: page.sm, file: modelData.file, label: page.stampText(modelData.stamp) })
                        }
                    }
                }
            }
        }
    }
}
