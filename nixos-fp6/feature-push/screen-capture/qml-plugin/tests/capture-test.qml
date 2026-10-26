// SPDX-License-Identifier: GPL-3.0-only
// Records 2 s of a moving bar (with a still second in the middle), then
// stops; checks the recorder's state transitions. Exit 0 = ok.
import QtQuick 2.15
import FP6.ScreenRecorder 0.1

Rectangle {
    id: root
    width: 360; height: 640
    gradient: Gradient {
        GradientStop { position: 0; color: "red" }
        GradientStop { position: 1; color: "blue" }
    }
    Rectangle {
        id: bar
        y: 20; width: 40; height: 120; color: "white"
        NumberAnimation on x { id: anim; from: 0; to: 320; duration: 2000; loops: Animation.Infinite; running: false }
    }
    Text { anchors.centerIn: parent; text: "TOP"; font.pixelSize: 60; y: 40; anchors.verticalCenterOffset: -200 }

    ScreenRecorder {
        id: recorder
        allowed: true
        property var log: []
        onRecordingChanged: log.push("recording=" + recording)
        onSavingChanged: {
            log.push("saving=" + saving);
            if (!saving && !recording) { console.log("STATES " + log.join(" ")); Qt.exit(log.join(" ") === "recording=true recording=false saving=true saving=false" ? 0 : 1); }
        }
    }
    Timer {
        interval: 500; running: true
        onTriggered: {
            if (!recorder.start(0)) { console.log("start refused"); Qt.exit(2); }
            anim.running = true; step.start();
        }
    }
    Timer { id: step; interval: 1000; onTriggered: { anim.running = false; still.start(); } }
    Timer { id: still; interval: 1000; onTriggered: { anim.running = true; stopper.start(); } }
    Timer { id: stopper; interval: 1000; onTriggered: recorder.stop() }
    Timer { interval: 20000; running: true; onTriggered: { console.log("timeout"); Qt.exit(4); } }
}
