/*
 * FP6 app permissions: all apps; tap for one app's permissions.
 * GPL-3.0-only.
 */
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItems
import SystemSettings 1.0
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "fp6AppListPage"
    property var model
    property bool networkView: false
    title: networkView ? i18n.tr("Network access") : i18n.tr("Permissions by app")
    flickable: flick

    function summary(app) {
        if (networkView) return model.networkSummary(app);
        if (model.isSystem(app)) return i18n.tr("System");
        var n = 0;
        for (var i = 0; i < model.perms.length; i++) {
            var d = model.decision(app, model.perms[i].id);
            if (d === "allow" || d === "while_using") n++;
        }
        var net = model.networkSummary(app);
        var parts = [];
        if (n > 0) parts.push(i18n.tr("%1 allowed").arg(n));
        if (net !== i18n.tr("Allowed")) parts.push(net);
        return parts.length ? parts.join(" · ") : i18n.tr("Nothing extra");
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: col.height
        boundsBehavior: contentHeight > height ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds
        Column {
            id: col
            anchors { left: parent.left; right: parent.right }
            ListItems.Caption {
                visible: page.networkView
                text: i18n.tr("Every app may use the network until you limit it here. System components always keep theirs.")
            }
            Repeater {
                model: page.model.allApps()
                delegate: SettingsListItems.IconProgression {
                    objectName: "fp6App_" + modelData
                    text: page.model.name(modelData)
                    iconSource: page.model.icon(modelData)
                    iconName: "application-x-executable"
                    layout.subtitle.text: page.summary(modelData)
                    onClicked: pageStack.addPageToNextColumn(page, Qt.resolvedUrl("Fp6AppPermissionsPage.qml"), { model: page.model, app: modelData })
                }
            }
        }
    }
}
