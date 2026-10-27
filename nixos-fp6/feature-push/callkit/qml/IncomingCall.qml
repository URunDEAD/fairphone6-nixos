import QtQuick 2.12
import Lomiri.Components 1.3

// Embeddable shell surface. The shell owns D-Bus and lock-screen privileges.
// Commands are intents: update callState only after the broker/provider confirms.
Rectangle {
    id: root
    width: units.gu(50)
    height: units.gu(100)
    color: "#f7f6f5"
    property string callState: "ringing"
    property bool silent: false
    property bool cellularBusy: false
    property bool muted: false
    property bool pending: false
    property string route: "earpiece"
    property string providerName: "Internet call"
    property string errorMessage: ""
    readonly property bool ringing: callState === "ringing"
    signal actionRequested(string action)
    signal routeRequested(string route)

    Rectangle { width: parent.width; height: units.gu(0.6); color: "#e95420" }
    Column {
        anchors { top: parent.top; topMargin: units.gu(6); horizontalCenter: parent.horizontalCenter }
        spacing: units.gu(1.2)
        Label { anchors.horizontalCenter: parent.horizontalCenter; text: "INTERNET CALL"; color: "#706965"; font.pixelSize: units.gu(1.6); font.letterSpacing: 2 }
        Label { anchors.horizontalCenter: parent.horizontalCenter; text: root.cellularBusy ? "Call waiting" : (root.ringing ? "Incoming call" : (root.callState === "held" ? "On hold" : (root.callState === "ended" ? "Call ended" : "Connected"))); font.pixelSize: units.gu(3.4); color: "#252220" }
    }
    Rectangle {
        id: avatar
        anchors { horizontalCenter: parent.horizontalCenter; top: parent.top; topMargin: units.gu(23) }
        width: units.gu(16); height: width; radius: width / 2
        color: "#ede0d9"
        Rectangle { anchors.centerIn: parent; width: parent.width - units.gu(1.6); height: width; radius: width / 2; color: "#fffaf7" }
        Label { anchors.centerIn: parent; text: "IC"; font.pixelSize: units.gu(5); color: "#a04728" }
    }
    Column {
        anchors { top: avatar.bottom; topMargin: units.gu(3); horizontalCenter: parent.horizontalCenter }
        width: parent.width - units.gu(6)
        spacing: units.gu(1.4)
        Label { width: parent.width; horizontalAlignment: Text.AlignHCenter; text: "Internet caller"; font.pixelSize: units.gu(3); color: "#252220" }
        Label { width: parent.width; horizontalAlignment: Text.AlignHCenter; text: root.providerName; color: "#706965"; font.pixelSize: units.gu(2); elide: Text.ElideRight }
        Label { width: parent.width; horizontalAlignment: Text.AlignHCenter; text: "Caller details hidden while locked"; color: "#706965"; font.pixelSize: units.gu(1.5) }
    }
    Rectangle {
        id: notice
        anchors { left: parent.left; right: parent.right; top: avatar.bottom; topMargin: units.gu(16); leftMargin: units.gu(3); rightMargin: units.gu(3) }
        height: units.gu(9); radius: units.gu(1)
        color: "#ebe7e4"
        visible: root.silent || root.cellularBusy || root.pending || root.errorMessage !== ""
        Label {
            anchors { fill: parent; margins: units.gu(1.5) }
            text: root.errorMessage !== "" ? root.errorMessage : (root.pending ? "Waiting for the calling app…" : (root.cellularBusy ? "A cellular call is in progress.\nFinish it before answering." : "Do Not Disturb\nRingtone and vibration are silenced."))
            color: "#554b45"; font.pixelSize: units.gu(1.8); wrapMode: Text.WordWrap; verticalAlignment: Text.AlignVCenter
        }
    }
    Row {
        visible: root.callState === "active" || root.callState === "held"
        anchors { horizontalCenter: parent.horizontalCenter; bottom: endButton.top; bottomMargin: units.gu(3) }
        spacing: units.gu(2)
        CallButton { text: root.muted ? "Unmute" : "Mute"; width: units.gu(12); enabled: !root.pending; onClicked: root.actionRequested(root.muted ? "unmute" : "mute") }
        CallButton { text: root.callState === "held" ? "Resume" : "Hold"; width: units.gu(12); enabled: !root.pending && !root.cellularBusy; onClicked: root.actionRequested(root.callState === "held" ? "resume" : "hold") }
    }
    Row {
        visible: root.callState === "active" || root.callState === "held"
        anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: units.gu(30) }
        spacing: units.gu(1)
        Repeater {
            model: ["earpiece", "speaker", "bluetooth"]
            CallButton {
                width: units.gu(13); text: modelData === "bluetooth" ? "Bluetooth" : (modelData === "speaker" ? "Speaker" : "Earpiece")
                color: root.route === modelData ? "#e95420" : "#ffffff"
                enabled: !root.pending; onClicked: root.routeRequested(modelData)
            }
        }
    }
    Row {
        visible: root.ringing
        anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: units.gu(12) }
        spacing: units.gu(3)
        CallButton { objectName: "declineButton"; width: units.gu(18); height: units.gu(7); text: "Decline"; color: "#c94343"; onClicked: root.actionRequested("end") }
        CallButton { objectName: "answerButton"; width: units.gu(18); height: units.gu(7); text: "Answer"; color: "#27865a"; enabled: !root.cellularBusy && !root.pending; onClicked: root.actionRequested("answer") }
    }
    CallButton {
        id: endButton
        objectName: "endButton"
        visible: root.callState === "active" || root.callState === "held"
        anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: units.gu(12) }
        width: units.gu(30); height: units.gu(7); text: "End call"; color: "#c94343"; onClicked: root.actionRequested("end")
    }
    Label {
        anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: units.gu(5) }
        text: "Unlock to open the calling app"; color: "#706965"; font.pixelSize: units.gu(1.6)
    }
}
