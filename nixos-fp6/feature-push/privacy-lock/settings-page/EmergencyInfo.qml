/*
 * Security & Privacy > Emergency information (FP6 privacy-lock).
 * Stored in GSettings emergency-info (JSON, this user's dconf). While "Show
 * on lock screen" is on, fp6-privacyd copies the known fields to
 * /var/lib/fp6-privacy/emergency.json, readable by this user and the
 * LightDM greeter only; the lock screen's Emergency button shows it.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.4
import SystemSettings 1.0
import SystemSettings.ListItems 1.0 as SettingsListItems
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItems

ItemPage {
    id: root
    objectName: "fp6EmergencyInfoPage"
    title: i18n.tr("Emergency information")
    flickable: flick

    property var settings
    property var info: ({})
    property bool loaded: false

    Component.onCompleted: {
        try { info = JSON.parse(settings.emergencyInfo || "{}"); } catch (e) { info = {}; }
        if (!info.contacts) info.contacts = [];
        loaded = true;
    }

    function set(key, value) {
        if (!loaded) return;
        var copy = JSON.parse(JSON.stringify(info));
        copy[key] = value;
        info = copy;
        saveTimer.restart();
    }

    function setContact(i, key, value) {
        var list = JSON.parse(JSON.stringify(info.contacts || []));
        list[i][key] = value;
        set("contacts", list);
    }

    Timer {
        id: saveTimer
        interval: 600
        onTriggered: root.settings.emergencyInfo = JSON.stringify(root.info)
    }
    Component.onDestruction: if (saveTimer.running) root.settings.emergencyInfo = JSON.stringify(root.info)

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: column.height + units.gu(6)
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: column
            anchors { left: parent.left; right: parent.right }
            spacing: units.gu(0.5)

            SettingsListItems.Standard {
                text: i18n.tr("Show on lock screen")
                Switch {
                    objectName: "fp6EmergencyShow"
                    property bool serverChecked: root.settings && root.settings.emergencyOnLockscreen === true
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: root.settings.emergencyOnLockscreen = checked
                }
            }
            ListItems.Caption {
                text: i18n.tr("Anyone holding the phone can then read this from the lock screen (Emergency) and call your emergency contacts without unlocking. Only fill in what you want a stranger or a paramedic to see.")
            }

            SettingsItemTitle { text: i18n.tr("About you") }

            Repeater {
                model: [
                    { key: "ownerName", label: i18n.tr("Name"), multi: false },
                    { key: "ownerMessage", label: i18n.tr("Message on the lock screen (e.g. \"If found, please call…\")"), multi: true },
                    { key: "bloodType", label: i18n.tr("Blood type"), multi: false },
                    { key: "allergies", label: i18n.tr("Allergies"), multi: true },
                    { key: "medications", label: i18n.tr("Medications"), multi: true },
                    { key: "conditions", label: i18n.tr("Medical conditions"), multi: true },
                    { key: "notes", label: i18n.tr("Other notes"), multi: true }
                ]
                delegate: Column {
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    spacing: units.gu(0.5)
                    Label { text: modelData.label; fontSize: "small"; width: parent.width; wrapMode: Text.WordWrap }
                    TextArea {
                        objectName: "fp6Emergency_" + modelData.key
                        width: parent.width
                        autoSize: true
                        maximumLineCount: modelData.multi ? 6 : 1
                        text: root.loaded ? (root.info[modelData.key] || "") : ""
                        onTextChanged: if (root.loaded && text !== (root.info[modelData.key] || "")) root.set(modelData.key, text)
                    }
                }
            }

            SettingsListItems.Standard {
                text: i18n.tr("Organ donor")
                Switch {
                    property bool serverChecked: root.info.organDonor === true
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: root.set("organDonor", checked)
                }
            }

            SettingsItemTitle { text: i18n.tr("Emergency contacts") }

            Repeater {
                model: root.info.contacts ? root.info.contacts.length : 0
                delegate: Column {
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    spacing: units.gu(0.5)
                    Row {
                        spacing: units.gu(1)
                        width: parent.width
                        TextField {
                            width: (parent.width - removeButton.width - units.gu(2)) / 2
                            placeholderText: i18n.tr("Name")
                            text: root.info.contacts[index] ? root.info.contacts[index].name || "" : ""
                            onTextChanged: if (root.info.contacts[index] && text !== (root.info.contacts[index].name || "")) root.setContact(index, "name", text)
                        }
                        TextField {
                            width: (parent.width - removeButton.width - units.gu(2)) / 2
                            placeholderText: i18n.tr("Relation")
                            text: root.info.contacts[index] ? root.info.contacts[index].relation || "" : ""
                            onTextChanged: if (root.info.contacts[index] && text !== (root.info.contacts[index].relation || "")) root.setContact(index, "relation", text)
                        }
                        Button {
                            id: removeButton
                            width: units.gu(5)
                            iconName: "delete"
                            onClicked: {
                                var list = JSON.parse(JSON.stringify(root.info.contacts));
                                list.splice(index, 1);
                                root.set("contacts", list);
                            }
                        }
                    }
                    TextField {
                        width: parent.width
                        placeholderText: i18n.tr("Phone number")
                        inputMethodHints: Qt.ImhDialableCharactersOnly
                        text: root.info.contacts[index] ? root.info.contacts[index].number || "" : ""
                        onTextChanged: if (root.info.contacts[index] && text !== (root.info.contacts[index].number || "")) root.setContact(index, "number", text)
                    }
                }
            }

            Button {
                anchors { left: parent.left; leftMargin: units.gu(2) }
                visible: !root.info.contacts || root.info.contacts.length < 5
                text: i18n.tr("Add emergency contact")
                onClicked: {
                    var list = JSON.parse(JSON.stringify(root.info.contacts || []));
                    list.push({ name: "", relation: "", number: "" });
                    root.set("contacts", list);
                }
            }
            ListItems.Caption {
                text: i18n.tr("Up to five. Numbers with letters are not shown on the lock screen. Emergency SOS can text these contacts with your location.")
            }
        }
    }
}
