// A compact "the assistant used a tool" chip; tap opens the activity log.
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    id: chip
    property string label: ""
    signal clicked()
    height: units.gu(4)
    width: parent ? parent.width : 0
    Rectangle {
        x: units.gu(2)
        height: units.gu(3.5)
        width: chipRow.width + units.gu(2)
        radius: height / 2
        color: theme.palette.normal.base
        Row {
            id: chipRow
            anchors.centerIn: parent
            spacing: units.gu(0.75)
            Icon { name: "settings"; width: units.gu(2); height: width; anchors.verticalCenter: parent.verticalCenter }
            Label { text: chip.label; textSize: Label.Small; anchors.verticalCenter: parent.verticalCenter }
        }
        MouseArea { anchors.fill: parent; onClicked: chip.clicked() }
    }
}
