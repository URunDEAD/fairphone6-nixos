/*
 * FP6 app permissions: the apps that asked for one permission, with their
 * answer; tap for the app's page. GPL-3.0-only.
 */
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItems
import SystemSettings 1.0
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "fp6PermissionAppsPage"
    property var model
    property string perm: "usb"
    readonly property var info: model.permInfo(perm)
    title: info.title
    flickable: flick

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: col.height
        boundsBehavior: contentHeight > height ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds

        Column {
            id: col
            anchors { left: parent.left; right: parent.right }

            ListItems.Caption { text: page.info.caption || "" }

            Repeater {
                id: rep
                model: page.model.requested(page.perm)
                delegate: SettingsListItems.IconProgression {
                    objectName: "fp6PermApp_" + modelData
                    text: page.model.name(modelData)
                    iconSource: page.model.icon(modelData)
                    iconName: "application-x-executable"
                    layout.subtitle.text: page.model.decisionLabel(page.model.decision(modelData, page.perm))
                                          + " · " + i18n.tr("last used %1").arg(page.model.timeText(page.model.perm(modelData, page.perm).last_used).toLowerCase())
                    onClicked: pageStack.addPageToNextColumn(page, Qt.resolvedUrl("Fp6AppPermissionsPage.qml"), { model: page.model, app: modelData })
                }
            }

            ListItems.Caption {
                visible: rep.count === 0
                text: i18n.tr("No app has asked for this yet.")
            }
        }
    }
}
