// Simulated fp6-permission-agent (com.fairphone6.Permissions): "prompt" and
// "state" (JSON, as the agent publishes them), the other actions log what
// the QML asked for.
pragma Singleton
import QtQuick 2.15
QtObject {
    id: bus
    property var log: []
    function note(s) { log = log.concat([s]); console.log("ACTION " + s); }
    function last(prefix) {
        for (var i = log.length - 1; i >= 0; i--) if (log[i].indexOf(prefix) === 0) return log[i];
        return "";
    }
    property QtObject prompt: QtObject { property bool valid: true; property var state: "{}"; function activate() {} }
    property QtObject state: QtObject { property bool valid: true; property var state: "{}"; function activate() {} }
    property QtObject a_ack: QtObject { property bool valid: true; property var state: null; function activate(p) { bus.note("ack " + p); } }
    property QtObject a_respond: QtObject { property bool valid: true; property var state: null; function activate(p) { bus.note("respond " + p); } }
    property QtObject a_focus: QtObject { property bool valid: true; property var state: null; function activate(p) { bus.note("focus " + p); } }
    property QtObject a_set: QtObject { property bool valid: true; property var state: null; function activate(p) { bus.note("set " + p); } }
    property QtObject a_net: QtObject { property bool valid: true; property var state: null; function activate(p) { bus.note("net " + p); } }
    property QtObject a_reset: QtObject { property bool valid: true; property var state: null; function activate(p) { bus.note("reset " + p); } }
    property QtObject a_refresh: QtObject { property bool valid: true; property var state: null; function activate() { bus.note("refresh"); } }
    property QtObject none: QtObject { property bool valid: false; property var state: undefined; function activate() {} }
    function action(busName, name) {
        if (busName !== "com.fairphone6.Permissions") return none;
        switch (name) {
        case "prompt": return prompt;
        case "state": return state;
        case "ack": return a_ack;
        case "respond": return a_respond;
        case "focus": return a_focus;
        case "set": return a_set;
        case "net": return a_net;
        case "reset": return a_reset;
        case "refresh": return a_refresh;
        }
        return none;
    }
}
