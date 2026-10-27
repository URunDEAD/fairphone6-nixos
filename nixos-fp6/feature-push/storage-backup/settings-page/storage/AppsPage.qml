/*
 * Storage > Apps: cache, data and settings per app (~/.cache,
 * ~/.local/share, ~/.config entries); clearing caches only.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import Lomiri.Components.Popups 1.3
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "fp6StorageAppsPage"
    title: i18n.tr("Apps")
    flickable: flick

    property var sm
    readonly property var home: sm && sm.storage ? sm.storage.home : null
    readonly property var sys: sm && sm.storage ? sm.storage.system : null
    readonly property bool busy: !!(sm && sm.busy)
    readonly property var last: sm && sm.storage ? sm.storage.lastAction : null

    function iconSource(icon) {
        if (!icon) return "image://theme/package-x-generic-symbolic"
        if (icon.indexOf("/") === 0) return "file://" + icon
        return "image://theme/" + icon
    }

    Component {
        id: confirmClear
        Dialog {
            id: d
            property string appId: ""
            property string name: ""
            title: appId ? i18n.tr("Clear the cache of %1?").arg(name) : i18n.tr("Clear all app caches?")
            text: i18n.tr("Cached files (thumbnails, web caches, downloaded map tiles) are deleted; apps rebuild them when needed. App data and settings stay.")
            Button {
                objectName: "fp6ConfirmClear"
                text: i18n.tr("Clear")
                color: theme.palette.normal.negative
                onClicked: { sm.call("storage.clearCache", d.appId ? { id: d.appId } : {}); PopupUtils.close(d) }
            }
            Button { text: i18n.tr("Cancel"); onClicked: PopupUtils.close(d) }
        }
    }

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

            SettingsListItems.SingleValue {
                text: i18n.tr("App caches")
                value: home ? sm.size(home.appCache) : ""
                showDivider: false
            }
            SettingsListItems.SingleValue {
                text: i18n.tr("App data and settings")
                value: home ? sm.size(home.appData) : ""
                showDivider: false
            }
            SettingsListItems.SingleValue {
                text: i18n.tr("Android (Waydroid)")
                value: home ? sm.size(Number(home.waydroidUser) + (sys && sys.waydroid ? Number(sys.waydroid.images) : 0)) : ""
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!(sys && sys.waydroid)
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: theme.palette.normal.backgroundTertiaryText
                text: sys && sys.waydroid ? i18n.tr("Waydroid system images %1, Android data %2.").arg(sm.size(sys.waydroid.images)).arg(sm.size(home ? home.waydroidUser : 0)) : ""
            }

            Button {
                objectName: "fp6ClearAll"
                anchors { left: parent.left; leftMargin: units.gu(2) }
                enabled: !!home && home.appCache > 0 && !page.busy
                text: i18n.tr("Clear all app caches")
                onClicked: PopupUtils.open(confirmClear, page, { appId: "", name: "" })
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!(last && last.kind === "clearCache") && !page.busy
                fontSize: "small"
                color: theme.palette.normal.backgroundSecondaryText
                text: last && last.kind === "clearCache" ? i18n.tr("Freed %1.").arg(sm.size(last.freed)) : ""
            }

            ListItem.ThinDivider {}
            SettingsItemTitle { text: i18n.tr("By app") }

            Repeater {
                model: home ? home.apps.slice(0, 40) : []
                delegate: ListItem.Subtitled {
                    objectName: "fp6App_" + modelData.id
                    iconSource: page.iconSource(modelData.icon)
                    iconFrame: false
                    text: modelData.name
                    subText: i18n.tr("Cache %1 · Data %2 · Settings %3").arg(sm.size(modelData.cache)).arg(sm.size(modelData.data)).arg(sm.size(modelData.config))
                    showDivider: false
                    onClicked: if (modelData.cache > 0 && !page.busy)
                        PopupUtils.open(confirmClear, page, { appId: modelData.id, name: modelData.name })
                }
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: theme.palette.normal.backgroundTertiaryText
                text: i18n.tr("Tap an app to clear its cache. Lomiri's own start-up cache is left alone by \"Clear all\".")
            }
        }
    }
}
