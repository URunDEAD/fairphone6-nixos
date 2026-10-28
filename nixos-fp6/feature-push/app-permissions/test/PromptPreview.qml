// Offscreen preview + behaviour checks of the Lomiri permission prompt
// (Components/Fp6PermissionPrompt.qml from the patched Lomiri tree).
import QtQuick 2.15
import Lomiri.Components 1.3
import QMenuModel 1.0
import "Components"
import "Shot.js" as Shot
import "Sample.js" as Sample

Item {
    id: shell
    width: units.gu(46.5)
    height: units.gu(103.5)
    property bool locked: Shot.shot === "prompt-locked"
    property string focusedApp: "org.example.Serial"

    Image { anchors.fill: parent; source: Shot.wallpaper; fillMode: Image.PreserveAspectCrop }
    Rectangle { // a stand-in app window
        anchors { fill: parent; topMargin: units.gu(3) }
        color: "#f4f4f4"
        Label { anchors.centerIn: parent; text: "Serial Monitor"; textSize: Label.XLarge; color: "#888" }
    }

    Fp6PermissionPrompt {
        id: prompt
        anchors.fill: parent
        locked: shell.locked
        focusedAppId: shell.focusedApp
    }

    function check(ok, what) { console.log((ok ? "PASS " : "FAIL ") + Shot.shot + ": " + what); }
    function find(item, name) {
        if (item.objectName === name) return item;
        for (var i = 0; i < item.children.length; i++) { var r = find(item.children[i], name); if (r) return r; }
        return null;
    }

    Component.onCompleted: FakeBus.prompt.state = JSON.stringify(Sample.prompt)
    Timer {
        interval: 1200; running: true
        onTriggered: {
            if (Shot.shot === "prompt-locked") {
                check(!prompt.visible, "hidden on the lock screen");
                check(FakeBus.last("ack") === "", "not acknowledged while locked (agent falls back to a notification)");
                check(FakeBus.last("focus") === "focus ", "focus reported as empty while locked");
                shell.locked = false;
                after.start();
                return;
            }
            check(prompt.visible, "visible");
            check(FakeBus.last("ack") === "ack 7", "acknowledged to the agent");
            check(FakeBus.last("focus") === "focus org.example.Serial", "focused app reported");
            var q = find(prompt, "fp6PermissionQuestion");
            check(q && q.text === "Allow Serial Monitor to access a USB device?", "question text");
            shot.start();
        }
    }
    Timer {
        id: after; interval: 600
        onTriggered: {
            check(prompt.visible && FakeBus.last("ack") === "ack 7", "shown and acknowledged after unlock");
            shot.start();
        }
    }
    Timer {
        id: shot; interval: 400
        onTriggered: shell.grabToImage(function(r) {
            r.saveToFile(Shot.out + "/" + Shot.shot + ".png");
            var b = find(prompt, "fp6PermissionWhileUsing");
            b.clicked();
            check(FakeBus.last("respond") === "respond 7 while_using", "answer sent");
            check(!prompt.visible, "closes after answering");
            var before = FakeBus.log.length;
            find(prompt, "fp6PermissionAllow").clicked();
            check(FakeBus.log.length === before, "no second answer for the same prompt");
            shell.focusedApp = "lomiri-clock-app";
            check(FakeBus.last("focus") === "focus lomiri-clock-app", "focus change forwarded");
            Qt.quit();
        })
    }
}
