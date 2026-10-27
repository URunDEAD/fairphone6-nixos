import QtQuick 2.12
import QtQuick.Window 2.2
import Lomiri.Components 1.3

Window {
    visible: true
    width: units.gu(45)
    height: units.gu(86)
    title: "Run self-test"
MainView {
    id: root
    objectName: "selfTestView"
    applicationName: "com.fairphone6.selftest"
    anchors.fill: parent
    backgroundColor: "#f8f7f6"
    Component.onCompleted: if (autoRun) backend.start()
    property var report: backend.report_json.length ? JSON.parse(backend.report_json) : null
    function colorFor(status) {
        return status === "PASS" ? "#23734b" : status === "FAIL" ? "#b52e38" : status === "WARN" ? "#98620d" : "#666666";
    }
    Page {
        id: page
        Rectangle { anchors.fill: parent; color: "#f8f7f6"; z: -1 }
        header: PageHeader { title: "Run self-test" }
        Flickable {
            anchors { fill: parent; topMargin: page.header.height; bottomMargin: footer.height }
            contentHeight: content.height + units.gu(3)
            clip: true
            Column {
                id: content
                x: units.gu(2.5)
                width: parent.width - units.gu(5)
                spacing: units.gu(2)
                Item { width: 1; height: units.gu(0.5) }
                Label { text: "DEVICE HEALTH"; color: "#772953"; fontSize: "small" }
                Label { text: root.report ? "Your phone, checked." : "Ready for the day?"; fontSize: "x-large"; width: parent.width; wrapMode: Text.Wrap }
                Label {
                    visible: !root.report
                    text: "Check system health and installed features after an update. Your settings stay as they are."
                    width: parent.width; wrapMode: Text.Wrap; color: "#555555"
                }
                Rectangle {
                    visible: !root.report
                    width: parent.width; height: privacy.height + units.gu(3); radius: units.gu(1); color: "#eee7ed"
                    Label {
                        id: privacy
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1.5) }
                        text: "Only on this phone\nNo uploads, audio playback or device discovery. Reports contain summaries, never personal identifiers."
                        wrapMode: Text.Wrap; color: "#5e2750"; fontSize: "small"
                    }
                }
                Label { visible: !root.report; text: "What gets checked"; fontSize: "large" }
                Repeater {
                    model: root.report ? [] : ["System services, memory & storage", "Audio, keyboard & connectivity", "Installed Fairphone features", "Settings schemas & app launchers"]
                    Label { text: "•  " + modelData; width: content.width; wrapMode: Text.Wrap }
                }
                Rectangle {
                    visible: root.report !== null
                    width: parent.width; height: units.gu(7); radius: units.gu(1); color: "#ffffff"
                    Label {
                        anchors { left: parent.left; verticalCenter: parent.verticalCenter; leftMargin: units.gu(1.5) }
                        text: root.report ? root.report.status + " · " + root.report.checks.length + " checks" : ""
                        color: root.report ? root.colorFor(root.report.status) : "#666666"; fontSize: "large"
                    }
                }
                Repeater {
                    model: root.report ? root.report.checks : []
                    Column {
                        width: content.width; spacing: units.gu(0.5)
                        Row {
                            spacing: units.gu(1)
                            Label { text: modelData.status; color: root.colorFor(modelData.status); font.bold: true; width: units.gu(6) }
                            Label { text: modelData.id.replace(/-/g, " "); width: content.width - units.gu(7); wrapMode: Text.Wrap; textFormat: Text.PlainText }
                        }
                        Label { text: modelData.detail; width: parent.width; wrapMode: Text.Wrap; color: "#666666"; fontSize: "small"; textFormat: Text.PlainText }
                        Item { width: 1; height: units.gu(0.5) }
                        Rectangle { width: parent.width; height: 1; color: "#e4e0dd" }
                    }
                }
                Label { text: "PASS  Healthy    WARN  Needs a look\nFAIL  Check failed    SKIP  Not checked"; width: parent.width; wrapMode: Text.Wrap; fontSize: "small"; color: "#666666" }
            }
        }
        Rectangle {
            id: footer
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
            height: actions.height + units.gu(3)
            color: "#ffffff"
            Rectangle { width: parent.width; height: 1; color: "#e4e0dd" }
            Column {
                id: actions
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1.5) }
                spacing: units.gu(1)
                Label { text: backend.message; visible: text.length > 0; width: parent.width; wrapMode: Text.Wrap; fontSize: "small"; textFormat: Text.PlainText }
                Button { width: parent.width; text: backend.busy ? "Checking…" : root.report ? "Run again" : "Run self-test"; color: "#e95420"; enabled: !backend.busy; onClicked: backend.start() }
                Label { text: "Private reports · Documents / Self-tests"; fontSize: "small"; color: "#666666"; anchors.horizontalCenter: parent.horizontalCenter }
            }
        }
    }
    Timer { interval: 150; repeat: true; running: backend.busy; onTriggered: backend.poll() }
    Timer {
        interval: 1500; running: previewPath.length > 0
        onTriggered: root.grabToImage(function(result) { if (!result.saveToFile(previewPath)) console.error("Preview could not be saved"); Qt.quit(); })
    }
}

}
