import QtQuick 2.12
import Lomiri.Components 1.3

// Embed in the shell's system indicator area. The trusted shell adapter supplies
// captureActive from D-Bus MicActive/StatusChanged OR live PipeWire capture nodes.
Rectangle {
    property bool captureActive: false
    visible: captureActive
    width: units.gu(3)
    height: units.gu(3)
    radius: width / 2
    color: "#168447"
    Accessible.name: qsTr("Microphone in use")
    Icon {
        anchors.centerIn: parent
        width: units.gu(2); height: width
        name: "audio-input-microphone-symbolic"
        color: "white"
    }
}
