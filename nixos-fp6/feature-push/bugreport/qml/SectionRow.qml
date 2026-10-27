import QtQuick 2.12
import Lomiri.Components 1.3
Item {
    id: row
    property string title
    property string description
    property bool selected: true
    height: Math.max(units.gu(8), textColumn.height + units.gu(3))
    width: parent.width
    Column {
        id: textColumn
        anchors { left: parent.left; right: toggle.left; rightMargin: units.gu(2); verticalCenter: parent.verticalCenter }
        spacing: units.gu(0.4)
        Label { text: row.title; fontSize: "medium"; width: parent.width; wrapMode: Text.Wrap }
        Label { text: row.description; color: "#666666"; fontSize: "small"; width: parent.width; wrapMode: Text.Wrap }
    }
    Switch {
        id: toggle
        anchors { right: parent.right; verticalCenter: parent.verticalCenter }
        checked: row.selected
        onCheckedChanged: row.selected = checked
        Accessible.name: row.title
    }
    Rectangle { anchors.bottom: parent.bottom; width: parent.width; height: 1; color: "#e6e6e6" }
}
