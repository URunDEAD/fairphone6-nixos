// Tube (vial) level: bubble moves along the tube, +-5 deg full scale.
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    id: t
    property real deviation: 0
    property real tolerance: 0.3
    property real range: 5
    readonly property bool level: Math.abs(deviation) <= tolerance

    Rectangle {
        id: tube
        anchors.fill: parent
        radius: height / 2
        color: theme.palette.normal.foreground
        border.color: theme.palette.normal.base
        border.width: units.dp(2)
    }
    Repeater {
        model: [-1, 1]
        Rectangle {
            width: units.dp(2); height: t.height * 0.8
            anchors.verticalCenter: parent.verticalCenter
            x: t.width / 2 + modelData * t.height * 0.55
            color: theme.palette.normal.base
        }
    }
    Rectangle {
        width: t.height * 0.9; height: t.height * 0.7; radius: height / 2
        anchors.verticalCenter: parent.verticalCenter
        readonly property real c: Math.max(-1, Math.min(1, -t.deviation / t.range))
        x: (t.width - width) / 2 + c * (t.width - width) / 2
        color: t.level ? theme.palette.normal.positive : theme.palette.normal.activity
        opacity: 0.85
    }
}
