// Inline notice: icon + wrapped text, accent border.
import QtQuick 2.12
import Lomiri.Components 1.3

Rectangle {
    id: notice
    property alias text: label.text
    property string iconName: "info"
    property color accent: theme.palette.normal.activity
    width: parent ? parent.width : units.gu(40)
    height: row.height + units.gu(3)
    radius: units.gu(1)
    color: theme.palette.normal.foreground
    border.color: accent
    border.width: units.dp(1)
    Row {
        id: row
        anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: units.gu(1.5) }
        spacing: units.gu(1.5)
        Icon {
            id: icon
            name: notice.iconName
            width: units.gu(2.5); height: width
            color: notice.accent
        }
        Label {
            id: label
            width: parent.width - icon.width - parent.spacing
            wrapMode: Text.Wrap
            textSize: Label.Small
            color: theme.palette.normal.foregroundText
        }
    }
}
