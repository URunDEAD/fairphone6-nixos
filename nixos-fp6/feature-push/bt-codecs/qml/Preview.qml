import QtQuick 2.12
import Lomiri.Components 1.3
import QtQuick.Window 2.2
Window {
    visible: true
    width: 450; height: 920
MainView {
    id: view
    anchors.fill: parent
    Rectangle { anchors.fill: parent; color: "#fafafa" }
    applicationName: "fp6-bluetooth-audio-preview"
    property bool stable: Qt.application.arguments.indexOf("--stable") >= 0
    property bool call: Qt.application.arguments.indexOf("--call") >= 0
    property bool disconnected: Qt.application.arguments.indexOf("--disconnected") >= 0
    property bool failed: Qt.application.arguments.indexOf("--error") >= 0
    QtObject {
        id: fixtureAudio
        property bool busy: false
        property bool available: true
        property string error: view.failed ? "Bluetooth audio service unavailable." : ""
        property var devices: view.disconnected || view.failed ? [] : [{
            id: "bluez_card.00_00_00_00_00_01", address: "00:00:00:00:00:01", name: "Test Headphones",
            profile: view.call ? "headset-head-unit-msbc" : view.stable ? "a2dp-sink-sbc" : "a2dp-sink",
            codec: view.call ? "msbc" : view.stable ? "sbc" : "ldac",
            bitrate: view.call ? 64000 : null, bitrate_source: "nominal SCO transport rate",
            profiles: [
                {id:"a2dp-sink",kind:"A2DP",codec:"ldac",available:true},
                {id:"a2dp-sink-sbc",kind:"A2DP",codec:"sbc",available:true},
                {id:"headset-head-unit-msbc",kind:"HFP",codec:"msbc",available:true}
            ]
        }]
        function selectProfile(device, profile) { console.log("explicit selection") }
        function prefer(device, quality) { console.log("explicit preference") }
    }
    Page {
        id: page
        header: null
        Rectangle {
            anchors { top: parent.top; left: parent.left; right: parent.right }
            height: units.gu(6)
            color: "white"
            Label { anchors { left: parent.left; leftMargin: units.gu(2); verticalCenter: parent.verticalCenter } text: "Test Headphones"; fontSize: "large" }
        }
        Column {
            anchors { left: parent.left; right: parent.right; top: parent.top; topMargin: units.gu(8) }
            spacing: units.gu(2)
            Label {
                anchors.left: parent.left
                anchors.leftMargin: units.gu(2)
                text: view.disconnected ? "Not connected" : "Connected"
                color: theme.palette.normal.positive
            }
            Fp6AudioQuality { width: parent.width; audio: fixtureAudio; address: "00:00:00:00:00:01"; connected: !view.disconnected }
        }
    }
    Timer {
        interval: 1200; running: true
        onTriggered: view.grabToImage(function(result) {
            var args = Qt.application.arguments
            var path = args.filter(function(a) { return a.indexOf("--output=") === 0 })[0].substring(9)
            if (!result.saveToFile(path)) { console.error("save failed"); Qt.quit(1) }
            Qt.quit()
        })
    }
}
}
