/*
 * App Launch > one app: keep ready / start at login / preload.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "appLaunchOptionsPage"
    property string appId
    property var model
    readonly property var info: model.app(appId)
    readonly property var s: model.appStatus(appId)
    readonly property var gs: model.settings
    title: info.name
    flickable: flick

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

            SettingsListItems.Standard {
                text: i18n.tr("Keep in memory")
                Switch {
                    objectName: "keepReadySwitch"
                    property bool serverChecked: model.has("keepReady", appId) || model.has("prelaunch", appId)
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: {
                        model.set("keepReady", appId, checked)
                        if (!checked) model.set("prelaunch", appId, false)
                    }
                }
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("When you switch away, the app is suspended instead of running, and it is closed last when memory runs low. Coming back is a resume, not a restart.")
            }

            SettingsListItems.Standard {
                text: i18n.tr("Start at login")
                Switch {
                    objectName: "prelaunchSwitch"
                    property bool serverChecked: model.has("prelaunch", appId)
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: {
                        model.set("prelaunch", appId, checked)
                        if (checked) model.set("keepReady", appId, true)
                    }
                }
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Started hidden in the background shortly after you log in (and now, when you turn this on), then kept ready. The first time you open it is already fast.")
            }

            SettingsListItems.Standard {
                text: i18n.tr("Preload files")
                Switch {
                    objectName: "preloadSwitch"
                    property bool serverChecked: model.has("preload", appId)
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: model.set("preload", appId, checked)
                }
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("The app's program, libraries and data are read into the file cache at login and every two hours, so even a fresh start does not wait for storage. The list is learned from the files the app uses.")
            }

            ListItem.ThinDivider {}

            SettingsItemTitle { text: i18n.tr("Now") }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: {
                    var t = model.stateText(s)
                    if (s && s.running)
                        t += "\n" + i18n.tr("Memory: %1 in RAM, %2 compressed (%3 processes)").arg(model.mb(s.memory)).arg(model.mb(s.swap)).arg(s.processes)
                    if (s && s.preload && s.preloadBytes > 0)
                        t += "\n" + i18n.tr("Files: %1 of %2 cached (%3 files%4)").arg(model.mb(s.cachedBytes)).arg(model.mb(s.preloadBytes)).arg(s.preloadFiles)
                            .arg(s.learned ? "" : i18n.tr(", from the package; learned after the first use"))
                    return t
                }
            }

            Button {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                objectName: "removeButton"
                text: i18n.tr("Remove from the list")
                onClicked: {
                    model.remove(appId)
                    pageStack.removePages(page)
                }
            }
        }
    }
}
