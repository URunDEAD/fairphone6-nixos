/*
 * System Settings > Voice Dictation (FP6): show or hide the microphone key
 * on the on-screen keyboard (GSettings org.maliit.keyboard.maliit
 * dictation-key, read live by maliit-keyboard). Language and model of the
 * dictation service are in the Voice Dictation app (fp6-dictation-settings).
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems
import GSettings 1.0

ItemPage {
    id: root
    objectName: "voiceDictationPage"
    title: i18n.tr("Voice Dictation")
    flickable: flick

    GSettings {
        id: keyboard
        schema.id: "org.maliit.keyboard.maliit"
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        boundsBehavior: (contentHeight > root.height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick

        Column {
            id: content
            anchors.left: parent.left
            anchors.right: parent.right
            spacing: units.gu(1)

            SettingsItemTitle { text: i18n.tr("Keyboard") }

            SettingsListItems.Standard {
                text: i18n.tr("Microphone key on the keyboard")
                enabled: keyboard.schema.isValid
                Switch {
                    objectName: "dictationKeySwitch"
                    property bool serverChecked: keyboard.dictationKey === true
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: keyboard.dictationKey = checked
                }
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: keyboard.schema.isValid
                    ? i18n.tr("The key sits next to the language key while the dictation service is running, in normal text fields (not in password or PIN fields). Tap it and speak, then tap again: the text is written where the cursor is. Hold it to cancel. Speech is recognised on the phone; nothing is sent anywhere.")
                    : i18n.tr("The keyboard settings are not installed.")
            }

            ListItem.ThinDivider {}

            SettingsItemTitle { text: i18n.tr("Recognition") }

            SettingsListItems.Standard {
                text: i18n.tr("Language and speech model")
                progression: true
                onClicked: Qt.openUrlExternally("application:///fp6-dictation.desktop")
            }
        }
    }
}
