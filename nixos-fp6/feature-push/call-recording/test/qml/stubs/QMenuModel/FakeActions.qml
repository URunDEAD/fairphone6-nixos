// Preview stub: fake fp6-callrec action states, shared by every group.
pragma Singleton
import QtQuick 2.15
QtObject {
    id: fake
    property var states: ({})
    property var objects: ({})
    property var log: []
    function get(name) {
        if (!objects[name]) {
            var o = Qt.createQmlObject('import QtQuick 2.15; QtObject { property string name; property bool valid: false; property var state; ' +
                'function activate(p) { FakeActions.activated(name, p); } }', fake, "FakeAction");
            o.name = name;
            if (states.hasOwnProperty(name)) { o.state = states[name]; o.valid = true; }
            objects[name] = o;
        }
        return objects[name];
    }
    function activated(name, p) {
        var o = objects[name];
        log.push("activate " + name + (p === undefined ? "" : "(" + p + ")"));
        console.log("FakeActions: activate", name, p === undefined ? "" : p);
        if (typeof o.state === "boolean") o.state = !o.state;
    }
}
