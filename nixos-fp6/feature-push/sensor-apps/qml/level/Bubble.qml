// Circular 2-axis bubble level. Full scale: 10 degrees at the rim.
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    id: b
    property real dx: 0
    property real dy: 0
    property real tolerance: 0.3
    property real range: 10
    readonly property bool level: Math.abs(dx) <= tolerance && Math.abs(dy) <= tolerance
    readonly property real r: width / 2

    Rectangle {
        anchors.fill: parent
        radius: width / 2
        color: theme.palette.normal.foreground
        border.color: theme.palette.normal.base
        border.width: units.dp(2)
    }
    Repeater {
        model: [1, 2, 5]
        Rectangle {
            width: b.width * modelData / b.range; height: width; radius: width / 2
            anchors.centerIn: parent
            color: "transparent"
            border.color: theme.palette.normal.base
            border.width: units.dp(1)
        }
    }
    Rectangle { anchors.centerIn: parent; width: parent.width; height: units.dp(1); color: theme.palette.normal.base }
    Rectangle { anchors.centerIn: parent; height: parent.height; width: units.dp(1); color: theme.palette.normal.base }

    Rectangle {
        id: bubble
        width: b.width * 0.16; height: width; radius: width / 2
        // the bubble moves to the higher side
        readonly property real len: Math.sqrt(b.dx * b.dx + b.dy * b.dy)
        readonly property real k: len > b.range ? b.range / len : 1
        x: b.r - width / 2 + (b.dx * k / b.range) * (b.r - width / 2)
        y: b.r - height / 2 - (b.dy * k / b.range) * (b.r - height / 2)
        color: b.level ? theme.palette.normal.positive : theme.palette.normal.activity
        opacity: 0.85
        border.color: Qt.darker(color, 1.3)
        border.width: units.dp(2)
        Behavior on color { ColorAnimation { duration: 150 } }
    }
}
