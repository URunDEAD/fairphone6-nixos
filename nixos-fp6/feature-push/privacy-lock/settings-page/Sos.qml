/*
 * Security & Privacy > Emergency SOS (FP6 privacy-lock). Off by default.
 * Backend: fp6-privacyd (counts com.fairphone6.PowerKey.Press, countdown
 * notification with Cancel, oFono Dial / SendMessage). Test mode: mock
 * dialer, nothing is called or sent.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.4
import SystemSettings 1.0
import SystemSettings.ListItems 1.0 as SettingsListItems
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItems
import Lomiri.Components.Popups 1.3

ItemPage {
    id: root
    objectName: "fp6SosPage"
    title: i18n.tr("Emergency SOS")
    flickable: flick

    property var settings
    readonly property var pressValues: [3, 5]
    readonly property var countdownValues: [3, 5, 10]

    Component {
        id: confirmDialog
        Dialog {
            id: dialog
            title: i18n.tr("Turn on Emergency SOS?")
            text: i18n.tr("Pressing the power button %1 times quickly will call %2 after a %3 second countdown, even when the phone is locked. Calling emergency services by mistake can tie up help others need: cancel at once if it starts by accident. Try it first with Test mode on.")
                    .arg(root.settings.sosPresses || 5).arg(root.settings.sosNumber || "112").arg(root.settings.sosCountdown || 5)
            Button {
                objectName: "fp6SosConfirm"
                text: i18n.tr("Turn on")
                color: theme.palette.normal.negative
                onClicked: { root.settings.sosEnabled = true; PopupUtils.close(dialog); }
            }
            Button {
                text: i18n.tr("Cancel")
                onClicked: { sosSwitch.checked = false; PopupUtils.close(dialog); }
            }
        }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: column.height + units.gu(4)

        Column {
            id: column
            anchors { left: parent.left; right: parent.right }

            SettingsListItems.Standard {
                text: i18n.tr("Press the power button quickly to call for help")
                Switch {
                    id: sosSwitch
                    objectName: "fp6SosSwitch"
                    property bool serverChecked: root.settings && root.settings.sosEnabled === true
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: {
                        if (checked) PopupUtils.open(confirmDialog);
                        else root.settings.sosEnabled = false;
                    }
                }
            }
            ListItems.Caption {
                text: i18n.tr("Off by default. A notification counts down with a Cancel button and the phone vibrates; then the call starts.")
            }

            SettingsListItems.Standard {
                text: i18n.tr("Test mode (no call, no SMS)")
                Switch {
                    objectName: "fp6SosTestMode"
                    property bool serverChecked: root.settings && root.settings.testMode === true
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: root.settings.testMode = checked
                }
            }
            ListItems.Caption {
                text: i18n.tr("Shows what SOS and the lock-screen contact buttons would do instead of calling or texting.")
            }

            SettingsItemTitle { text: i18n.tr("Emergency number") }
            TextField {
                objectName: "fp6SosNumber"
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                inputMethodHints: Qt.ImhDialableCharactersOnly
                text: root.settings ? root.settings.sosNumber : "112"
                onEditingFinished: if (/^[0-9+*#]{2,20}$/.test(text)) root.settings.sosNumber = text
            }
            ListItems.Caption { text: i18n.tr("112 works in the EU and on most GSM networks worldwide.") }

            ListItems.ItemSelector {
                text: i18n.tr("Presses")
                model: root.pressValues.map(function (v) { return i18n.tr("%1 presses").arg(v); })
                selectedIndex: Math.max(0, root.pressValues.indexOf(root.settings ? root.settings.sosPresses : 5))
                onDelegateClicked: root.settings.sosPresses = root.pressValues[index]
            }
            ListItems.ItemSelector {
                text: i18n.tr("Countdown")
                model: root.countdownValues.map(function (v) { return i18n.tr("%1 seconds").arg(v); })
                selectedIndex: Math.max(0, root.countdownValues.indexOf(root.settings ? root.settings.sosCountdown : 5))
                onDelegateClicked: root.settings.sosCountdown = root.countdownValues[index]
            }

            SettingsListItems.Standard {
                text: i18n.tr("Text my emergency contacts")
                Switch {
                    objectName: "fp6SosSms"
                    property bool serverChecked: root.settings && root.settings.sosSendSms === true
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: root.settings.sosSendSms = checked
                }
            }
            ListItems.Caption {
                text: i18n.tr("After the call starts, sends an SMS with your location (if location is on) to the contacts in Emergency information.")
            }
        }
    }
}
