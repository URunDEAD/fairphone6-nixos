/*
 * Do Not Disturb > Calls, messages and apps: what still gets through.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "dndExceptionsPage"
    property var dm
    readonly property var gs: dm.settings
    title: i18n.tr("Calls, messages and apps")
    flickable: flick

    readonly property var fromValues: ["everyone", "contacts", "favourites", "nobody"]
    readonly property var fromNames: [i18n.tr("Everyone"), i18n.tr("Contacts"), i18n.tr("Favourite contacts"), i18n.tr("Nobody")]

    // Apps seen so far (names only) plus the ones already allowed.
    readonly property var appNames: {
        var out = (dm.status && dm.status.apps) ? dm.status.apps.slice() : []
        var allowed = gs.dndAllowedApps || []
        for (var i = 0; i < allowed.length; i++) if (out.indexOf(allowed[i]) < 0) out.push(allowed[i])
        return out
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        Column {
            id: content
            anchors { left: parent.left; right: parent.right }
            spacing: units.gu(1)

            SettingsItemTitle { text: i18n.tr("Calls that ring") }
            OptionSelector {
                objectName: "callsFrom"
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                expanded: true
                model: page.fromNames
                selectedIndex: Math.max(0, page.fromValues.indexOf(gs.dndCallsFrom))
                onDelegateClicked: gs.dndCallsFrom = page.fromValues[index]
            }
            SettingsListItems.Standard {
                text: i18n.tr("Repeated callers (2nd call within 15 minutes)")
                Switch {
                    objectName: "repeatCallers"
                    property bool serverChecked: gs.dndRepeatCallers
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.dndRepeatCallers = checked
                }
            }

            SettingsItemTitle { text: i18n.tr("Messages (SMS/MMS) that alert") }
            OptionSelector {
                objectName: "messagesFrom"
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                expanded: true
                model: page.fromNames
                selectedIndex: Math.max(0, page.fromValues.indexOf(gs.dndMessagesFrom))
                onDelegateClicked: gs.dndMessagesFrom = page.fromValues[index]
            }
            Label {
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Favourites are the contacts marked with a star in Contacts.")
            }

            SettingsItemTitle { text: i18n.tr("Numbers that always get through") }
            Repeater {
                model: gs.dndAllowedNumbers
                delegate: SettingsListItems.Standard {
                    text: modelData
                    Icon {
                        name: "delete"
                        width: units.gu(2.5); height: width
                        SlotsLayout.position: SlotsLayout.Trailing
                    }
                    onClicked: gs.dndAllowedNumbers = page.dm.listWith(gs.dndAllowedNumbers, modelData, false)
                }
            }
            Row {
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                spacing: units.gu(1)
                TextField {
                    id: number
                    objectName: "addNumberField"
                    width: parent.width - addNumber.width - parent.spacing
                    inputMethodHints: Qt.ImhDialableCharactersOnly
                    placeholderText: i18n.tr("Phone number")
                }
                Button {
                    id: addNumber
                    objectName: "addNumber"
                    text: i18n.tr("Add")
                    enabled: number.text.replace(/[^0-9]/g, "").length >= 3
                    onClicked: {
                        gs.dndAllowedNumbers = page.dm.listWith(gs.dndAllowedNumbers, number.text.trim(), true)
                        number.text = ""
                    }
                }
            }

            SettingsItemTitle { text: i18n.tr("Apps whose notifications still show") }
            Label {
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: page.appNames.length === 0
                    ? i18n.tr("Apps appear here after they have shown a notification.")
                    : i18n.tr("Alarms, incoming-call screens and urgent notifications always show.")
            }
            Repeater {
                model: page.appNames
                delegate: SettingsListItems.Standard {
                    objectName: "allowApp_" + modelData
                    text: modelData
                    CheckBox {
                        SlotsLayout.position: SlotsLayout.Leading
                        checked: (gs.dndAllowedApps || []).indexOf(modelData) >= 0
                        onTriggered: gs.dndAllowedApps = page.dm.listWith(gs.dndAllowedApps || [], modelData, checked)
                    }
                }
            }
        }
    }
}
