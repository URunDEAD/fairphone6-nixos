/*
 * FP6 charging feedback: the lock-screen charging line under the clock and
 * date, for as long as a charger is connected (whatever woke the display):
 *   [ring] Charging · 45 % · Full in 1 h 10 min
 *          Limited to 80 % to protect the battery      (charge limit / adaptive)
 * or "Charged · 100 %", "Charging paused at 80 % to protect the battery".
 * Live: fp6-charging-feedback re-reads every 10 s while plugged in. The
 * small ring shows the level and breathes slowly while charging (stepped at
 * 12.5 Hz, only with the display on and the cover page shown).
 * Setting: com.fairphone6.charging-feedback lockscreen-info (default on).
 * Replaces Lomiri's own "… until full" hint at the bottom (CoverPage.qml),
 * which then gives the space back to the swipe hint.
 *
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15
import GSettings 1.0
import Lomiri.Components 1.3
import Powerd 0.1
import "../Components"

Item {
    id: cs
    objectName: "fp6ChargingStatus"

    // GreeterView: the cover page is on screen (animate only then).
    property bool animate: true
    readonly property bool shown: settings.lockscreenInfo !== false && m.ready && m.plugged && m.lockLine !== ""
    readonly property alias model: m

    implicitHeight: col.height
    height: shown ? implicitHeight : 0
    opacity: shown ? 1 : 0
    visible: opacity > 0
    Behavior on opacity { NumberAnimation { duration: 250 } }

    Fp6ChargingModel { id: m }
    GSettings {
        id: settings
        schema.id: "com.fairphone6.charging-feedback"
    }

    property real phase: 0
    Timer {
        interval: 80
        repeat: true
        running: cs.visible && cs.animate && m.charging && Powerd.status === Powerd.On
        onTriggered: cs.phase = (cs.phase + 0.08 / 2.4) % 1
    }
    readonly property real breath: m.charging ? 0.5 - 0.5 * Math.cos(2 * Math.PI * phase) : 0

    Column {
        id: col
        width: parent.width
        spacing: units.gu(0.5)

        Row {
            anchors.horizontalCenter: parent.horizontalCenter
            spacing: units.gu(1)
            Fp6ChargingRing {
                id: glyph
                width: units.gu(2.8)
                height: width
                anchors.verticalCenter: parent.verticalCenter
                progress: Math.max(0, m.level) / 100
                colorA: m.palette[0]
                colorB: m.palette[1]
                thickness: 0.11
                glow: 0.25 + 0.55 * cs.breath
                bolt: m.charging
                boltScale: 0.5
                boltColor: "white"
                boltOpacity: 0.7 + 0.3 * cs.breath
            }
            Label {
                objectName: "fp6ChargingLockLine"
                anchors.verticalCenter: parent.verticalCenter
                width: Math.min(implicitWidth, cs.width - glyph.width - units.gu(1.5))
                text: m.lockLine
                color: "white"
                fontSize: "medium"
                font.weight: Font.Light
                wrapMode: Text.WordWrap
                horizontalAlignment: lineCount > 1 ? Text.AlignHCenter : Text.AlignLeft
            }
        }
        Label {
            objectName: "fp6ChargingLockNote"
            anchors.horizontalCenter: parent.horizontalCenter
            width: Math.min(implicitWidth, cs.width - units.gu(4))
            visible: text !== ""
            text: m.lockNote
            color: "white"
            opacity: 0.72
            fontSize: "small"
            font.weight: Font.Light
            wrapMode: Text.WordWrap
            horizontalAlignment: Text.AlignHCenter
        }
    }
}
