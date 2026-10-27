// Laptop preview stand-in for the camera (no camera is opened): a
// synthetic Wi-Fi QR code on a "desk", decoded through the same grab ->
// QrScanFilter.decodeImage -> Rust path the scanner falls back to.
import QtQuick 2.12
import Fp6.WifiQrScan 1.0

Rectangle {
    id: view
    property bool active: false
    property var filter: null
    readonly property bool ready: true
    readonly property string error: ""
    property string payload: fp6PreviewPayload.length > 0 ? fp6PreviewPayload
                             : "WIFI:T:WPA;S:Test\\;Net;P:p@ss\\:word\\,1;H:false;;"
    gradient: Gradient {
        GradientStop { position: 0; color: "#5b5348" }
        GradientStop { position: 1; color: "#2d2924" }
    }
    clip: true

    Rectangle {
        id: card
        anchors.centerIn: parent
        anchors.verticalCenterOffset: -parent.height * 0.04
        width: Math.min(parent.width, parent.height) * 0.56
        height: width * 1.25
        rotation: -6
        color: "#f3f1ec"
        radius: width * 0.03
        QrCode {
            anchors { top: parent.top; left: parent.left; right: parent.right; margins: parent.width * 0.06 }
            height: width
            text: view.payload
        }
        Text {
            anchors { bottom: parent.bottom; horizontalCenter: parent.horizontalCenter; bottomMargin: parent.height * 0.05 }
            text: "Guest Wi-Fi"
            color: "#333"
            font.pixelSize: parent.width * 0.09
        }
    }

    function grab(callback) {
        view.grabToImage(function (result) { callback(result.image); });
    }
    function focusAt(x, y) {}
}
