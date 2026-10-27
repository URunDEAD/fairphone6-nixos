// Horizontal cents scale (-50..+50) with an in-tune zone and a needle.
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    id: meter
    property real cents: NaN
    property real tolerance: 5
    readonly property bool inTune: isFinite(cents) && Math.abs(cents) <= tolerance
    implicitHeight: units.gu(10)

    function xOf(c) { return width / 2 + Math.max(-50, Math.min(50, c)) / 50 * (width / 2 - units.gu(1)) }

    // scale
    Rectangle {
        id: track
        anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter }
        height: units.gu(1)
        radius: height / 2
        color: theme.palette.normal.foreground
    }
    Rectangle {
        anchors.verticalCenter: track.verticalCenter
        x: meter.xOf(-meter.tolerance)
        width: meter.xOf(meter.tolerance) - x
        height: units.gu(2)
        radius: units.gu(0.5)
        color: theme.palette.normal.positive
        opacity: meter.inTune ? 0.9 : 0.35
    }
    Repeater {
        model: 11
        Rectangle {
            readonly property int c: -50 + index * 10
            x: meter.xOf(c) - width / 2
            anchors.verticalCenter: track.verticalCenter
            width: units.dp(2)
            height: c === 0 ? units.gu(4) : (c % 50 === 0 ? units.gu(3) : units.gu(2))
            color: theme.palette.normal.backgroundSecondaryText
        }
    }
    Label {
        x: meter.xOf(-50) - width / 2; anchors.top: track.bottom; anchors.topMargin: units.gu(2)
        text: "−50"; textSize: Label.Small; color: theme.palette.normal.backgroundSecondaryText
    }
    Label {
        x: meter.xOf(0) - width / 2; anchors.top: track.bottom; anchors.topMargin: units.gu(2)
        text: "0"; textSize: Label.Small; color: theme.palette.normal.backgroundSecondaryText
    }
    Label {
        x: meter.xOf(50) - width / 2; anchors.top: track.bottom; anchors.topMargin: units.gu(2)
        text: "+50"; textSize: Label.Small; color: theme.palette.normal.backgroundSecondaryText
    }
    // needle
    Rectangle {
        visible: isFinite(meter.cents)
        x: meter.xOf(meter.cents) - width / 2
        anchors.verticalCenter: track.verticalCenter
        width: units.gu(0.8)
        height: units.gu(6)
        radius: width / 2
        color: meter.inTune ? theme.palette.normal.positive
               : Math.abs(meter.cents) < 20 ? LomiriColors.orange : theme.palette.normal.negative
        Behavior on x { NumberAnimation { duration: 80 } }
    }
}
