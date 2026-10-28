/*
 * FP6 app permissions: the block at the top of Security & Privacy > App
 * permissions (inserted into the stock AppAccess.qml): one row per
 * permission, network access, all apps, history. GPL-3.0-only.
 */
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItems
import SystemSettings 1.0
import SystemSettings.ListItems 1.0 as SettingsListItems

Column {
    id: section
    objectName: "fp6PermissionsSection"
    property var page          // the page to push next to (the AppAccess page)
    readonly property alias permissions: pm
    anchors.left: parent ? parent.left : undefined
    anchors.right: parent ? parent.right : undefined

    Fp6PermissionsModel { id: pm }

    function open(file, props) {
        props = props || {};
        props.model = pm;
        pageStack.addPageToNextColumn(page, Qt.resolvedUrl(file), props);
    }

    ListItems.Caption {
        visible: pm.available && !pm.brokerUp
        text: i18n.tr("The permission service is not running; apps are refused until it is back.")
    }

    SettingsItemTitle { text: i18n.tr("Device & data access") }

    Repeater {
        model: pm.perms
        delegate: SettingsListItems.SingleValueProgression {
            objectName: "fp6Perm_" + modelData.id
            text: modelData.title
            property int asked: pm.requested(modelData.id).length
            value: asked > 0 ? i18n.tr("%1/%2").arg(pm.grantedCount(modelData.id)).arg(asked) : i18n.tr("None")
            onClicked: section.open("Fp6PermissionAppsPage.qml", { perm: modelData.id })
        }
    }

    SettingsListItems.SingleValueProgression {
        objectName: "fp6PermNetwork"
        text: i18n.tr("Network access")
        value: {
            var apps = pm.allApps(), n = 0;
            for (var i = 0; i < apps.length; i++) if (pm.networkSummary(apps[i]) !== i18n.tr("Allowed") && !pm.isSystem(apps[i])) n++;
            return n > 0 ? i18n.tr("%1 app limited", "%1 apps limited", n).arg(n) : i18n.tr("All apps");
        }
        onClicked: section.open("Fp6NetworkAppsPage.qml")
    }

    SettingsListItems.StandardProgression {
        objectName: "fp6PermAllApps"
        text: i18n.tr("Permissions by app")
        onClicked: section.open("Fp6AppListPage.qml")
    }

    SettingsListItems.StandardProgression {
        objectName: "fp6PermHistory"
        text: i18n.tr("Permission history")
        onClicked: section.open("Fp6PermissionHistoryPage.qml")
    }

    ListItems.Caption {
        text: i18n.tr("Apps ask the first time they need one of these. System components (Settings, Phone, Messaging, the terminal, USB and sharing services) keep their access.")
    }

    ListItems.Caption {
        visible: pm.media.length > 0
        text: i18n.tr("Camera, microphone and location: apps ask the first time they use them (Allow, Only this time, Don't allow); camera and microphone through PipeWire, location while Location is on. The Camera app has camera and microphone access to start with.")
    }

    // Header of the stock (trust-store) list below, when it is still shown.
    SettingsItemTitle {
        visible: pm.media.length === 0
        text: i18n.tr("Camera, location and microphone")
    }
}
