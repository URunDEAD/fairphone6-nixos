// "Other apps": the Ubuntu Touch content-hub source picker (File Manager,
// Gallery, ...). content-hub copies the chosen items into
// ~/.cache/fp6-file-picker/HubIncoming/<n>/ (APP_ID=fp6-file-picker), and
// those paths are returned to the calling app.
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Content 1.3

Item {
    id: apps
    signal picked(var paths)
    signal canceled()

    property var activeTransfer: null

    ContentPeerPicker {
        id: peerPicker
        anchors.fill: parent
        showTitle: false
        contentType: ContentType.All
        handler: ContentHandler.Source
        onPeerSelected: {
            peer.selectionType = root.multiple ? ContentTransfer.Multiple : ContentTransfer.Single;
            apps.activeTransfer = peer.request();
        }
        onCancelPressed: apps.canceled()
    }

    ContentTransferHint {
        anchors.fill: parent
        activeTransfer: apps.activeTransfer
    }

    Connections {
        target: apps.activeTransfer
        ignoreUnknownSignals: true
        onStateChanged: {
            var t = apps.activeTransfer;
            if (t.state === ContentTransfer.Charged) {
                var paths = [];
                for (var i = 0; i < t.items.length; i++) {
                    var u = String(t.items[i].url);
                    if (u.indexOf("file://") === 0)
                        paths.push(decodeURIComponent(u.substring(7)));
                    else if (u.charAt(0) === "/")
                        paths.push(u);
                }
                if (paths.length > 0)
                    apps.picked(paths);
                else
                    apps.canceled();
            } else if (t.state === ContentTransfer.Aborted) {
                apps.activeTransfer = null;
            }
        }
    }
}
