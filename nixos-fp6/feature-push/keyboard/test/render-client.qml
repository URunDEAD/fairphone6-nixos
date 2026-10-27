// Text field client for render.sh: takes focus and asks for the input panel
// (maliit D-Bus input context, QT_IM_MODULE=maliit).
import QtQuick 2.4
import QtQuick.Window 2.2

Window {
    id: win
    visible: true
    width: Screen.width
    height: Screen.height
    color: "#f0f0f0"

    Rectangle {
        x: 24; y: 120; width: parent.width - 48; height: 120
        color: "white"; border.color: "#888"; border.width: 2
        TextInput {
            id: field
            objectName: "field"
            anchors.fill: parent
            anchors.margins: 16
            font.pixelSize: 48
            focus: true
            inputMethodHints: Qt.ImhNone
        }
    }
    // "clear" button (render.sh taps it between scenarios).
    Rectangle {
        x: parent.width - 220; y: 20; width: 200; height: 80
        color: "#ddd"
        Text { anchors.centerIn: parent; text: "clear"; font.pixelSize: 32 }
        MouseArea {
            anchors.fill: parent
            onClicked: { Qt.inputMethod.reset(); field.text = ""; field.forceActiveFocus(); Qt.inputMethod.show(); }
        }
    }
    Text {
        x: 24; y: 40
        font.pixelSize: 36
        text: "fp6 keyboard render test"
    }
    Timer {
        interval: 1500; running: true; repeat: true
        onTriggered: { if (!Qt.inputMethod.visible) { win.requestActivate(); field.forceActiveFocus(); Qt.inputMethod.show(); } }
    }
}
