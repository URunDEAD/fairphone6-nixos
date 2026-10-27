import QtQuick 2.12
import Lomiri.Components 1.3
import Qt.labs.settings 1.0

MainView {
    id: root
    objectName: "dictationSettings"
    applicationName: "com.fairphone6.dictation"
    width: units.gu(48)
    height: units.gu(94)
    backgroundColor: "#f5f5f5"
    function argument(prefix) {
        for (var i = 0; i < Qt.application.arguments.length; ++i)
            if (Qt.application.arguments[i].indexOf(prefix) === 0)
                return Qt.application.arguments[i].substring(prefix.length)
        return ""
    }
    Settings {
        id: preferences
        fileName: root.argument("--config=")
        property string language: "en"
        property string model: "base-q5_1"
        property bool punctuation: true
    }
    Page {
        id: page
        Rectangle { anchors.fill: parent; color: "#f5f5f5"; z: -1 }
        header: PageHeader { title: i18n.tr("Voice dictation") }
        Flickable {
            anchors.fill: parent
            anchors.topMargin: page.header.height
            contentHeight: content.height + units.gu(4)
            clip: true
            Column {
                id: content
                anchors { left: parent.left; right: parent.right; margins: units.gu(2.5) }
                spacing: units.gu(2)
                Rectangle {
                    width: parent.width; height: units.gu(14); radius: units.gu(1)
                    color: "#e5f2ec"
                    Column {
                        anchors { fill: parent; margins: units.gu(2) }
                        spacing: units.gu(1)
                        Label { text: i18n.tr("Your voice stays on this phone"); font.bold: true; color: "#175a40" }
                        Label {
                            width: parent.width
                            text: i18n.tr("Dictate offline from the keyboard. Audio is discarded after each request. No account or cloud service.")
                            wrapMode: Text.WordWrap; color: "#244e3d"
                        }
                    }
                }
                Label { text: i18n.tr("LANGUAGE"); fontSize: "small"; color: "#666666" }
                Row {
                    spacing: units.gu(1)
                    Button {
                        width: (content.width - units.gu(1))/2
                        text: "English"
                        color: preferences.language === "en" ? "#d9c7e8" : "#ffffff"
                        onClicked: { preferences.language = "en"; preferences.sync() }
                    }
                    Button {
                        width: (content.width - units.gu(1))/2
                        text: "Română"
                        color: preferences.language === "ro" ? "#d9c7e8" : "#ffffff"
                        onClicked: { preferences.language = "ro"; preferences.sync() }
                    }
                }
                Label { text: i18n.tr("OFFLINE MODEL"); fontSize: "small"; color: "#666666" }
                Column {
                    width: parent.width; spacing: units.gu(1)
                    Repeater {
                        model: [
                            { key: "tiny-q5_1", title: "Tiny · 31 MB · Lower memory" },
                            { key: "base-q5_1", title: "Base · 57 MB · Recommended" },
                            { key: "small-q5_1", title: "Small · 182 MB · Slower" }
                        ]
                        Button {
                            width: content.width
                            text: (preferences.model === modelData.key ? "✓  " : "") + modelData.title
                            color: preferences.model === modelData.key ? "#d9c7e8" : "#ffffff"
                            onClicked: { preferences.model = modelData.key; preferences.sync() }
                        }
                    }
                }
                Label {
                    width: parent.width; wrapMode: Text.WordWrap; fontSize: "small"; color: "#555555"
                    text: i18n.tr("Models are installed separately. Selecting one does not download it. Small may exceed the phone memory target.")
                }
                Rectangle { width: parent.width; height: units.dp(1); color: "#dddddd" }
                Row {
                    width: parent.width
                    Label { width: parent.width - units.gu(8); text: i18n.tr("Automatic punctuation"); anchors.verticalCenter: parent.verticalCenter }
                    Switch { checked: preferences.punctuation; onClicked: { preferences.punctuation = checked; preferences.sync() } }
                }
                Label {
                    width: parent.width; wrapMode: Text.WordWrap; fontSize: "small"; color: "#555555"
                    text: i18n.tr("Changes apply to the next recording. Hold or tap the keyboard microphone to speak, then release or tap again to insert text. Maximum 30 seconds.")
                }
                Rectangle {
                    width: parent.width; height: installText.height + units.gu(3); radius: units.gu(1); color: "#ffffff"
                    Label {
                        id: installText
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1.5) }
                        wrapMode: Text.WordWrap; fontSize: "small"
                        text: i18n.tr("Install selected model from Terminal:") + "\nfp6-dictate download " + preferences.model
                    }
                }
            }
        }
    }
    Timer {
        interval: 1500; running: root.argument("--preview=") !== ""; repeat: false
        onTriggered: root.grabToImage(function(result) {
            var success = result.saveToFile(root.argument("--preview="))
            console.log(success ? "Preview saved" : "Preview failed")
            Qt.quit()
        })
    }
}
