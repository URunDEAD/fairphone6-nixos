import QtQuick 2.12
import Lomiri.Components 1.3

MainView {
    id: root
    applicationName: "com.fairphone6.Share"
    backgroundColor: "#f6f6f6"
    property string lastSnapshot: ""
    property var state: ({revision: 0, request: null, targets: [], recent: [], busy: false})
    property string message: ""
    property string printTarget: ""
    property string ocrId: ""
    property string quickId: ""
    property string ocrText: ""
    property bool commandPending: false
    property string consumedEvent: ""
    property var request: state.request
    property var targets: state.targets || []
    property bool busy: commandPending || state.busy
    property string contentType: state.content_type || "documents"
    signal requestOpened()
    function refresh() {
        shareBridge.poll()
        try { if (shareBridge.snapshot !== lastSnapshot) { var next = JSON.parse(shareBridge.snapshot); var opening = next.request && next.revision !== state.revision; state = next; lastSnapshot = shareBridge.snapshot; if (opening) requestOpened() } } catch (error) { message = "Connecting to share service…" }
        if (shareBridge.error.length) { message = shareBridge.error; commandPending = false; ocrId = ""; quickId = "" }
        if (shareBridge.event.length) {
            try {
                var event = JSON.parse(shareBridge.event)
                commandPending = false
                if (event.kind === "duplicate") Qt.quit()
                else if (event.kind === "copy") { Clipboard.push({text: event.value}); message = "Copied to clipboard" }
                else if (event.kind === "message") message = event.value
                else if (event.kind === "content") contentLoader.item.send(event.value, request, contentType)
                else if (event.kind === "quick") { quickId = event.value; message = "Connecting with Quick Share…" }
                else if (event.kind === "quick_status") {
                    var transfer = event.value
                    message = "Quick Share: " + transfer.status + (transfer.pin ? ". Compare PIN " + transfer.pin + " on both devices before accepting." : "")
                    if (["finished", "rejected", "cancelled", "failed"].indexOf(transfer.status) >= 0) quickId = ""
                }
                else if (event.kind === "ocr") { ocrId = event.value; message = "Recognizing text on this device…" }
                else if (event.kind === "ocr_status") {
                    var job = event.value
                    if (job.status === "done") {
                        ocrId = ""
                        ocrText = job.result ? (typeof job.result === "string" ? job.result : job.result.text || "") : ""
                        message = ocrText.length ? "Text recognized. Review and copy below." : "No text found."
                    } else if (job.status === "failed" || job.status === "canceled") { ocrId = ""; message = job.error || "Recognition canceled" }
                }
            } catch (error) { message = "Could not complete action: " + error }
        }
    }
    function selectTarget(target) {
        if (previewMode || busy) return
        if (target.id === "action:print") { printTarget = target.id; return }
        commandPending = true
        message = "Opening " + target.label + "…"
        shareBridge.command("Select", state.revision, target.id, "")
    }
    function filename(uri) {
        try { return decodeURIComponent(uri.substring(uri.lastIndexOf('/') + 1)) } catch (e) { return "Shared file" }
    }
    function forGroup(group) {
        return targets.filter(function(t) {
            return group === "Nearby" ? (t.provider === "quick" || t.provider === "kde" || t.provider === "web") : group === "Apps" ? t.provider === "content" : t.provider === "action"
        })
    }
    Component.onCompleted: refresh()
    Timer { interval: 400; running: !previewMode; repeat: true; onTriggered: root.refresh() }
    Timer { interval: 900; running: root.ocrId.length > 0 && !previewMode; repeat: true; onTriggered: shareBridge.command("OcrJob", root.state.revision, root.ocrId, "") }
    Timer { interval: 900; running: root.quickId.length > 0 && !previewMode; repeat: true; onTriggered: shareBridge.command("QuickJob", root.state.revision, root.quickId, "") }
    Loader {
        id: contentLoader
        active: !previewMode
        source: "ContentBridge.qml"
        onLoaded: {
            item.received.connect(function(items, mime, source) {
                root.commandPending = false
                shareBridge.command("Share", 0, JSON.stringify({items: items, mime: mime, source_app: source || "Content Hub"}), "")
            })
            item.finished.connect(function(text) { root.message = text })
        }
    }
    Page {
        anchors.fill: parent
        header: PageHeader { title: "Share"; trailingActionBar.actions: [Action { text: "Close"; iconName: "close"; onTriggered: Qt.quit() }] }
        Flickable {
            anchors { fill: parent; topMargin: units.gu(7) }
            contentHeight: body.height + units.gu(4)
            clip: true
            Column {
                id: body
                width: parent.width - units.gu(4)
                anchors.horizontalCenter: parent.horizontalCenter
                spacing: units.gu(2)
                Item { width: 1; height: units.gu(0.5) }
                Rectangle {
                    width: parent.width; height: previewColumn.height + units.gu(4); radius: units.gu(2); color: "white"
                    Column {
                        id: previewColumn
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                        spacing: units.gu(1)
                        Label { text: root.request ? "FROM " + root.request.source_app.toUpperCase() : "SYSTEM SHARE"; textFormat: Text.PlainText; color: "#777777"; fontSize: "small"; width: parent.width; elide: Text.ElideRight }
                        Label { text: root.request ? root.request.items[0].indexOf("file:") === 0 ? root.filename(root.request.items[0]) + (root.request.items.length > 1 ? " + " + (root.request.items.length - 1) + " more" : "") : root.request.items.join("\n") : "Choose Share in an app, or use fp6-share FILE…"; textFormat: Text.PlainText; fontSize: "large"; width: parent.width; wrapMode: Text.Wrap; maximumLineCount: 4; elide: Text.ElideRight }
                        Label { text: root.request ? root.request.items.length + (root.request.items.length === 1 ? " item · " : " items · ") + root.request.mime : "Private sharing on your phone"; textFormat: Text.PlainText; color: "#666666"; width: parent.width; wrapMode: Text.Wrap; fontSize: "small" }
                    }
                }
                Label { visible: root.message.length > 0; width: parent.width; text: root.message; textFormat: Text.PlainText; color: "#65477e"; wrapMode: Text.Wrap }
                Button { visible: root.quickId.length > 0; text: "Cancel transfer"; onClicked: { shareBridge.command("CancelJob", root.state.revision, root.quickId, "quick"); root.quickId = ""; root.message = "Canceling transfer…" } }
                Button { visible: root.ocrId.length > 0; text: "Cancel recognition"; onClicked: { shareBridge.command("CancelJob", root.state.revision, root.ocrId, "ocr"); root.ocrId = ""; root.message = "Canceling recognition…" } }
                Label { visible: root.state.recent && root.state.recent.length > 0; text: "Recent"; fontSize: "large"; color: "#343434" }
                Row {
                    visible: root.state.recent && root.state.recent.length > 0
                    width: parent.width
                    Flickable {
                        width: parent.width; height: units.gu(17); contentWidth: recentRow.width; clip: true
                        Row {
                            id: recentRow; spacing: units.gu(1.5)
                            Repeater {
                                model: root.state.recent || []
                                Rectangle {
                                    width: units.gu(17); height: units.gu(16); color: "#e9e4f0"; radius: units.gu(1.5)
                                    Column {
                                        anchors { fill: parent; margins: units.gu(1.5) } spacing: units.gu(1)
                                        Label { text: "●"; color: "#6c478b"; fontSize: "x-large" }
                                        Label { width: parent.width; text: modelData.label; textFormat: Text.PlainText; wrapMode: Text.Wrap; maximumLineCount: 2; elide: Text.ElideRight; fontSize: "medium" }
                                        Label { text: modelData.provider === "quick" ? "Quick Share" : "KDE Connect"; color: "#675877"; fontSize: "small" }
                                    }
                                    MouseArea { anchors.fill: parent; enabled: !root.busy; onClicked: root.selectTarget(modelData) }
                                }
                            }
                        }
                    }
                }
                Repeater {
                    model: ["Nearby", "Apps", "Actions"]
                    Column {
                        property string group: modelData
                        property var rows: root.forGroup(group)
                        width: body.width; spacing: units.gu(1)
                        visible: rows.length > 0
                        Label { text: group; fontSize: "large"; color: "#343434" }
                        Repeater {
                            model: rows
                            Rectangle {
                                width: body.width; height: Math.max(units.gu(8), details.height + units.gu(2.5)); radius: units.gu(1); color: "white"
                                Rectangle {
                                    anchors { left: parent.left; leftMargin: units.gu(1.5); verticalCenter: parent.verticalCenter }
                                    width: units.gu(4.5); height: width; radius: width / 2
                                    color: modelData.provider === "action" ? "#e7f0e8" : "#ede5f4"
                                    Label { anchors.centerIn: parent; text: modelData.provider === "web" ? "↗" : modelData.provider === "action" ? "◇" : modelData.label.substring(0, 1); textFormat: Text.PlainText; color: "#65477e"; fontSize: "large" }
                                }
                                Column {
                                    id: details
                                    anchors { left: parent.left; leftMargin: units.gu(7.5); right: parent.right; rightMargin: units.gu(3); verticalCenter: parent.verticalCenter }
                                    spacing: units.gu(0.4)
                                    Label { width: parent.width; text: modelData.label; textFormat: Text.PlainText; fontSize: "medium"; elide: Text.ElideRight }
                                    Label { width: parent.width; text: modelData.detail; textFormat: Text.PlainText; color: "#777777"; fontSize: "small"; wrapMode: Text.Wrap }
                                }
                                MouseArea { anchors.fill: parent; enabled: !root.busy; onClicked: root.selectTarget(modelData) }
                            }
                        }
                    }
                }
                Label { visible: root.request && !root.forGroup("Nearby").length; width: parent.width; text: "No nearby devices available. Open the sharing app on the other device."; color: "#777777"; wrapMode: Text.Wrap }
                Column {
                    visible: root.printTarget.length > 0; width: parent.width; spacing: units.gu(1)
                    Label { text: "Send to the default CUPS printer?"; width: parent.width; wrapMode: Text.Wrap }
                    Button { text: "Print now"; onClicked: { root.commandPending = true; shareBridge.command("Select", root.state.revision, root.printTarget, "confirm"); root.printTarget = "" } }
                    Button { text: "Cancel print"; onClicked: root.printTarget = "" }
                }
                TextArea { id: recognized; visible: root.ocrText.length > 0; width: parent.width; text: root.ocrText; autoSize: true; maximumLineCount: 8 }
                Button { visible: root.ocrText.length > 0; text: "Copy recognized text"; onClicked: { Clipboard.push({text: recognized.text}); root.message = "Copied recognized text" } }
                Button { visible: root.state.recent && root.state.recent.length > 0; text: "Clear recent targets"; onClicked: shareBridge.command("ClearHistory", root.state.revision, "", "") }
                Label { width: parent.width; text: "Only the target you choose receives these items."; color: "#888888"; fontSize: "small"; wrapMode: Text.Wrap }
            }
        }
    }
}
