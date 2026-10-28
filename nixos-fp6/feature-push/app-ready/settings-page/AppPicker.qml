/*
 * Memory & performance > Add apps: tap an app to always keep it ready (and
 * preload it), or, with target "never", to never keep it ready.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem

ItemPage {
    id: picker
    objectName: "appLaunchPickerPage"
    property var model
    // "keep": always keep ready; "never": never keep ready
    property string target: "keep"
    title: target === "never" ? i18n.tr("Never keep ready") : i18n.tr("Add apps")
    flickable: list

    readonly property var candidates: {
        var out = []
        for (var i = 0; i < model.apps.length; i++)
            if ((target === "never" ? model.neverList : model.chosen).indexOf(model.apps[i].id) < 0) out.push(model.apps[i])
        return out
    }

    ListView {
        id: list
        anchors.fill: parent
        model: picker.candidates
        delegate: ListItem.Standard {
            objectName: "pick_" + modelData.id
            iconSource: picker.model.iconSource(modelData.icon)
            iconFrame: false
            text: modelData.name
            onClicked: {
                if (picker.target === "never") {
                    picker.model.setNever(modelData.id, true)
                } else {
                    picker.model.set("neverKeep", modelData.id, false)
                    picker.model.set("keepReady", modelData.id, true)
                    picker.model.set("preload", modelData.id, true)
                }
            }
        }
        Label {
            anchors.centerIn: parent
            visible: list.count === 0
            text: i18n.tr("No more apps")
        }
    }
}
