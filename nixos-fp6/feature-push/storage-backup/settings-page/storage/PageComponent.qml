/*
 * System Settings > Storage (FP6): usage by category, app caches, old
 * system versions, unused system files, system logs. Replaces the stock
 * About > Storage page (Click/Libertine/Anbox categories) on this port.
 * Backend: fp6-storage-agent (session) + fp6-storaged (root, polkit).
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems
import Fp6.StorageBackup 1.0

ItemPage {
    id: root
    objectName: "fp6StoragePage"
    title: i18n.tr("Storage")
    flickable: flick

    property AgentModel sm: AgentModel {}
    readonly property var home: sm.storage ? sm.storage.home : null
    readonly property var sys: sm.storage ? sm.storage.system : null
    readonly property var st: sys && sys.store && !sys.store.error ? sys.store : null
    readonly property bool scanning: sm.busy && sm.job.kind === "scan"

    function n(v) { return Number(v) || 0 }

    // [label, bytes, color, page or ""]
    readonly property var parts: {
        if (!home) return []
        var p = []
        if (st) {
            p.push([i18n.tr("System and apps"), n(st.currentSystem), "#b05738", "SystemPage.qml"])
            p.push([i18n.tr("Old system versions"), n(st.oldGenerations), "#f67936", "SystemPage.qml"])
            p.push([i18n.tr("Unused system files"), n(st.garbage), "#f8ae46", "SystemPage.qml"])
            p.push([i18n.tr("Other system files"), n(st.other), "#edb55f", "SystemPage.qml"])
        }
        if (sys) p.push([i18n.tr("System logs"), n(sys.journal), "#d1c35f", "SystemPage.qml"])
        p.push([i18n.tr("Photos and videos"), n(home.pictures) + n(home.videos), "#9dd93f", ""])
        p.push([i18n.tr("Music"), n(home.music), "#82ce5f", ""])
        p.push([i18n.tr("Documents"), n(home.documents), "#76a8b6", ""])
        p.push([i18n.tr("Downloads"), n(home.downloads), "#8891c1", ""])
        p.push([i18n.tr("App data"), n(home.appData), "#a77eaa", "AppsPage.qml"])
        p.push([i18n.tr("App caches"), n(home.appCache), "#cb556c", "AppsPage.qml"])
        var wd = n(home.waydroidUser) + (sys && sys.waydroid ? n(sys.waydroid.images) : 0)
        p.push([i18n.tr("Android (Waydroid)"), wd, "#c27fd1", "AppsPage.qml"])
        var used = n(home.fsTotal) - n(home.fsFree)
        var known = 0
        for (var i = 0; i < p.length; i++) known += p[i][1]
        p.push([i18n.tr("Other files"), Math.max(0, used - known), "#9a9a9a", ""])
        return p
    }

    Component.onCompleted: sm.call("status", {}, function(err, v) {
        if (!err && (!v.storage.home || (Date.now() / 1000 - v.storage.home.time) > 300) && !(v.job && v.job.state === "running"))
            sm.call("storage.scan", {})
    })

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

            Item { width: 1; height: units.gu(1) }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !sm.ok && sm.error !== ""
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: theme.palette.normal.negative
                text: i18n.tr("Storage details are not available: %1").arg(sm.error)
            }

            SettingsListItems.SingleValue {
                objectName: "fp6StorageTotal"
                text: i18n.tr("Total storage")
                value: home ? sm.size(home.fsTotal) : ""
                showDivider: false
            }

            // Stacked bar, like the stock page.
            Item {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                height: units.gu(3)
                Rectangle {
                    anchors.fill: parent
                    color: theme.palette.normal.base
                    radius: units.gu(0.5)
                }
                Row {
                    anchors.fill: parent
                    Repeater {
                        model: home ? root.parts : []
                        Rectangle {
                            height: parent.height
                            width: home && n(home.fsTotal) > 0 ? parent.width * modelData[1] / n(home.fsTotal) : 0
                            color: modelData[2]
                        }
                    }
                }
                ActivityIndicator {
                    anchors.centerIn: parent
                    running: !home
                    visible: running
                }
            }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!home
                text: home ? i18n.tr("%1 free of %2").arg(sm.size(home.fsFree)).arg(sm.size(home.fsTotal)) : ""
            }

            Repeater {
                model: root.parts
                delegate: ListItem.Standard {
                    objectName: "fp6StorageRow" + index
                    progression: modelData[3] !== ""
                    enabled: true
                    showDivider: false
                    height: units.gu(5)
                    Rectangle {
                        id: swatch
                        anchors { left: parent.left; leftMargin: units.gu(2); verticalCenter: parent.verticalCenter }
                        width: units.gu(2); height: width; radius: units.gu(0.4)
                        color: modelData[2]
                    }
                    Label {
                        anchors { left: swatch.right; leftMargin: units.gu(1.5); right: sizeLabel.left; verticalCenter: parent.verticalCenter }
                        text: modelData[0]
                        elide: Text.ElideRight
                    }
                    Label {
                        id: sizeLabel
                        anchors { right: parent.right; rightMargin: modelData[3] !== "" ? units.gu(5) : units.gu(2); verticalCenter: parent.verticalCenter }
                        text: sm.size(modelData[1])
                        color: theme.palette.normal.backgroundSecondaryText
                    }
                    onClicked: if (modelData[3] !== "")
                        pageStack.addPageToNextColumn(root, Qt.resolvedUrl(modelData[3]), { sm: root.sm })
                }
            }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!(sm.storage && sm.storage.systemError)
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: sm.storage ? i18n.tr("System details are not available: %1").arg(sm.storage.systemError) : ""
            }

            ListItem.ThinDivider {}

            SettingsItemTitle { text: i18n.tr("Free up space") }

            SettingsListItems.StandardProgression {
                objectName: "fp6StorageApps"
                text: i18n.tr("App caches and data")
                onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("AppsPage.qml"), { sm: root.sm })
            }
            SettingsListItems.StandardProgression {
                objectName: "fp6StorageSystem"
                text: i18n.tr("Old system versions, unused files and logs")
                onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("SystemPage.qml"), { sm: root.sm })
            }

            Row {
                anchors { left: parent.left; leftMargin: units.gu(2) }
                spacing: units.gu(1)
                Button {
                    objectName: "fp6StorageRescan"
                    text: root.scanning ? i18n.tr("Scanning…") : i18n.tr("Scan again")
                    enabled: !sm.busy
                    onClicked: sm.call("storage.scan", {})
                }
                ActivityIndicator {
                    anchors.verticalCenter: parent.verticalCenter
                    running: root.scanning
                    visible: running
                }
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!home
                fontSize: "small"
                color: theme.palette.normal.backgroundTertiaryText
                text: home ? i18n.tr("Scanned %1").arg(sm.when(home.time)) : ""
            }
        }
    }
}
