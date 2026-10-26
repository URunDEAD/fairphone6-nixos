/*
 * FP6 gesture shortcuts: one shortcut's action (+ app or command).
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import Lomiri.Components 1.3

Column {
    id: chooser

    // GSettings object and the key prefix in camelCase ("shake", "doublePress").
    property var settings
    property string prefix
    property bool allowNone: true
    property var apps: []
    property string title

    readonly property var kinds: allowNone ? ["none", "flashlight", "app", "command"]
                                           : ["flashlight", "app", "command"]
    readonly property var kindLabels: allowNone
        ? [i18n.tr("Nothing"), i18n.tr("Toggle flashlight"), i18n.tr("Open an app"), i18n.tr("Run a custom command")]
        : [i18n.tr("Toggle flashlight"), i18n.tr("Open an app"), i18n.tr("Run a custom command")]
    readonly property string kind: settings ? settings[prefix + "Action"] : ""
    readonly property string appId: settings ? settings[prefix + "App"] : ""
    readonly property string command: settings ? settings[prefix + "Command"] : ""

    function appIndex() {
        for (var i = 0; i < apps.length; i++) {
            if (apps[i].id === appId) return i
        }
        return -1
    }

    spacing: units.gu(1)
    anchors.left: parent.left
    anchors.right: parent.right
    anchors.leftMargin: units.gu(2)
    anchors.rightMargin: units.gu(2)

    Label {
        text: chooser.title
        visible: text !== ""
        font.weight: Font.Medium
    }

    OptionSelector {
        objectName: chooser.prefix + "ActionSelector"
        width: parent.width
        model: chooser.kindLabels
        selectedIndex: Math.max(0, chooser.kinds.indexOf(chooser.kind))
        onDelegateClicked: chooser.settings[chooser.prefix + "Action"] = chooser.kinds[index]
    }

    Label {
        width: parent.width
        visible: chooser.kind === "app" && chooser.apps.length === 0
        wrapMode: Text.WordWrap
        text: i18n.tr("Looking for installed apps…")
    }

    OptionSelector {
        id: appSelector
        objectName: chooser.prefix + "AppSelector"
        width: parent.width
        visible: chooser.kind === "app" && chooser.apps.length > 0
        containerHeight: units.gu(36)
        // Entry 0 is the "nothing chosen yet" placeholder.
        model: [{ id: "", name: i18n.tr("Choose an app…") }].concat(chooser.apps)
        selectedIndex: chooser.appIndex() + 1
        // Waydroid apps share names with native ones (Contacts, Phone).
        delegate: OptionSelectorDelegate {
            text: modelData.name + (modelData.id.indexOf("waydroid.") === 0 ? i18n.tr(" (Android)") : "")
        }
        onDelegateClicked: {
            if (index > 0)
                chooser.settings[chooser.prefix + "App"] = chooser.apps[index - 1].id
        }
    }

    TextField {
        id: commandField
        objectName: chooser.prefix + "CommandField"
        width: parent.width
        visible: chooser.kind === "command"
        placeholderText: i18n.tr("Shell command, e.g. notify-send Hello")
        inputMethodHints: Qt.ImhNoPredictiveText | Qt.ImhNoAutoUppercase
        text: chooser.command
        onTextChanged: saveTimer.restart()
        onAccepted: { saveTimer.stop(); save() }
        function save() {
            if (chooser.settings[chooser.prefix + "Command"] !== text)
                chooser.settings[chooser.prefix + "Command"] = text
        }
        Timer {
            id: saveTimer
            interval: 800
            onTriggered: commandField.save()
        }
    }

    Label {
        width: parent.width
        visible: chooser.kind === "command"
        wrapMode: Text.WordWrap
        fontSize: "small"
        text: i18n.tr("Runs with /bin/sh as your user, never as root, and only while the phone is unlocked. It is stopped after %1 seconds.")
              .arg(chooser.settings ? chooser.settings.commandTimeout : 10)
    }
}
