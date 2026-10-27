// Network details (Settings > Wi-Fi > connected network, and > Previous
// networks > network): replaces the stock "Password" / "Show password" rows
// (which showed every saved password without asking) by a confirmed
// password row, and adds "Share (QR code)". Both ask for the fingerprint or
// passcode first (Fp6WifiAuthDialog) unless the phone has no screen lock.
import QtQuick 2.12
import SystemSettings.ListItems 1.0 as SettingsListItems
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import Fp6.WifiQr 1.0

Column {
    id: extras
    objectName: "fp6WifiDetailsExtras"
    property string networkName
    // NetworkManager settings path ("" = the connected network)
    property string connectionPath
    // the details ItemPage (pageStack / popup parent)
    property var page
    readonly property alias share: share
    property bool showPassword: false
    readonly property bool passwordShown: showPassword && share.revealed

    anchors { left: parent.left; right: parent.right }

    WifiShare {
        id: share
        ssid: extras.networkName
        connectionPath: extras.connectionPath
        onRevealedChanged: if (!revealed) extras.showPassword = false
    }

    function authenticateThen(action) {
        if (!share.needsAuth) {
            run(action);
            return;
        }
        var d = PopupUtils.open(Qt.resolvedUrl("Fp6WifiAuthDialog.qml"), extras.page, { share: share, reason: action });
        d.finished.connect(function (ok) { if (ok) extras.run(action); });
    }
    function run(action) {
        if (action === "share") {
            pageStack.addPageToNextColumn(extras.page, Qt.resolvedUrl("Fp6WifiSharePage.qml"), { share: share });
        } else {
            showPassword = true;
            if (!share.revealed)
                share.reveal();
        }
    }

    SettingsListItems.Standard {
        objectName: "fp6PasswordRow"
        text: i18n.tr("Password")
        visible: share.hasPassword
        Label {
            objectName: "fp6PasswordValue"
            SlotsLayout.position: SlotsLayout.Trailing
            width: Math.min(implicitWidth, extras.width * 0.55)
            elide: Text.ElideMiddle
            text: extras.passwordShown ? share.password : "••••••••"
            color: extras.passwordShown ? theme.palette.normal.backgroundText : theme.palette.normal.backgroundSecondaryText
        }
    }

    SettingsListItems.Standard {
        objectName: "fp6ShowPasswordRow"
        text: i18n.tr("Show password")
        visible: share.hasPassword
        Switch {
            id: showSwitch
            objectName: "fp6ShowPasswordSwitch"
            checked: extras.showPassword
            onClicked: {
                if (extras.showPassword)
                    extras.showPassword = false;
                else
                    extras.authenticateThen("password");
                checked = Qt.binding(function () { return extras.showPassword; });
            }
        }
    }

    SettingsListItems.IconProgression {
        objectName: "fp6ShareQr"
        text: i18n.tr("Share (QR code)")
        iconSource: Qt.resolvedUrl("fp6-icons/fp6-qrcode-symbolic.svg")
        visible: share.found
        enabled: share.shareable
        layout.subtitle.text: share.shareable ? "" : i18n.tr("Not available for enterprise (802.1X) networks")
        onClicked: extras.authenticateThen("share")
    }
}
