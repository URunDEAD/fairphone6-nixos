import QtQuick 2.12
import QtQuick.Window 2.2
import Lomiri.Components 1.3
import Fairphone.LocalLlm 1.0

Window {
    visible: true
    width: units.gu(45)
    height: units.gu(100)
    title: "On-device AI models"
MainView {
    id: root
    anchors.fill: parent
    applicationName: "com.fairphone6.local-llm"
    backgroundColor: "#f8f7f6"
    Page {
        anchors.fill: parent
        Rectangle { anchors.fill: parent; color: "#f8f7f6"; z: -1 }
        header: PageHeader { title: "On-device AI models" }
        ModelsView {
            id: content
            anchors { fill: parent; topMargin: parent.header.height }
            preview: previewPath.length > 0
            fixture: previewSnapshot
            contentY: previewDownloads ? units.gu(67) : 0
        }
    }
    Timer {
        interval: 1200; running: previewPath.length > 0
        onTriggered: root.grabToImage(function(result) { result.saveToFile(previewPath); Qt.quit(); })
    }
}
}
