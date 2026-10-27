// Fingerprint check for the Wi-Fi share/password confirmation, the same
// biometryd identify operation as the Lomiri greeter. Loaded through a
// Loader: without the Biometryd module (laptop preview) the dialog falls
// back to the passcode only. (An Item root: Loader loads visual types only.)
import QtQuick 2.12
import Biometryd 0.0

Item {
    id: root
    signal identified(int uid)
    signal failedAttempt(string reason)
    readonly property bool available: Biometryd.available
    property var operation: null

    function start() {
        if (!Biometryd.available || operation)
            return;
        operation = Biometryd.defaultDevice.identifier.identifyUser();
        operation.start(obs);
    }
    function cancel() {
        if (operation) {
            operation.cancel();
            operation = null;
        }
    }

    Observer {
        id: obs
        onSucceeded: {
            root.operation = null;
            root.identified(result);
        }
        onFailed: {
            root.operation = null;
            if (reason !== "ERROR_CANCELED")
                root.failedAttempt(reason);
        }
    }
    Component.onDestruction: cancel()
}
