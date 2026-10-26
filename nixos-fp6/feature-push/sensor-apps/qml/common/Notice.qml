// Inline notice card: icon, text, optional button.
import QtQuick 2.12
import Lomiri.Components 1.3

Rectangle {
    id: notice
    property alias text: label.text
    property string iconName: "info"
    property string buttonText: ""
    property color accent: theme.palette.normal.activity
    signal clicked()

    width: parent ? parent.width : units.gu(40)
    height: col.height + units.gu(3)
    radius: units.gu(1)
    color: theme.palette.normal.foreground
    border.color: accent
    border.width: units.dp(1)

    Column {
        id: col
        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1.5) }
        spacing: units.gu(1)
        Row {
            width: parent.width
            spacing: units.gu(1.5)
            Icon {
                id: icon
                name: notice.iconName
                width: units.gu(3); height: width
                color: notice.accent
            }
            Label {
                id: label
                width: parent.width - icon.width - parent.spacing
                wrapMode: Text.Wrap
                color: theme.palette.normal.foregroundText
            }
        }
        Button {
            visible: notice.buttonText !== ""
            text: notice.buttonText
            anchors.right: parent.right
            color: theme.palette.normal.positive
            onClicked: notice.clicked()
        }
    }
}
