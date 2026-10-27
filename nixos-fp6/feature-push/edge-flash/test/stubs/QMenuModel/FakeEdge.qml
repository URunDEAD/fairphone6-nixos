// Simulated fp6-lomiri-powerd EdgeFlash: "flash" turns "flashing" on after
// one 40 ms "D-Bus round trip"; "stop" or the requested time turns it off.
// log lists what the shell asked for.
pragma Singleton
import QtQuick 2.15
Item {
    id: fake
    property var log: []
    property bool refuse: false
    property int level: 0
    property QtObject flashing: QtObject {
        property bool valid: true
        property var state: false
        function activate() {}
    }
    property QtObject flash: QtObject {
        property bool valid: true
        property var state: undefined
        function activate(p) {
            fake.log = fake.log.concat(["flash " + p]);
            if (fake.refuse) return;
            var parts = String(p).split(":");
            fake.level = parseInt(parts[1]);
            expire.interval = parseInt(parts[0]);
            expire.restart();
            on.restart();
        }
    }
    property QtObject stop: QtObject {
        property bool valid: true
        property var state: undefined
        function activate() {
            fake.log = fake.log.concat(["stop"]);
            expire.stop();
            fake.level = 0;
            fake.flashing.state = false;
        }
    }
    readonly property var actions: ({ "flash": flash, "stop": stop, "flashing": flashing })
    Timer { id: on; interval: 40; onTriggered: fake.flashing.state = true }
    Timer { id: expire; onTriggered: { fake.level = 0; fake.flashing.state = false; fake.log = fake.log.concat(["timeout"]); } }
}
