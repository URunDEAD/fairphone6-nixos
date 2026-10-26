// Two-column row: name on the left, value on the right.
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    property alias name: n.text
    property alias value: v.text
    property alias valueColor: v.color
    width: parent ? parent.width : units.gu(40)
    height: Math.max(n.height, v.height) + units.gu(1.2)
    Label {
        id: n
        anchors { left: parent.left; verticalCenter: parent.verticalCenter }
        width: parent.width * 0.45
        wrapMode: Text.Wrap
        color: theme.palette.normal.backgroundSecondaryText
    }
    Label {
        id: v
        anchors { right: parent.right; verticalCenter: parent.verticalCenter }
        width: parent.width * 0.55
        horizontalAlignment: Text.AlignRight
        wrapMode: Text.Wrap
    }
    Rectangle {
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        height: units.dp(1)
        color: theme.palette.normal.base
        opacity: 0.5
    }
}
