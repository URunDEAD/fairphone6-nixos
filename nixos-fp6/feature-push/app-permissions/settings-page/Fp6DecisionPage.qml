/*
 * FP6 app permissions: choose the answer for one app and permission.
 * GPL-3.0-only.
 */
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItems
import SystemSettings 1.0
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "fp6DecisionPage"
    property var model
    property string app
    property string perm
    title: model.permInfo(perm).title
    readonly property string current: model.decision(app, perm)

    Column {
        anchors { left: parent.left; right: parent.right; top: page.header.bottom }

        ListItems.Caption {
            text: i18n.tr("%1 access for %2").arg(page.model.permInfo(page.perm).title).arg(page.model.name(page.app))
        }

        Repeater {
            model: [
                { id: "allow", text: i18n.tr("Allow") },
                { id: "while_using", text: i18n.tr("Allow only while using the app") },
                { id: "ask", text: i18n.tr("Ask every time") },
                { id: "deny", text: i18n.tr("Don't allow") }
            ]
            delegate: SettingsListItems.Standard {
                objectName: "fp6Decision_" + modelData.id
                text: modelData.text
                Icon {
                    width: units.gu(2.5); height: width
                    name: "tick"
                    visible: page.current === modelData.id
                    SlotsLayout.position: SlotsLayout.Trailing
                }
                onClicked: page.model.set(page.app, page.perm, modelData.id)
            }
        }

        ListItems.Caption {
            visible: page.model.isPreset(page.app, page.perm)
            text: i18n.tr("This is the app's default. Reset permissions on the app's page brings it back after a change.")
        }

        ListItems.Caption {
            text: {
                switch (page.perm) {
                case "usb": return i18n.tr("Taking USB access away disconnects the device for a moment, so the app loses it at once.");
                case "camera":
                case "microphone": return i18n.tr("Applies at once: a recording in progress stops when access is taken away. \"Only this time\" is offered when the app asks.");
                case "location": return i18n.tr("Applies to the app's next location request. \"Only this time\" is offered when the app asks.");
                default: return i18n.tr("Changes apply to the next request; confined apps lose file and data access at once.");
                }
            }
        }
    }
}
