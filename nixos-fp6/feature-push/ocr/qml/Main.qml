import QtQuick 2.12
import Lomiri.Components 1.3

MainView {
    id: root
    objectName: "ocrApp"
    applicationName: "fairphone-ocr"
    anchorToKeyboard: true
    width: units.gu(48)
    height: units.gu(90)
    backgroundColor: "#f5f6f7"
    property string apiUrl: ""
    property string initialPath: ""
    property bool preview: false
    property var result: null
    property var chosen: ({})
    property string jobId: ""
    property bool busy: false
    property int progress: 0
    property string message: "Your images stay on this device."
    property bool indexEnabled: false
    onIndexEnabledChanged: indexSwitch.checked = indexEnabled
    property var hits: []
    property bool showingSearch: false
    property string language: "eng+ron"
    property color accent: "#087f82"

    function call(body, callback) {
        if (!apiUrl) return;
        var xhr = new XMLHttpRequest();
        xhr.open("POST", apiUrl);
        xhr.setRequestHeader("Content-Type", "application/json");
        xhr.setRequestHeader("X-Ocr-Token", apiUrl.substring(apiUrl.lastIndexOf("/") + 1));
        xhr.onreadystatechange = function() {
            if (xhr.readyState !== XMLHttpRequest.DONE) return;
            if (xhr.status !== 200) { message = "Cannot reach the OCR service."; busy = false; jobId = ""; return; }
            try {
                var reply = JSON.parse(xhr.responseText);
                if (reply.error) { message = reply.error; if (body.action === "recognize" || body.action === "job") { busy = false; jobId = ""; } }
                else if (callback) callback(reply);
            } catch (e) { message = "Invalid service response: " + e; busy = false; jobId = ""; }
        };
        xhr.send(JSON.stringify(body));
    }
    function openImage(path) {
        if (busy || !path) return;
        showingSearch = false; result = null; chosen = ({}); textOutput.text = "";
        busy = true; progress = 0; message = "Queued";
        call({action: "recognize", path: path, languages: language}, function(reply) { jobId = reply.id; });
    }
    function acceptResult(value) {
        result = value; chosen = ({}); textOutput.text = value.text;
        message = value.words.length ? value.words.length + " words · tap regions to select" : "No text found. Try a clearer image.";
    }
    function selectedText() {
        if (!result) return "";
        var text = "", line = "";
        for (var i = 0; i < result.words.length; i++) {
            var word = result.words[i];
            if (chosen[i]) { if (text) text += line === word.line ? " " : "\n"; text += word.text; line = word.line; }
        }
        return text;
    }
    function toggleWord(index) {
        var next = {}; for (var key in chosen) next[key] = chosen[key];
        next[index] = !next[index]; chosen = next;
    }
    function copyAll() { textOutput.selectAll(); textOutput.copy(); textOutput.deselect(); message = "Text copied."; }
    function copySelected() {
        var selected = selectedText();
        if (selected) { clipboardText.text = selected; clipboardText.selectAll(); clipboardText.copy(); }
        else if (textOutput.selectedText) textOutput.copy();
        else { message = "Tap image regions or select text first."; return; }
        message = "Selection copied.";
    }
    function share() {
        var text = selectedText() || textOutput.selectedText || textOutput.text;
        if (!text) return;
        call({action: "export", text: text}, function(reply) { if (hub.item) hub.item.shareText(reply.url, text); });
    }
    Component.onCompleted: {
        if (!preview) {
            call({action: "settings"}, function(reply) { indexEnabled = reply.index_enabled; });
            if (initialPath) openImage(initialPath);
        }
    }
    Timer {
        interval: 450; running: root.jobId !== ""; repeat: true
        onTriggered: root.call({action: "job", id: root.jobId}, function(job) {
            root.progress = job.progress; root.message = job.stage;
            if (job.status === "done") { root.jobId = ""; root.busy = false; root.acceptResult(job.result); if (job.warning) root.message = job.warning; }
            else if (job.status === "failed" || job.status === "canceled") { root.jobId = ""; root.busy = false; root.message = job.error || "Canceled"; }
        })
    }
    TextEdit { textFormat: TextEdit.PlainText; id: clipboardText; visible: false }
    Page {
        anchors.fill: parent
        header: PageHeader { title: "Text from images" }
        Flickable {
            anchors { fill: parent; topMargin: units.gu(7); leftMargin: units.gu(2); rightMargin: units.gu(2); bottomMargin: units.gu(1) }
            contentHeight: content.height + units.gu(2); clip: true
            Column {
                id: content; width: parent.width; spacing: units.gu(1.5)
                Row {
                    spacing: units.gu(1)
                    Rectangle { width: units.gu(1); height: width; radius: width/2; color: root.accent; anchors.verticalCenter: parent.verticalCenter }
                    Label { textFormat: Text.PlainText; text: "OFFLINE · ENGLISH + ROMÂNĂ"; color: root.accent; fontSize: "small" }
                }
                Row {
                    width: parent.width; spacing: units.gu(1)
                    Button { width: (parent.width - units.gu(1))/2; text: "Open image"; color: root.accent; enabled: !root.busy && hub.status === Loader.Ready; onClicked: hub.item.openImage() }
                    Button { width: (parent.width - units.gu(1))/2; text: "Latest screenshot"; enabled: !root.busy; onClicked: root.call({action: "latest"}, function(reply) { root.openImage(reply.path); }) }
                }
                Row {
                    width: parent.width; spacing: units.gu(1)
                    TextField { id: pathInput; width: parent.width - loadButton.width - units.gu(1); placeholderText: "Local image path or file:// URL"; onAccepted: root.openImage(text) }
                    Button { id: loadButton; text: "Read"; enabled: !root.busy && pathInput.text.length > 0; onClicked: root.openImage(pathInput.text) }
                }
                Row {
                    width: parent.width; spacing: units.gu(1)
                    Button { width: (parent.width - units.gu(2))/3; text: "EN + RO"; color: root.language === "eng+ron" ? root.accent : "#e4e7e8"; enabled: !root.busy; onClicked: root.language = "eng+ron" }
                    Button { width: (parent.width - units.gu(2))/3; text: "English"; color: root.language === "eng" ? root.accent : "#e4e7e8"; enabled: !root.busy; onClicked: root.language = "eng" }
                    Button { width: (parent.width - units.gu(2))/3; text: "Română"; color: root.language === "ron" ? root.accent : "#e4e7e8"; enabled: !root.busy; onClicked: root.language = "ron" }
                }
                Label { textFormat: Text.PlainText; width: parent.width; text: root.message; wrapMode: Text.Wrap; fontSize: "small"; color: "#4c5963" }
                Row {
                    visible: root.busy; width: parent.width; spacing: units.gu(1)
                    ProgressBar { width: parent.width - cancelButton.width - units.gu(1); minimumValue: 0; maximumValue: 100; value: root.progress; anchors.verticalCenter: parent.verticalCenter }
                    Button { id: cancelButton; text: "Cancel"; enabled: root.jobId !== ""; onClicked: root.call({action: "cancel", id: root.jobId}, function() { root.message = "Canceling…"; }) }
                }
                Rectangle {
                    width: parent.width; height: root.result ? units.gu(26) : units.gu(22)
                    radius: units.gu(1); color: "#e8edef"; clip: true; visible: !root.showingSearch
                    Image {
                        id: photo; anchors.fill: parent; anchors.margins: units.gu(1)
                        source: root.result ? root.result.image_url : ""; fillMode: Image.PreserveAspectFit
                        autoTransform: true; asynchronous: true; cache: false
                        sourceSize.width: 1200; sourceSize.height: 1200
                        Item {
                            x: (photo.width - photo.paintedWidth)/2; y: (photo.height - photo.paintedHeight)/2
                            width: photo.paintedWidth; height: photo.paintedHeight
                            Repeater {
                                model: root.result ? root.result.words : []
                                Rectangle {
                                    x: modelData.x / root.result.width * parent.width
                                    y: modelData.y / root.result.height * parent.height
                                    width: Math.max(3, modelData.width / root.result.width * parent.width)
                                    height: Math.max(3, modelData.height / root.result.height * parent.height)
                                    color: root.chosen[index] ? "#66087f82" : "#10087f82"
                                    border.color: root.chosen[index] ? root.accent : "#80087f82"
                                    border.width: root.chosen[index] ? 2 : 1
                                    MouseArea { anchors.fill: parent; onClicked: root.toggleWord(index) }
                                }
                            }
                        }
                    }
                    Column {
                        visible: !root.result; anchors.centerIn: parent; spacing: units.gu(1)
                        Label { textFormat: Text.PlainText; text: "Aa"; font.pixelSize: units.gu(6); color: root.accent; anchors.horizontalCenter: parent.horizontalCenter }
                        Label { textFormat: Text.PlainText; text: "A photo. A screenshot. Your words."; fontSize: "small"; color: "#4c5963" }
                    }
                }
                Column {
                    visible: root.result !== null && !root.showingSearch; width: parent.width; spacing: units.gu(1)
                    Label { textFormat: Text.PlainText; text: "RECOGNIZED TEXT"; fontSize: "small"; color: root.accent }
                    TextArea { textFormat: TextEdit.PlainText; id: textOutput; objectName: "recognizedText"; width: parent.width; height: units.gu(16); placeholderText: "Recognized text appears here" }
                    Row {
                        width: parent.width; spacing: units.gu(1)
                        Button { width: (parent.width - units.gu(2))/3; text: "Copy all"; onClicked: root.copyAll() }
                        Button { width: (parent.width - units.gu(2))/3; text: "Copy selected"; onClicked: root.copySelected() }
                        Button { width: (parent.width - units.gu(2))/3; text: "Share"; enabled: hub.status === Loader.Ready; onClicked: root.share() }
                    }
                }
                Rectangle { width: parent.width; height: 1; color: "#d6dcdf" }
                Row {
                    width: parent.width; spacing: units.gu(1)
                    Column {
                        width: parent.width - indexSwitch.width - units.gu(1)
                        Label { textFormat: Text.PlainText; text: "Remember text on this device"; fontSize: "medium" }
                        Label { textFormat: Text.PlainText; text: "Optional. Turning off deletes the text index."; fontSize: "small"; color: "#64717a"; width: parent.width; wrapMode: Text.Wrap }
                    }
                    Switch {
                        id: indexSwitch; checked: root.indexEnabled
                        onClicked: {
                            var requested = checked;
                            // Keep the confirmed setting visible until the durable write succeeds.
                            checked = root.indexEnabled;
                            root.call({action: "index", enabled: requested}, function() {
                                root.indexEnabled = requested; indexSwitch.checked = requested;
                                if (!requested) { root.hits = []; root.showingSearch = false; }
                            });
                        }
                    }
                }
                Row {
                    width: parent.width; spacing: units.gu(1); visible: root.indexEnabled
                    TextField { id: searchInput; width: parent.width - searchButton.width - units.gu(1); placeholderText: "Search saved screenshot text"; onAccepted: searchButton.clicked() }
                    Button { id: searchButton; text: "Search"; onClicked: root.call({action: "search", query: searchInput.text}, function(reply) { root.hits = reply; root.showingSearch = true; root.message = reply.length + " saved matches"; }) }
                }
                Repeater {
                    model: root.showingSearch ? root.hits : []
                    Rectangle {
                        width: content.width; height: hitText.height + units.gu(3); color: "white"; radius: units.gu(0.5)
                        Label { textFormat: Text.PlainText; id: hitText; anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1.5) } text: modelData.text.substring(0, 180) + "\n" + modelData.path.split("/").pop(); wrapMode: Text.Wrap; maximumLineCount: 5; elide: Text.ElideRight }
                        MouseArea { anchors.fill: parent; onClicked: root.openImage(modelData.path) }
                    }
                }
                Label { textFormat: Text.PlainText; width: parent.width; text: "No cloud. No account. No automatic image scanning."; fontSize: "x-small"; color: "#64717a"; wrapMode: Text.Wrap }
            }
        }
    }
    Loader {
        id: hub; anchors.fill: parent; active: !root.preview; source: "ContentActions.qml"
        onLoaded: { item.imageReceived.connect(root.openImage); item.failed.connect(function(error) { root.message = error; }); }
        onStatusChanged: if (status === Loader.Error) root.message = "Content Hub is unavailable. Open a local path instead.";
    }
}
