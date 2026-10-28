// End-to-end helper (test/e2e.sh): loads the real Lomiri.Notifications
// plugin, which owns org.freedesktop.Notifications on the test bus, and
// replies the way the shell does (Model.fp6Reply), either while the bubble
// is shown (FP6_E2E_MODE=bubble) or after it timed out and was parked
// (FP6_E2E_MODE=parked), or never (FP6_E2E_MODE=none: the client times out).
import QtQuick 2.15
import Lomiri.Notifications 1.0 as LN

Item {
    id: root
    property string mode: "bubble"
    property int pending: 0

    Instantiator {
        model: LN.Model
        delegate: QtObject {
            Component.onCompleted: {
                var label = model.hints["x-fp6-inline-reply"];
                console.log("e2e: notification " + model.id + " canReply=" + LN.Model.getRaw(model.id).canReply +
                            " label=" + label + " placeholder=" + model.hints["x-kde-reply-placeholder-text"]);
                if (label === undefined) return;
                root.pending = model.id;
                if (root.mode === "bubble") {
                    var id = model.id;
                    Qt.callLater(function() { console.log("e2e: reply ok=" + LN.Model.fp6Reply(id, "Auto reply ✓ from the test")); });
                }
            }
        }
    }
    Connections {
        target: LN.Model
        function onFp6ReplyableChanged() {
            if (root.mode !== "parked" || root.pending === 0) return;
            // Parked: bubble gone (8 s), still open for the sender.
            if (LN.Model.getRaw(root.pending) === null && LN.Model.fp6CanReply(root.pending)) {
                var id = root.pending;
                root.pending = 0;
                console.log("e2e: parked " + id + ", replying");
                Qt.callLater(function() { console.log("e2e: reply ok=" + LN.Model.fp6Reply(id, "Late reply from the panel")); });
            }
        }
    }
}
