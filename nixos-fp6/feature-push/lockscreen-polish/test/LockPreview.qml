// Offscreen preview of the FP6 lock-screen fingerprint hint with fake
// biometryd states (test/preview.sh). The cover page, clock and passcode
// pad are simple mock-ups; the hint (Fp6FingerprintHint.qml) and the
// wallpaper (Wallpaper.qml + Fp6WallpaperEffects.qml) are the real files
// taken from the patches.
import QtQuick 2.15
import Lomiri.Components 1.3
import "Shot.js" as Shot

Item {
    id: root
    width: 1116
    height: 2484

    readonly property string scene: Shot.shot
    property real coverX: 0
    readonly property real coverProgress: Math.max(0, Math.min(1, 1 - Math.abs(coverX) / width))
    property bool home: scene.indexOf("home") === 0

    // --- home screen (wallpaper effects) --------------------------------
    Item {
        anchors.fill: parent
        visible: root.home || root.scene === "success"
        Wallpaper {
            anchors.fill: parent
            source: "file://" + Shot.homeWallpaper
            sourceSize: 2484
            fp6Blur: root.scene.indexOf("blur") >= 0 ? 1 : 0
            fp6Dim: root.scene.indexOf("dim") >= 0 ? 0.3 : 0
        }
        Rectangle {   // launcher strip mock
            anchors { left: parent.left; top: parent.top; bottom: parent.bottom; topMargin: units.gu(3) }
            width: units.gu(8); color: "#66000000"
            Column {
                anchors.horizontalCenter: parent.horizontalCenter; y: units.gu(2); spacing: units.gu(2)
                Repeater { model: 7; Rectangle { width: units.gu(6); height: width; radius: units.gu(1.4); color: Qt.hsla(index / 7, 0.45, 0.6, 1) } }
            }
        }
        Rectangle { width: parent.width; height: units.gu(3); color: "black"; opacity: 0.35 }
    }

    // --- lock screen (cover page) ---------------------------------------
    Item {
        id: lockLayer
        anchors.fill: parent
        visible: !root.home

        // Passcode screen behind the cover (a match unlocks: no passcode).
        Item {
            anchors.fill: parent
            visible: root.scene !== "success"
            Wallpaper {
                anchors.fill: parent
                source: "file://" + Shot.wallpaper
                sourceSize: 2484
            }
            Rectangle { anchors.fill: parent; color: "black"; opacity: 0.6 }
            Column {
                anchors.horizontalCenter: parent.horizontalCenter
                y: parent.height * 0.22
                spacing: units.gu(3)
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: hint.message !== "" && root.coverProgress < 0.5 ? hint.message : "Enter passcode"
                    color: "white"; fontSize: "large"
                }
                Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: units.gu(2)
                    Repeater { model: 4; Rectangle { width: units.gu(1.5); height: width; radius: width / 2; color: "transparent"; border.color: "white"; border.width: 2 } }
                }
                Grid {
                    anchors.horizontalCenter: parent.horizontalCenter
                    columns: 3; spacing: units.gu(4)
                    Repeater {
                        model: [ "1","2","3","4","5","6","7","8","9","","0","⌫" ]
                        Label { width: units.gu(6); height: units.gu(6); text: modelData; color: "white"; fontSize: "x-large"
                                horizontalAlignment: Text.AlignHCenter; verticalAlignment: Text.AlignVCenter }
                    }
                }
            }
            Label { anchors { left: parent.left; bottom: parent.bottom; margins: units.gu(2) } text: "Cancel"; color: "#cccccc"; fontSize: "small" }
            Label { anchors { right: parent.right; bottom: parent.bottom; margins: units.gu(2) } text: "Emergency"; color: "#cccccc"; fontSize: "small" }
        }
        Rectangle { anchors.fill: parent; color: "black"; opacity: root.coverProgress * 0.8; visible: root.scene !== "success" }

        // Cover page.
        Item {
            id: cover
            width: parent.width; height: parent.height
            x: root.coverX
            Wallpaper {
                anchors.fill: parent
                source: "file://" + Shot.wallpaper
                sourceSize: 2484
            }
            Rectangle { anchors.fill: parent; color: "black"; opacity: 0.4 }
            Column {
                anchors.horizontalCenter: parent.horizontalCenter
                y: units.gu(2) + units.gu(3)
                Label { anchors.horizontalCenter: parent.horizontalCenter; text: "12:30"; color: "white"; font.weight: Font.Light; font.pixelSize: units.gu(10) }
                Label { anchors.horizontalCenter: parent.horizontalCenter; text: "Tuesday, 27 October"; color: "white"; font.weight: Font.Light; fontSize: "large" }
            }
            Label {
                anchors.horizontalCenter: parent.horizontalCenter
                anchors.bottom: parent.bottom; anchors.bottomMargin: units.gu(5)
                text: "Fully charged"; color: "white"; font.weight: Font.Light
            }
        }
        // Status bar mock.
        Rectangle {
            width: parent.width; height: units.gu(3); color: "black"; opacity: 0.35
            Label { anchors { right: parent.right; rightMargin: units.gu(2); verticalCenter: parent.verticalCenter } text: "▾  ▮ 84%"; color: "white"; fontSize: "small" }
        }
    }

    Fp6FingerprintHint {
        id: hint
        anchors.fill: parent
        sensorCenter: 1100 / 2484
        sensorLength: 0.075
        coverProgress: root.coverProgress
        quiet: root.scene === "quiet"
        textCue: true
        onPasscodeRequested: slide.restart()
    }

    // Sensor reference mark (outside the hint): where the power key is.
    Rectangle {
        visible: Shot.marker
        x: parent.width - 6; y: 1100 - height / 2
        width: 6; height: 186; color: "#ff00ff"; opacity: 0.6
    }

    NumberAnimation { id: slide; target: root; property: "coverX"; to: -root.width; duration: 300; easing.type: Easing.InOutQuad }

    // --- script ----------------------------------------------------------
    property var steps: ({
        "appear":   [ [0, function() { hint.available = true; }] ],
        "rest":     [ [0, function() { hint.available = true; }] ],
        "reading":  [ [0, function() { hint.textCue = false; hint.available = true; }], [2600, function() { hint.reading(); }] ],
        "fail":     [ [0, function() { hint.textCue = false; hint.available = true; }], [2600, function() { hint.failed("nomatch"); }] ],
        "busy":     [ [0, function() { hint.textCue = false; hint.available = true; }], [2600, function() { hint.failed("busy"); }] ],
        "lockout":  [ [0, function() { hint.textCue = false; hint.available = true; }],
                      [2600, function() { hint.lockedOut = true; hint.available = false; hint.failed("lockout"); }] ],
        "success":  [ [0, function() { hint.textCue = false; hint.available = true; }],
                      [2600, function() { hint.succeeded(); slide.restart(); hint.available = false; }] ],
        "passcode": [ [0, function() { root.coverX = -root.width; hint.textCue = false; hint.available = true; }],
                      [2600, function() { hint.failed("nomatch"); }] ],
        "quiet":    [ [0, function() { hint.available = true; }] ],
        "screenoff":[ [0, function() { hint.available = true; }], [800, function() { hint.available = false; }] ],
        "home":     [], "home-blur": [], "home-dim": [], "home-blur-dim": []
    })
    property var shots: Shot.shots     // [[ms, name], ...]
    property real frameFrom: Shot.frameFrom
    property real frameTo: Shot.frameTo
    property int frameStep: Shot.frameStep
    property int stepIndex: 0
    property int shotIndex: 0
    property int frameNo: 0
    property double nextFrame: Shot.frameFrom
    property int pending: 0

    function grab(path, small) {
        pending++;
        root.grabToImage(function(r) { r.saveToFile(path); pending--; },
                         small ? Qt.size(width / 3, height / 3) : Qt.size(width, height));
    }

    // The script runs on the animation clock (not wall time), so stills and
    // frames show the same animation state however slow a grab is.
    property real at: 0
    NumberAnimation on at { from: 0; to: 120000; duration: 120000; running: true }
    onAtChanged: {
        var t = root.at - 1200;   // wallpaper load warm-up
        if (t < 0) return;
        var s = root.steps[root.scene] || [];
        while (root.stepIndex < s.length && s[root.stepIndex][0] <= t) { s[root.stepIndex][1](); root.stepIndex++; }
        while (root.shotIndex < root.shots.length && root.shots[root.shotIndex][0] <= t) {
            root.grab(Shot.out + "/" + root.scene + "-" + root.shots[root.shotIndex][1] + ".png"); root.shotIndex++;
        }
        if (root.frameStep > 0 && t >= root.nextFrame && t <= root.frameTo) {
            var n = ("000" + root.frameNo).slice(-3);
            root.grab(Shot.out + "/frames/" + root.scene + "/" + n + ".png", true);
            root.frameNo++; root.nextFrame += root.frameStep;
        }
        var end = Math.max(root.frameTo, root.shots.length ? root.shots[root.shots.length - 1][0] : 0) + 400;
        if (t > end && root.pending === 0) Qt.quit();
    }
}
