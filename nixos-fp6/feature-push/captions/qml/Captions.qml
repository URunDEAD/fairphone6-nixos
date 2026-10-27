import QtQuick 2.12
import QtQuick.Window 2.12
import Lomiri.Components 1.3
import Qt.labs.settings 1.0

Window {
    id: window
    visible: true
    width: units.gu(48)
    height: collapsed ? units.gu(7) : (settingsOpen ? units.gu(76) : units.gu(40))
    minimumWidth: units.gu(40)
    title: "Live captions"
    flags: Qt.Window | Qt.WindowStaysOnTopHint | Qt.FramelessWindowHint
    color: "transparent"
    property bool collapsed: argument("--collapsed=") === "true"
    property bool settingsOpen: argument("--settings=") === "true"
    property bool preview: argument("--preview=") !== ""
    property var status: ({state:"idle", capture_active:false, text:"", message:"Captions are off", dropped:0})
    property string notice: ""
    property bool allowCalls: false // Explicit consent never persists across app launches.
    function argument(prefix) {
        for (var i=0; i<Qt.application.arguments.length; ++i)
            if (Qt.application.arguments[i].indexOf(prefix) === 0) return Qt.application.arguments[i].substring(prefix.length)
        return ""
    }
    Settings {
        id: prefs
        category: "Captions"
        property string language: "en"
        property string model: "tiny-q5_1"
        property string source: "monitor"
        property real panelOpacity: 0.94
        property int threads: 2
        property int silenceMinutes: 3
    }
    onClosing: if (!preview) backend.command("Stop", "")
    Rectangle {
        id: panel
        anchors.fill: parent
        radius: units.gu(1.5)
        color: Qt.rgba(0.10,0.12,0.14,prefs.panelOpacity)
        border.color: "#485052"
        Column {
            anchors.fill: parent
            spacing: units.gu(1)
            Rectangle {
                width: parent.width; height: units.gu(7); radius: units.gu(1.5); color: "#263a36"
                MouseArea {
                    anchors.fill: parent
                    property real pressX; property real pressY
                    onPressed: { pressX=mouse.x; pressY=mouse.y }
                    onPositionChanged: if (pressed) { window.x += mouse.x-pressX; window.y += mouse.y-pressY }
                }
                Row {
                    anchors { left: parent.left; leftMargin: units.gu(2); verticalCenter: parent.verticalCenter }
                    spacing: units.gu(1)
                    Rectangle { width: units.gu(1); height: width; radius: width/2; color: status.capture_active ? "#71e0b3" : "#94a29d"; anchors.verticalCenter: parent.verticalCenter }
                    Label { text: i18n.tr("Live captions"); color: "white"; font.bold: true; anchors.verticalCenter: parent.verticalCenter }
                }
                Row {
                    anchors { right: parent.right; rightMargin: units.gu(1); verticalCenter: parent.verticalCenter }
                    spacing: units.gu(0.5)
                    Button { width: units.gu(5); text: "⚙"; onClicked: { settingsOpen=!settingsOpen; collapsed=false } }
                    Button { width: units.gu(5); text: collapsed ? "+" : "−"; onClicked: collapsed = !collapsed }
                    Button { width: units.gu(5); text: "×"; onClicked: window.close() }
                }
            }
            Column {
                visible: !collapsed
                width: parent.width-units.gu(4)
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: units.gu(1.2)
                Label { width: parent.width; text: status.capture_active ? (status.source === "monitor" ? i18n.tr("● DEVICE AUDIO IN USE") : i18n.tr("● MICROPHONE IN USE")) : i18n.tr("PRIVATE · OFFLINE"); color: "#71e0b3"; fontSize: "small" }
                Rectangle {
                    width: parent.width; height: units.gu(14); color: "#111a1a"; radius: units.gu(1)
                    Flickable {
                        id: captions
                        anchors { fill: parent; margins: units.gu(1.5) }
                        contentHeight: captionText.height; clip: true
                        Label {
                            id: captionText
                            width: parent.width
                            text: status.text || (status.capture_active ? i18n.tr("Listening for speech…") : i18n.tr("Caption videos, podcasts and voice messages on this phone."))
                            color: "white"; fontSize: "large"; wrapMode: Text.WordWrap
                            textFormat: Text.PlainText
                            onTextChanged: captions.contentY = Math.max(0, height-captions.height)
                        }
                    }
                }
                Row {
                    width: parent.width; spacing: units.gu(1)
                    Button {
                        width: (parent.width-units.gu(1))/2
                        text: status.state === "starting" || status.capture_active ? i18n.tr("Stop captions") : i18n.tr("Start captions")
                        color: "#71e0b3"
                        onClicked: {
                            if (preview) return
                            notice = backend.command(status.state === "starting" || status.capture_active ? "Stop" : "Start", JSON.stringify({ language:prefs.language,model:prefs.model,source:prefs.source,threads:prefs.threads,silence_minutes:prefs.silenceMinutes,allow_calls:allowCalls }))
                        }
                    }
                    Button { width: (parent.width-units.gu(1))/2; text: i18n.tr("Save visible text"); enabled: status.text !== ""; onClicked: if (!preview) notice=backend.save() }
                }
                Row {
                    visible: settingsOpen
                    width: parent.width; spacing: units.gu(1); enabled: !status.capture_active && status.state !== "starting"
                    Button { width: (parent.width-units.gu(1))/2; text: prefs.language === "en" ? "English" : "Română"; onClicked: prefs.language=prefs.language === "en" ? "ro" : "en" }
                    Button { width: (parent.width-units.gu(1))/2; text: prefs.model === "tiny-q5_1" ? "Tiny · Q5" : "Base · Q5"; onClicked: prefs.model=prefs.model === "tiny-q5_1" ? "base-q5_1" : "tiny-q5_1" }
                }
                Button { visible: settingsOpen; width: parent.width; enabled: !status.capture_active && status.state !== "starting"; text: prefs.source === "monitor" ? i18n.tr("Source: device audio") : i18n.tr("Source: microphone"); onClicked: prefs.source=prefs.source === "monitor" ? "microphone" : "monitor" }
                Row {
                    visible: settingsOpen
                    width: parent.width; spacing: units.gu(1); enabled: !status.capture_active && status.state !== "starting"
                    Button { width: (parent.width-units.gu(1))/2; text: prefs.threads + " CPU threads"; onClicked: prefs.threads=prefs.threads === 2 ? 4 : 2 }
                    Button { width: (parent.width-units.gu(1))/2; text: "Stop: " + prefs.silenceMinutes + " min silent"; onClicked: prefs.silenceMinutes=prefs.silenceMinutes === 3 ? 1 : prefs.silenceMinutes === 1 ? 5 : 3 }
                }
                Row {
                    spacing: units.gu(1); width: parent.width
                    visible: settingsOpen
                    Label { text: i18n.tr("Opacity"); color: "#e3e8e5"; width: units.gu(9); anchors.verticalCenter: parent.verticalCenter }
                    Slider { width: parent.width-units.gu(10); minimumValue: 0.55; maximumValue: 1; value: prefs.panelOpacity; onValueChanged: prefs.panelOpacity=value }
                }
                Row {
                    spacing: units.gu(1); width: parent.width
                    enabled: !status.capture_active && status.state !== "starting"
                    visible: settingsOpen
                    Button { id: calls; width: units.gu(6); text: (status.capture_active ? status.allow_calls : allowCalls) ? i18n.tr("On") : i18n.tr("Off"); color: (status.capture_active ? status.allow_calls : allowCalls) ? "#71e0b3" : "#606866"; onClicked: allowCalls=!allowCalls }
                    Label { width: parent.width-calls.width-units.gu(1); text: i18n.tr("Allow during calls or unknown call status"); wrapMode: Text.WordWrap; color: "#e3e8e5"; fontSize: "small" }
                }
                Label { width: parent.width; text: notice || status.message || i18n.tr("Captions are off"); color: "#cbd8d2"; wrapMode: Text.WordWrap; textFormat: Text.PlainText; fontSize: "small"; maximumLineCount: 3; elide: Text.ElideRight }
                Label { width: parent.width; text: i18n.tr("Nothing is saved unless you tap Save. Continuous decoding uses battery.") + (status.dropped ? " Skipped windows: " + status.dropped : ""); color: "#a4b5ae"; wrapMode: Text.WordWrap; fontSize: "x-small" }
            }
        }
    }
    Timer {
        interval: 200; running: !preview; repeat: true
        onTriggered: { try { status=JSON.parse(backend.snapshot()) } catch(e) {} }
    }
    Timer {
        interval: 1500; running: preview
        onTriggered: panel.grabToImage(function(result) { if (!result.saveToFile(argument("--preview="))) console.error("Preview save failed"); Qt.quit() })
    }
    Component.onCompleted: if (preview) {
        prefs.language=argument("--language=") || "en"
        status={state:"listening",capture_active:true,source:"monitor",allow_calls:false,text:prefs.language === "ro" ? "Trenul ajunge în gară peste zece minute. Ne întâlnim acolo." : "The train arrives at the station in ten minutes. I will meet you there.",message:"Synthetic preview · audio stays on this phone",dropped:0}
    }
}
