// Simulated fp6-charging-feedback agent (state JSON, wake) and the sound
// indicator's silent-mode. log lists what the shell asked for / played.
pragma Singleton
import QtQuick 2.15
QtObject {
    id: bus
    property var log: []
    function note(s) { log = log.concat([s]); }
    property int seq: 0
    property QtObject agentState: QtObject { property bool valid: true; property var state: "{}"; function activate() {} }
    property QtObject wake: QtObject { property bool valid: true; property var state: undefined; function activate(p) { bus.note("wake " + p); } }
    property QtObject silent: QtObject { property bool valid: true; property var state: false; function activate() {} }
    property QtObject none: QtObject { property bool valid: false; property var state: undefined; function activate() {} }
    function action(busName, name) {
        if (busName === "com.fairphone6.ChargingFeedback") return name === "state" ? agentState : name === "wake" ? wake : none;
        if (busName === "org.ayatana.indicator.sound" && name === "silent-mode") return silent;
        return none;
    }
    property var current: ({})
    // A reading like fp6-charging-feedback's (model::compose), with overrides.
    function set(o) {
        var d = { plugged: true, source: "usb", status: "charging", level: 45, speed: "normal", chargeMw: 8000,
                  inputMw: 9000, charger: "USB charger", ttf: 4200, target: 100, limited: false, mode: "normal",
                  holdUntil: 0, usbHost: false, seq: seq, event: current.event || "" };
        for (var k in o) d[k] = o[k];
        current = d;
        agentState.state = JSON.stringify(d);
    }
    function plug(o) { seq += 1; var x = o || {}; x.seq = seq; x.event = "plug"; set(x); }
    function unplug() { seq += 1; set({ seq: seq, event: "unplug", plugged: false, source: "", status: "discharging", speed: "", ttf: -1, charger: "" }); }
}
