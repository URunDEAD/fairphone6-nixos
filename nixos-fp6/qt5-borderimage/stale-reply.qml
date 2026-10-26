import QtQuick 2.15
Item {
    width: 32; height: 32
    BorderImage { id: border; anchors.fill: parent }
    Component.onCompleted: {
        border.source = "fp6-unsupported-test://host/first.sci";
        border.source = "";
    }
    Timer { interval: 500; running: true; onTriggered: { console.log("FP6_BORDERIMAGE_SURVIVED"); Qt.quit(); } }
}
