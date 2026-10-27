// Preview stub: fake indicator action states, "bus/action" -> state.
pragma Singleton
import QtQuick 2.15
QtObject {
    id: fake
    property var states: ({})
    property var objects: ({})
    property var log: []
    function get(bus, name) {
        var key = bus + "/" + name;
        if (!objects[key]) {
            var o = Qt.createQmlObject('import QtQuick 2.15; import QMenuModel 1.0; QtObject { property string key; property bool valid: false; property var state; ' +
                'function activate(p) { FakeActions.activated(key, p); } function updateState(v) { state = v; FakeActions.log.push(key + "=" + v); } }',
                fake, "FakeAction");
            o.key = key;
            if (states.hasOwnProperty(key)) { o.state = states[key]; o.valid = true; }
            objects[key] = o;
        }
        return objects[key];
    }
    function activated(key, p) {
        var o = objects[key];
        log.push("activate " + key + (p === undefined ? "" : "(" + p + ")"));
        if (typeof o.state === "boolean") o.state = (p === undefined) ? !o.state : !!p;
    }
}
