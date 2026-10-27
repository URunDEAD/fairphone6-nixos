// Offscreen preview + behaviour checks of Fp6EdgeFlash.qml (the real file,
// placeholders substituted) over a stand-in lock screen, 1116x2484 like the
// FP6 panel. Powerd and the EdgeFlash D-Bus actions are stubs (test/stubs).
// Shot.js: shot (scene), out (directory), icons.
import QtQuick 2.15
import GSettings 1.0
import Powerd 0.1
import QMenuModel 1.0
import "Shot.js" as Shot

Item {
    id: root
    width: 1116
    height: 2484

    property int failures: 0
    function check(cond, what) {
        console.log((cond ? "PASS " : "FAIL ") + Shot.shot + ": " + what);
        if (!cond) failures += 1;
    }

    // What Lomiri shows behind (lock screen); the glow must cover it all.
    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            GradientStop { position: 0; color: "#2b3a67" }
            GradientStop { position: 1; color: "#b85c38" }
        }
        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            y: parent.height * 0.18
            text: "12:47"
            color: "white"
            font.pixelSize: 260
        }
    }

    GSettings { id: gs; schema.id: "com.fairphone6.edge-flash" }
    GSettings { id: battery; schema.id: "com.fairphone6.battery" }
    GSettings { id: dnd; schema.id: "com.fairphone6.notifications" }

    Fp6EdgeFlash {
        id: edge
        anchors.fill: parent
    }

    // ---- frame capture (quarter size) ---------------------------------------
    property int frame: 0
    property bool recording: false
    Timer {
        interval: 66; repeat: true; running: root.recording
        onTriggered: {
            var n = root.frame++;
            var name = Shot.out + "/frames/" + Shot.shot + "-" + ("000" + n).slice(-3) + ".png";
            root.grabToImage(function(r) { r.saveToFile(name); }, Qt.size(279, 621));
        }
    }
    function shot(name) {
        root.grabToImage(function(r) { r.saveToFile(Shot.out + "/" + Shot.shot + "-" + name + ".png"); });
    }

    // ---- scenario: list of [delay ms, function] ------------------------------
    property var steps: []
    property int stepIndex: 0
    Timer {
        id: stepper
        onTriggered: {
            var s = root.steps[root.stepIndex++];
            s[1]();
            if (root.stepIndex < root.steps.length) {
                interval = root.steps[root.stepIndex][0];
                restart();
            }
        }
    }
    function run(list) {
        steps = list;
        stepIndex = 0;
        stepper.interval = list[0][0];
        stepper.start();
    }
    function done() {
        recording = false;
        console.log("RESULT " + Shot.shot + " failures=" + failures + " log=" + JSON.stringify(FakeEdge.log));
        quitTimer.start();
    }
    Timer { id: quitTimer; interval: 400; onTriggered: Qt.quit() }

    function offerChat() {
        return edge.offer({ "x-fp6-app-name": "teleports.ubports" }, true, Shot.chatIcon, false);
    }

    Component.onCompleted: {
        gs.enabled = true;
        gs.colorMode = "app";
        gs.brightness = 16;
        gs.durationMs = 2600;
        Powerd.status = Powerd.Off;
        var S = Shot.shot;
        if (S === "app") {
            run([
                [500, function() {
                    recording = true;
                    check(offerChat(), "offer accepted with the display off");
                    check(edge.phase === "arming" && edge.strength === 0, "arming: black frame, no glow yet");
                    shot("0-arming");
                }],
                [200, function() {
                    check(edge.phase === "lit" && FakeEdge.log[0] === "flash 3200:16", "backlight requested after the black frame: " + FakeEdge.log[0]);
                }],
                [250, function() { shot("1-in"); }],
                [600, function() {
                    shot("2-full");
                    check(Math.abs(edge.tint.g - 1) < 0.01 && edge.tint.r < 0.4 && edge.tint.b < 0.6,
                          "tint sampled from the green chat icon: " + edge.tint);
                }],
                [700, function() { shot("3-sweep"); }],
                [800, function() { shot("4-fade"); check(edge.phase === "fading" || edge.phase === "idle", "fading near the end (" + edge.phase + ")"); }],
                [1000, function() {
                    // "stop" normally; "timeout" (powerd's safety timer) when the
                    // offscreen recording starves the main loop.
                    check(edge.phase === "idle" && FakeEdge.flashing.state === false
                          && (FakeEdge.log.indexOf("stop") >= 0 || FakeEdge.log.indexOf("timeout") >= 0),
                          "after the glow: dark again, overlay hidden (" + FakeEdge.log[FakeEdge.log.length - 1] + ")");
                    check(edge.unseen === 1, "one unseen notification");
                    shot("5-after");
                }],
                [100, done]
            ]);
        } else if (S === "fixed") {
            gs.colorMode = "fixed";
            gs.color = "#a46cf0";
            run([
                [500, function() { check(offerChat(), "offer accepted"); }],
                [1100, function() { shot("full"); check(String(edge.tint) === "#a46cf0", "fixed colour " + edge.tint); }],
                [2400, done]
            ]);
        } else if (S === "hash") {
            run([
                [500, function() { check(edge.offer({ "x-fp6-app-name": "org.example.app" }, true, "", false), "offer without icon"); }],
                [1100, function() { shot("full"); check(edge.tintPalette.indexOf(String(edge.tint)) >= 0, "per-app palette colour " + edge.tint); }],
                [2400, done]
            ]);
        } else if (S === "policy") {
            // GSettings changes reach the other GSettings object on the next
            // main-loop turn: every write gets its own step.
            run([
                [300, function() {
                    check(!edge.offer({}, false, "", false), "snap decision / confirmation: no flash (wakes)");
                    check(!edge.offer({}, true, "", true), "critical: no flash (wakes)");
                    gs.onlyCallsMessages = true;
                }],
                [100, function() {
                    check(!edge.offer({ "x-fp6-app-name": "org.example.app" }, true, "", false), "only calls/messages: other app wakes");
                    battery.saverActive = true;
                }],
                [100, function() {
                    check(!edge.offer({ "x-fp6-dnd": "allowed" }, true, "", false), "battery saver: no flash");
                    gs.inBatterySaver = true;
                }],
                [100, function() {
                    check(edge.offer({ "x-fp6-dnd": "allowed" }, true, "", false), "SMS with 'also in battery saver'");
                }],
                [3400, function() {
                    check(edge.offer({ "x-fp6-app-name": "teleports.ubports" }, true, "", false), "chat app in message-apps flashes");
                    battery.saverActive = false;
                    Powerd.status = Powerd.On;
                    check(edge.unseen === 0 && edge.phase === "idle", "display on: seen, overlay gone");
                    check(!edge.offer({ "x-fp6-dnd": "allowed" }, true, "", false), "display on: no flash");
                    Powerd.status = Powerd.Off;
                    gs.enabled = false;
                }],
                [100, function() {
                    check(!edge.offer({ "x-fp6-dnd": "allowed" }, true, "", false), "setting off: no flash");
                }],
                [100, done]
            ]);
        } else if (S === "refused") {
            FakeEdge.refuse = true;
            run([
                [300, function() { check(offerChat(), "offer accepted (powerd decides)"); }],
                [1300, function() {
                    check(edge.phase === "idle" && !edge.visible, "powerd refused (pocket/face down): overlay gone, display stays dark");
                    shot("after");
                }],
                [100, done]
            ]);
        } else if (S === "wake") {
            run([
                [300, function() { offerChat(); }],
                [700, function() {
                    check(edge.phase === "lit", "glowing");
                    Powerd.status = Powerd.On;
                    check(edge.phase === "idle" && !edge.visible, "power key during the glow: overlay gone at once");
                    shot("after");
                }],
                [100, done]
            ]);
        } else if (S === "again") {
            run([
                [300, function() { offerChat(); }],
                [1500, function() {
                    var before = FakeEdge.log.length;
                    check(edge.offer({ "x-fp6-app-name": "mail" }, true, Shot.mailIcon, false), "second notification while lit");
                    check(edge.elapsed === 0 && FakeEdge.log.length === before + 1, "glow restarts, backlight extended");
                }],
                [900, function() { shot("second"); }],
                [2400, function() { check(edge.unseen === 2 && edge.phase === "idle", "two unseen"); }],
                [100, done]
            ]);
        } else if (S === "preview") {
            Powerd.status = Powerd.On;
            run([
                [300, function() { gs.previewRequest = gs.previewRequest + 1; }],
                [100, function() { check(edge.phase === "preview" && FakeEdge.log.length === 0, "Settings preview: glow without powerd"); }],
                [800, function() { shot("full"); }],
                [2400, function() { check(edge.phase === "idle", "preview ends"); }],
                [100, done]
            ]);
        }
    }
}
