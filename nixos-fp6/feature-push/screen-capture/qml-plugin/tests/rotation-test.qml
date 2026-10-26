// SPDX-License-Identifier: GPL-3.0-only
// Landscape check: a 640x360 "shell" drawn rotated by 90 degrees on the
// 360x640 portrait window (as OrientedShell does with transformRotationAngle),
// recorded with start(90). The video must be 640x360 with "TOP" upright at
// the top-left and the red bar along the top edge.
import QtQuick 2.15
import FP6.ScreenRecorder 0.1

Rectangle {
    width: 360; height: 640; color: "black"
    Item {
        id: shell
        width: 640; height: 360
        transform: Rotation { origin.x: 180; origin.y: 180; angle: 90 }
        Rectangle { anchors.fill: parent; color: "blue" }
        Rectangle { width: parent.width; height: 40; color: "red" }
        Text { x: 20; y: 60; text: "TOP"; font.pixelSize: 60; color: "white" }
        Rectangle { id: dot; y: 200; width: 30; height: 30; color: "yellow"
            NumberAnimation on x { from: 0; to: 600; duration: 1000; loops: Animation.Infinite } }
    }
    ScreenRecorder {
        id: recorder
        allowed: true
        onSavingChanged: if (!saving && !recording) Qt.exit(0)
    }
    Timer { interval: 300; running: true; onTriggered: { recorder.start(90); stopper.start() } }
    Timer { id: stopper; interval: 1500; onTriggered: recorder.stop() }
    Timer { interval: 20000; running: true; onTriggered: Qt.exit(4) }
}
