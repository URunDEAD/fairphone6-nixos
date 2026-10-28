/*
 * FP6 app permissions: recent uses, refusals, prompts and changes (all
 * apps, or one), newest first, from fp6-permd's history (camera and
 * location from the privacy indicator). GPL-3.0-only.
 */
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItems
import SystemSettings 1.0
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "fp6PermissionHistoryPage"
    property var model
    property string app: ""
    title: app ? i18n.tr("History: %1").arg(model.name(app)) : i18n.tr("Permission history")
    flickable: list

    ListView {
        id: list
        anchors.fill: parent
        model: page.model.history(page.app)
        header: ListItems.Caption {
            text: i18n.tr("The last 200 events. Repeated uses within a minute count once.")
        }
        delegate: SettingsListItems.Icon {
            objectName: "fp6History_" + index
            readonly property var info: page.model.permInfo(modelData.perm)
            iconName: info.icon
            text: (page.app ? "" : page.model.name(modelData.app) + " · ") + info.title
            layout.subtitle.text: page.model.kindText(modelData.kind, modelData.detail)
                                  + ((modelData.detail && modelData.kind !== "prompted" && modelData.kind !== "changed") ? " · " + modelData.detail : "")
            layout.summary.text: page.model.timeText(modelData.time)
        }
        Label {
            anchors.centerIn: parent
            visible: list.count === 0
            text: i18n.tr("Nothing yet")
        }
    }
}
