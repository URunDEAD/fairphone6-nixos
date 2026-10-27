import QtQuick 2.12
import Lomiri.Components 1.3

// QtQuick geometry avoids UITK's OpenGL-only ShapeMaterial in offscreen software
// previews. Typography and sizing still use UITK 1.3; controls retain Suru colours.
Rectangle {
    id: control
    property string text: ""
    signal clicked()
    width: units.gu(14)
    height: units.gu(5)
    radius: units.gu(0.7)
    color: "#ffffff"
    border.color: activeFocus ? "#e95420" : "#d8d1cc"
    border.width: activeFocus ? 2 : 1
    opacity: enabled ? (pointer.pressed ? 0.75 : 1) : 0.4
    activeFocusOnTab: enabled
    Accessible.role: Accessible.Button
    Accessible.name: text
    Accessible.onPressAction: if (control.enabled) control.clicked()
    Keys.onReturnPressed: if (enabled) clicked()
    Keys.onSpacePressed: if (enabled) clicked()
    Label {
        anchors.centerIn: parent
        text: control.text
        color: control.color.toString() === "#ffffff" ? "#36302c" : "#ffffff"
        font.pixelSize: units.gu(1.9)
    }
    MouseArea { id: pointer; anchors.fill: parent; enabled: control.enabled; onClicked: control.clicked() }
}
