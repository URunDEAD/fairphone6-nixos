// Offscreen previews + behaviour checks of FP6 charging feedback (1116x2484,
// 24 px/gu like the FP6). The charging animation, model, ring and the
// lock-screen line are the real files (from the patched Lomiri tree); the
// lock screen (wallpaper, clock, status bar, Lomiri's own bottom hint) and
// the home screen are mock-ups with the stock sizes, the fingerprint hint is
// the real lockscreen-polish file. fp6-charging-feedback, the sound
// indicator, Powerd and NotificationAudio are stubs (test/stubs).
// Shot.js: shot (scene), out (directory), wallpaper.
import QtQuick 2.15
import GSettings 1.0
import Lomiri.Components 1.3
import Powerd 0.1
import QMenuModel 1.0
import "Components"
import "Greeter"
import "Shot.js" as Shot

Item {
    id: root
    width: 1116
    height: 2484

    readonly property string scene: Shot.shot
    property bool locked: scene.indexOf("home") < 0
    property int failures: 0
    function check(cond, what) {
        console.log((cond ? "PASS " : "FAIL ") + scene + ": " + what);
        if (!cond) failures += 1;
    }

    GSettings { id: cf; schema.id: "com.fairphone6.charging-feedback" }
    GSettings { id: dnd; schema.id: "com.fairphone6.notifications" }

    // ---- home screen mock (unlocked scenes) ------------------------------------
    Item {
        anchors.fill: parent
        visible: !root.locked
        Rectangle {
            anchors.fill: parent
            gradient: Gradient {
                GradientStop { position: 0; color: "#33415e" }
                GradientStop { position: 1; color: "#7a4b5c" }
            }
        }
        Grid {
            anchors.horizontalCenter: parent.horizontalCenter
            y: units.gu(8)
            columns: 4; spacing: units.gu(4)
            Repeater {
                model: 20
                Rectangle { width: units.gu(7); height: width; radius: units.gu(1.6); color: Qt.hsla(index / 20, 0.45, 0.62, 1) }
            }
        }
    }

    // ---- lock screen mock (stock GreeterView/CoverPage sizes) -------------------
    Item {
        id: cover
        anchors.fill: parent
        visible: root.locked
        Image {
            anchors.fill: parent
            source: "file://" + Shot.wallpaper
            sourceSize.height: 2484
            fillMode: Image.PreserveAspectCrop
        }
        Rectangle { anchors.fill: parent; color: "black"; opacity: 0.4 }   // backgroundShadeOpacity
        // Clock.qml: time 7.5 gu Light, date "medium", 0.5 gu apart; portrait
        // state: top = panelHeight + 2 gu.
        Column {
            id: clock
            anchors.horizontalCenter: parent.horizontalCenter
            y: units.gu(3) + units.gu(2)
            spacing: units.gu(0.5)
            Label { anchors.horizontalCenter: parent.horizontalCenter; text: "12:47"; color: "white"; font.weight: Font.Light; font.pixelSize: units.gu(7.5) }
            Label { anchors.horizontalCenter: parent.horizontalCenter; text: "Wednesday, 28 October 2026"; color: "white"; font.weight: Font.Light; fontSize: "medium" }
        }
        // As in the patched GreeterView.qml.
        Fp6ChargingStatus {
            id: status
            anchors.top: clock.bottom
            anchors.topMargin: units.gu(2)
            anchors.horizontalCenter: clock.horizontalCenter
            width: Math.min(cover.width - units.gu(4), units.gu(60))
            animate: true
        }
        // CoverPage.qml chargingHint (Lomiri's own), hidden while ours shows.
        Label {
            id: stockHint
            anchors.horizontalCenter: parent.horizontalCenter
            anchors.bottom: parent.bottom
            anchors.bottomMargin: units.gu(5)
            visible: !status.shown && status.model.charging
            text: "1 hour, 10 minutes until full"
            color: "white"
            font.weight: Font.Light
        }
    }

    // Status bar mock with the suru icon ayatana-indicator-power picks
    // (battery-NNN-charging while charging, gpm-battery-100 when full).
    Rectangle {
        width: parent.width; height: units.gu(3); color: "black"; opacity: 0.35
    }
    Row {
        anchors { right: parent.right; rightMargin: units.gu(1.5) }
        height: units.gu(3)
        spacing: units.gu(1)
        readonly property var m: status.model
        Icon {
            anchors.verticalCenter: parent.verticalCenter
            height: units.gu(2.2); width: height
            color: "white"
            readonly property string idx: ("00" + Math.round(Math.max(0, parent.m.level) / 10) * 10).slice(-3)
            name: parent.m.charging ? "battery-" + idx + "-charging"
                : parent.m.status === "full" ? "gpm-battery-100" : "battery-" + idx
        }
        Label { anchors.verticalCenter: parent.verticalCenter; text: Math.max(0, parent.m.level) + "%"; color: "white"; fontSize: "small" }
        Label { anchors.verticalCenter: parent.verticalCenter; text: "12:47"; color: "white"; fontSize: "small" }
    }

    Fp6FingerprintHint {
        id: hint
        anchors.fill: parent
        visible: root.scene === "lock-fingerprint"
        sensorCenter: 1100 / 2484
        sensorLength: 0.075
        coverProgress: 1
        textCue: true
    }

    Fp6ChargingFeedback {
        id: fb
        anchors.fill: parent
        locked: root.locked
        panelHeight: units.gu(3)
    }

    // ---- capture -------------------------------------------------------------------
    property int frame: 0
    property bool recording: false
    Timer {
        interval: 66; repeat: true; running: root.recording
        onTriggered: {
            var n = root.frame++;
            var name = Shot.out + "/frames/" + root.scene + "-" + ("000" + n).slice(-3) + ".png";
            root.grabToImage(function(r) { r.saveToFile(name); }, Qt.size(279, 621));
        }
    }
    function shot(name) {
        var f = Shot.out + "/" + root.scene + (name ? "-" + name : "") + ".png";
        root.grabToImage(function(r) { r.saveToFile(f); });
    }

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
        console.log("RESULT " + scene + " failures=" + failures + " log=" + JSON.stringify(FakeBus.log));
        quitTimer.start();
    }
    Timer { id: quitTimer; interval: 400; onTriggered: Qt.quit() }
    function sounds() { return FakeBus.log.filter(function(x) { return x.indexOf("sound") === 0; }); }
    function wakes() { return FakeBus.log.filter(function(x) { return x.indexOf("wake") === 0; }); }

    // A lock-screen state: the agent's reading, then one still.
    function still(o, extra) {
        run([
            [300, function() { FakeBus.set(o); }],
            [900, function() {
                if (extra) extra();
                shot("");
            }],
            [100, done]
        ]);
    }

    Component.onCompleted: {
        Powerd.status = Powerd.On;
        var S = scene;
        // Agent up, unplugged, before anything happens (primes the event counter).
        FakeBus.set({ plugged: false, source: "", status: "discharging", speed: "", ttf: -1, level: 45, charger: "" });
        var at0630 = Math.round(new Date(2026, 9, 29, 6, 30).getTime() / 1000);
        if (S === "lock-charging") still({ level: 45, ttf: 4200 }, function() {
            check(status.shown, "line shown");
            check(status.model.lockLine === "Charging · 45 % · Full in 1 h 10 min", status.model.lockLine);
            check(!stockHint.visible, "Lomiri's own hint hidden");
        });
        else if (S === "lock-fast") still({ level: 62, speed: "fast", ttf: 2100, charger: "USB PD", chargeMw: 18500 }, function() {
            check(status.model.lockLine === "Fast charging · 62 % · Full in 35 min", status.model.lockLine);
        });
        else if (S === "lock-slow") still({ level: 30, speed: "slow", ttf: 12000, charger: "USB port", chargeMw: 2400 }, function() {
            check(status.model.lockLine === "Charging slowly · 30 % · Full in 3 h 20 min", status.model.lockLine);
        });
        else if (S === "lock-limited") still({ level: 72, ttf: 720, target: 80, limited: true }, function() {
            check(status.model.lockLine === "Charging · 72 % · 80 % in 12 min", status.model.lockLine);
            check(status.model.lockNote === "Limited to 80 % to protect the battery", status.model.lockNote);
        });
        else if (S === "lock-paused") still({ level: 80, status: "paused", ttf: -1, target: 80, limited: true }, function() {
            check(status.model.lockLine === "Charging paused at 80 % to protect the battery", status.model.lockLine);
        });
        else if (S === "lock-adaptive") still({ level: 80, status: "paused", ttf: -1, target: 80, limited: true, mode: "hold", holdUntil: at0630 }, function() {
            check(status.model.lockLine === "Charging paused at 80 %", status.model.lockLine);
            check(status.model.lockNote.indexOf("Adaptive charging · full by ") === 0, status.model.lockNote);
        });
        else if (S === "lock-full") still({ level: 100, status: "full", ttf: -1 }, function() {
            check(status.model.lockLine === "Charged · 100 %", status.model.lockLine);
        });
        else if (S === "lock-wireless") still({ level: 50, source: "wireless", charger: "Wireless", ttf: 6000 }, function() {
            check(status.model.lockLine === "Charging wirelessly · 50 % · Full in 1 h 40 min", status.model.lockLine);
        });
        else if (S === "lock-fingerprint") {
            run([
                [300, function() { FakeBus.set({ level: 45, ttf: 4200 }); hint.available = true; }],
                [1000, function() { shot(""); check(status.shown && hint.message !== "", "line and the fingerprint cue together: " + hint.message); }],
                [100, done]
            ]);
        } else if (S === "lock-off") {
            cf.lockscreenInfo = false;
            still({ level: 45, ttf: 4200 }, function() {
                check(!status.shown && stockHint.visible, "setting off: no line, Lomiri's own hint back");
            });
        } else if (S === "lock-unplugged") {
            still({ plugged: false, source: "", status: "discharging", speed: "", ttf: -1, level: 45 }, function() {
                check(!status.shown && !fb.visible, "unplugged: nothing");
            });
        } else if (S === "anim-locked") {
            run([
                [500, function() { recording = true; FakeBus.plug({ level: 64, speed: "fast", ttf: 1900, charger: "USB PD" }); }],
                [50, function() {
                    check(fb.phase === "full", "locked + display on: full-screen animation (" + fb.phase + ")");
                    check(sounds().length === 1 && sounds()[0] === "sound power-plug.oga", "plug sound: " + sounds());
                    check(wakes().length === 0, "display already on: no wake");
                }],
                [300, function() { shot("1-in"); }],
                [600, function() { shot("2-fill"); }],
                [800, function() { shot("3-full"); check(fb.vTitle === "Fast charging" && fb.vDetail === "Full in 32 min", fb.vTitle + " / " + fb.vDetail); }],
                [900, function() { shot("4-out"); }],
                [800, function() { check(fb.phase === "idle" && !fb.visible, "gone after ~3.2 s"); shot("5-after"); }],
                [100, done]
            ]);
        } else if (S === "anim-limited") {
            run([
                [500, function() { FakeBus.plug({ level: 58, ttf: 1500, target: 80, limited: true }); }],
                [1600, function() { shot(""); check(fb.vDetail === "Limited to 80 % · 25 min left", fb.vDetail); }],
                [2000, done]
            ]);
        } else if (S === "anim-slow") {
            run([
                [500, function() { FakeBus.plug({ level: 23, speed: "slow", ttf: 15000, charger: "USB port" }); }],
                [1600, function() { shot(""); check(fb.vTitle === "Charging slowly", fb.vTitle); }],
                [2000, done]
            ]);
        } else if (S === "anim-screenoff") {
            Powerd.status = Powerd.Off;
            run([
                [500, function() { FakeBus.plug({ level: 37, ttf: 5000 }); }],
                [50, function() {
                    check(fb.phase === "waiting" && !fb.visible, "display off: waiting for the wake (" + fb.phase + ")");
                    check(wakes().length === 1 && wakes()[0] === "wake 7", "wake requested: " + wakes());
                }],
                [250, function() { Powerd.status = Powerd.On; }],   // Peek: display on
                [20, function() { check(fb.phase === "full", "animation starts with the display on"); }],
                [1500, function() { shot(""); }],
                [2000, done]
            ]);
        } else if (S === "anim-refused") {
            Powerd.status = Powerd.Off;
            run([
                [500, function() { FakeBus.plug({ level: 37 }); }],
                [1700, function() { check(fb.phase === "idle" && !fb.visible, "powerd refused (pocket/call): nothing shown"); check(sounds().length === 1, "sound still played"); }],
                [100, done]
            ]);
        } else if (S === "anim-nowake") {
            Powerd.status = Powerd.Off;
            cf.wakeScreen = false;
            run([
                [500, function() { FakeBus.plug({ level: 37 }); }],
                [100, function() { check(wakes().length === 0 && fb.phase === "idle", "'turn the screen on' off: no wake"); check(sounds().length === 1, "sound played"); }],
                [100, done]
            ]);
        } else if (S === "anim-dnd") {
            Powerd.status = Powerd.Off;
            dnd.dndActive = true;
            run([
                [500, function() { FakeBus.plug({ level: 37 }); }],
                [100, function() {
                    check(wakes().length === 0 && fb.phase === "idle", "DND (no wake): display stays dark");
                    check(sounds().length === 0, "DND: no sound");
                }],
                [100, done]
            ]);
        } else if (S === "anim-silent") {
            FakeBus.silent.state = true;
            run([
                [500, function() { FakeBus.plug({ level: 37 }); }],
                [100, function() { check(sounds().length === 0, "silent mode: no sound"); check(fb.phase === "full", "animation still shown"); }],
                [100, done]
            ]);
        } else if (S === "anim-off") {
            cf.animation = false;
            cf.sound = false;
            run([
                [500, function() { FakeBus.plug({ level: 37 }); }],
                [100, function() { check(fb.phase === "idle" && sounds().length === 0, "animation and sound off: nothing"); }],
                [100, done]
            ]);
        } else if (S === "anim-unplug") {
            run([
                [500, function() { FakeBus.plug({ level: 37 }); }],
                [700, function() { FakeBus.unplug(); }],
                [50, function() { check(sounds().length === 2 && sounds()[1] === "sound power-unplug.oga", "unplug sound: " + sounds()); }],
                [500, function() { check(fb.phase === "idle", "unplugged during the animation: faded out at once"); }],
                [100, done]
            ]);
        } else if (S === "anim-bounce") {
            run([
                [500, function() { FakeBus.plug({ level: 37 }); }],
                [100, function() { FakeBus.set({ level: 38 }); }],   // a new reading, same event
                [100, function() { check(sounds().length === 1, "readings during the animation are not new events"); }],
                [100, done]
            ]);
        } else if (S === "home-banner") {
            run([
                [500, function() { recording = true; FakeBus.plug({ level: 81, ttf: 1500, speed: "fast", charger: "USB PD" }); }],
                [50, function() { check(fb.phase === "compact", "unlocked: banner (" + fb.phase + ")"); }],
                [700, function() { shot(""); }],
                [2600, function() { check(fb.phase === "idle", "banner gone"); }],
                [100, done]
            ]);
        } else if (S === "home-usbhost") {
            run([
                [500, function() { FakeBus.plug({ level: 81, usbHost: true, charger: "USB port" }); }],
                [50, function() {
                    check(fb.phase === "idle", "computer connected: the USB notification is the pop-up, no banner");
                    check(sounds().length === 1, "sound played once");
                }],
                [100, done]
            ]);
        } else if (S === "home-usbhost-late") {
            run([
                [500, function() { FakeBus.plug({ level: 81, charger: "USB port" }); }],
                [300, function() { check(fb.phase === "compact", "banner first"); FakeBus.set({ level: 81, usbHost: true }); }],
                [600, function() { check(fb.phase === "idle", "computer recognised a moment later: banner leaves (USB notification shows)"); }],
                [100, done]
            ]);
        } else if (S === "preview") {
            FakeBus.set({ plugged: false, source: "", status: "discharging", speed: "", ttf: -1, level: 52 });
            run([
                [300, function() { cf.previewRequest = 1; }],
                [1500, function() { shot(""); check(fb.phase === "full" && fb.vTitle === "Charging" && fb.vDetail === "Preview", "Settings preview while unplugged"); }],
                [2000, done]
            ]);
        } else {
            console.log("FAIL unknown scene " + S);
            failures += 1;
            done();
        }
    }
}
