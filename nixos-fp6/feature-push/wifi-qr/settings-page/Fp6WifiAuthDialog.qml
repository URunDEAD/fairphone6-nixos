// "Confirm it's you" before a saved Wi-Fi password is shown or shared:
// fingerprint (if set up for unlocking) or the phone's passcode/passphrase
// (checked over PAM by WifiShare). Closes itself on success.
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3

Dialog {
    id: dialog
    objectName: "fp6WifiAuthDialog"
    property var share
    // "share" or "password"
    property string reason: "share"
    signal finished(bool ok)

    readonly property bool passcode: share.lockType === "passcode"
    readonly property bool fingerprint: fpLoader.status === Loader.Ready && fpLoader.item.available && fpTries < 5
    property int fpTries: 0
    property string fpStatus: i18n.tr("Touch the fingerprint sensor")
    property bool fpError: false

    title: i18n.tr("Confirm it's you")
    text: {
        var what = reason === "share" ? i18n.tr("To share “%1”, ").arg(share.ssid)
                                      : i18n.tr("To show the password of “%1”, ").arg(share.ssid);
        if (fingerprint)
            return what + (passcode ? i18n.tr("use your fingerprint or enter your passcode.")
                                    : i18n.tr("use your fingerprint or enter your passphrase."));
        return what + (passcode ? i18n.tr("enter your passcode.") : i18n.tr("enter your passphrase."));
    }

    function done(ok) {
        if (fpLoader.item)
            fpLoader.item.cancel();
        PopupUtils.close(dialog);
        dialog.finished(ok);
    }
    function confirm() {
        if (field.text.length > 0 && !share.checking && share.retryAfter === 0) {
            share.checkPasscode(field.text);
            field.text = "";
        }
    }

    Loader {
        id: fpLoader
        visible: false
        active: dialog.share.fingerprintEnabled
        source: "Fp6FingerprintAuth.qml"
        onLoaded: {
            item.identified.connect(function (uid) {
                if (uid !== dialog.share.uid) {
                    dialog.fpError = true;
                    dialog.fpStatus = i18n.tr("Not recognized. Try again.");
                    dialog.fpTries++;
                    if (dialog.fingerprint) item.start();
                    return;
                }
                dialog.share.fingerprintIdentified(uid);
            });
            item.failedAttempt.connect(function (why) {
                dialog.fpTries++;
                dialog.fpError = true;
                dialog.fpStatus = dialog.fingerprint ? i18n.tr("Not recognized. Try again.")
                                                     : (dialog.passcode ? i18n.tr("Use your passcode.") : i18n.tr("Use your passphrase."));
                if (dialog.fingerprint) item.start();
            });
            item.start();
        }
    }

    Connections {
        target: dialog.share
        function onPasscodeChecked(ok, message) {
            if (!ok) {
                errorLabel.text = message;
                field.forceActiveFocus();
            }
        }
        function onAuthChanged() {
            if (dialog.share.authenticated)
                dialog.done(true);
        }
    }

    Row {
        visible: dialog.fingerprint
        spacing: units.gu(1.5)
        height: units.gu(4)
        Icon {
            objectName: "fp6FingerprintIcon"
            width: units.gu(4)
            height: width
            source: Qt.resolvedUrl("fp6-icons/fp6-fingerprint-symbolic.svg")
            color: dialog.fpError ? theme.palette.normal.negative : theme.palette.normal.focus
        }
        Label {
            anchors.verticalCenter: parent.verticalCenter
            text: dialog.fpStatus
            color: dialog.fpError ? theme.palette.normal.negative : theme.palette.normal.backgroundSecondaryText
        }
    }

    TextField {
        id: field
        objectName: "fp6WifiAuthField"
        width: parent.width
        echoMode: TextInput.Password
        placeholderText: dialog.passcode ? i18n.tr("Passcode") : i18n.tr("Passphrase")
        inputMethodHints: (dialog.passcode ? Qt.ImhDigitsOnly : Qt.ImhNoAutoUppercase)
                          | Qt.ImhSensitiveData | Qt.ImhNoPredictiveText | Qt.ImhHiddenText
        enabled: !dialog.share.checking && dialog.share.retryAfter === 0
        onAccepted: dialog.confirm()
        onTextChanged: if (text.length > 0) errorLabel.text = ""
        Component.onCompleted: if (!dialog.fingerprint) forceActiveFocus()
    }

    Label {
        id: errorLabel
        objectName: "fp6WifiAuthError"
        visible: text.length > 0 || dialog.share.retryAfter > 0
        text: ""
        color: theme.palette.normal.negative
        wrapMode: Text.WordWrap
        Connections {
            target: dialog.share
            function onRetryAfterChanged() {
                errorLabel.text = dialog.share.retryAfter > 0
                    ? i18n.tr("Too many attempts. Try again in %1 s.").arg(dialog.share.retryAfter) : "";
            }
        }
    }

    Row {
        width: parent.width
        spacing: units.gu(1)
        Button {
            objectName: "fp6WifiAuthCancel"
            width: (field.width - units.gu(1)) / 2
            text: i18n.tr("Cancel")
            onClicked: dialog.done(false)
        }
        Button {
            objectName: "fp6WifiAuthConfirm"
            width: (field.width - units.gu(1)) / 2
            text: dialog.share.checking ? i18n.tr("Checking…") : i18n.tr("Confirm")
            color: theme.palette.normal.positive
            enabled: field.text.length > 0 && !dialog.share.checking && dialog.share.retryAfter === 0
            onClicked: dialog.confirm()
        }
    }
}
