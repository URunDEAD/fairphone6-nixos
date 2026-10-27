import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Content 1.3

MainView {
    id: root
    objectName: "webshareMain"
    applicationName: "fp6-webshare"
    width: units.gu(48)
    height: units.gu(92)
    backgroundColor: "#f2f5f3"
    property string controlUrl: ""
    property string controlKey: ""
    property bool previewMode: false
    property var selectedFiles: []
    property var networks: []
    property var stateData: ({ active: false, pending: [], files: [], qr: [], text: null })
    property var transfer: null
    property string notice: ""
    property bool busy: false
    property bool refreshing: false
    property bool active: !!stateData.active

    function api(action, args, done) {
        if (previewMode) return;
        var xhr = new XMLHttpRequest();
        xhr.open("POST", controlUrl);
        xhr.setRequestHeader("Content-Type", "application/json");
        xhr.setRequestHeader("X-WebShare-Key", controlKey);
        xhr.onreadystatechange = function () {
            if (xhr.readyState !== XMLHttpRequest.DONE) return;
            var reply;
            try { reply = JSON.parse(xhr.responseText); }
            catch (e) { reply = { ok: false, error: "Service unavailable. Close and reopen Share via link." }; }
            if (!reply.ok) notice = reply.error;
            if (done) done(reply.ok, reply.value);
        };
        xhr.send(JSON.stringify({ action: action, args: args === undefined ? null : args }));
    }
    function refresh() {
        if (refreshing || previewMode) return;
        refreshing = true;
        api("status", null, function (ok, value) {
            refreshing = false;
            if (ok) stateData = value;
        });
    }
    function scanNetworks() {
        api("interfaces", null, function (ok, value) {
            if (ok) {
                networks = value;
                if (!value.length) notice = "Connect to Wi-Fi or turn on your phone hotspot, then refresh networks.";
            }
        });
    }
    function collectTransfer() {
        if (!transfer || transfer.state !== ContentTransfer.Charged) return;
        var paths = [];
        for (var i = 0; i < transfer.items.length; i++) {
            var url = transfer.items[i].url.toString();
            if (url.indexOf("file:///") !== 0) {
                notice = "Only local files can be shared.";
                transfer.finalize(); transfer = null; return;
            }
            try { paths.push(decodeURIComponent(url.substring(7))); }
            catch (e) { notice = "Invalid file URL"; return; }
        }
        selectedFiles = paths;
        notice = "Review the selected files, then start sharing.";
    }
    function acceptTransfer(value) {
        if (active || busy || transfer) {
            notice = "Finish the current selection or share first.";
            value.finalize(); return;
        }
        transfer = value;
        collectTransfer();
    }
    Component.onCompleted: { if (!previewMode) { scanNetworks(); refresh(); } }
    Component.onDestruction: { if (transfer) transfer.finalize(); }
    Connections {
        target: root.previewMode ? null : ContentHub
        function onShareRequested(transfer) { root.acceptTransfer(transfer); }
        function onImportRequested(transfer) { root.acceptTransfer(transfer); }
    }
    Connections { target: root.transfer; function onStateChanged() { root.collectTransfer(); } }
    Timer { interval: 1500; running: !root.previewMode; repeat: true; onTriggered: root.refresh() }

    Page {
        id: page
        Rectangle { anchors.fill: parent; color: root.backgroundColor; z: -1 }
        header: PageHeader { title: "Share via link" }
        Flickable {
            anchors { top: page.header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            contentHeight: content.height + units.gu(4)
            clip: true
            Column {
                id: content
                anchors { left: parent.left; right: parent.right; margins: units.gu(2.5) }
                y: units.gu(2)
                spacing: units.gu(2)
                Label { text: root.active ? "YOUR PHONE, CONNECTED" : "NEARBY SHARING"; color: "#49685b"; fontSize: "small" }
                Label {
                    width: parent.width; text: root.active ? "Scan. Connect. Share." : "A link for any device."
                    fontSize: "x-large"; wrapMode: Text.WordWrap
                }
                Label {
                    width: parent.width; wrapMode: Text.WordWrap
                    text: "iPhone, Android or computer — on the same Wi-Fi or your phone hotspot."
                }
                Rectangle {
                    width: parent.width; height: warning.height + units.gu(3); radius: units.gu(1)
                    color: "#fff1ce"
                    Label {
                        id: warning; anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1.5) }
                        text: "HTTP is not encrypted. Use trusted Wi-Fi or a private hotspot. Anyone with an unused link can connect."
                        wrapMode: Text.WordWrap; color: "#604916"; fontSize: "small"
                    }
                }
                Column {
                    visible: !root.active; width: parent.width; spacing: units.gu(1.5)
                    Label { text: "1  Choose what to share"; font.bold: true }
                    Repeater {
                        model: root.selectedFiles
                        Label { width: content.width; text: "• " + modelData.split("/").pop(); textFormat: Text.PlainText; wrapMode: Text.WrapAnywhere }
                    }
                    Button { width: parent.width; text: root.selectedFiles.length ? "Change files" : "Choose files"; enabled: !root.busy; onClicked: { if (root.transfer) { root.transfer.finalize(); root.transfer = null; } pickerLoader.item.visible = true; } }
                    Button { visible: root.selectedFiles.length > 0; width: parent.width; text: "Clear selection"; onClicked: { root.selectedFiles = []; if (root.transfer) { root.transfer.finalize(); root.transfer = null; } } }
                    Label { text: "2  Choose the network"; font.bold: true }
                    OptionSelector { id: networkChoice; width: parent.width; model: root.networks.map(function (n) { return n.name + " · " + n.address; }); selectedIndex: 0 }
                    Button { width: parent.width; text: "Refresh networks"; onClicked: root.scanNetworks() }
                    Label { text: "Stop automatically after (minutes)" }
                    TextField { id: minutes; width: parent.width; text: "10"; inputMethodHints: Qt.ImhDigitsOnly; validator: IntValidator { bottom: 1; top: 30 } }
                    Row {
                        width: parent.width; spacing: units.gu(1.5)
                        Switch { id: uploads; checked: false }
                        Label { text: "Allow uploads · approval required"; width: parent.width - uploads.width - units.gu(2); wrapMode: Text.WordWrap }
                    }
                    Row {
                        width: parent.width; spacing: units.gu(1.5)
                        Switch { id: texts; checked: false }
                        Label { text: "Allow text · tap Copy to clipboard"; width: parent.width - texts.width - units.gu(2); wrapMode: Text.WordWrap }
                    }
                    Button {
                        width: parent.width; text: root.busy ? "Starting…" : "Start sharing"; color: "#1c7058"
                        enabled: !root.busy && root.networks.length > 0 && minutes.acceptableInput && (root.selectedFiles.length > 0 || uploads.checked || texts.checked)
                        onClicked: {
                            root.busy = true; root.notice = "";
                            root.api("start", { files: root.selectedFiles, interface: root.networks[networkChoice.selectedIndex].name, minutes: Number(minutes.text), uploads: uploads.checked, text: texts.checked }, function (ok, value) {
                                root.busy = false;
                                if (ok) { root.stateData = value; root.selectedFiles = []; if (root.transfer) { root.transfer.finalize(); root.transfer = null; } }
                            });
                        }
                    }
                }
                Column {
                    visible: root.active; width: parent.width; spacing: units.gu(1.5)
                    Rectangle {
                        width: parent.width; height: qr.height + units.gu(3); radius: units.gu(1); color: "white"
                        Canvas {
                            id: qr
                            anchors { top: parent.top; horizontalCenter: parent.horizontalCenter; topMargin: units.gu(1.5) }
                            width: Math.min(content.width - units.gu(3), units.gu(30)); height: width
                            property var matrix: root.stateData.qr || []
                            onMatrixChanged: requestPaint()
                            onWidthChanged: requestPaint()
                            onPaint: {
                                var ctx = getContext("2d"); ctx.fillStyle = "white"; ctx.fillRect(0, 0, width, height);
                                if (!matrix.length) return;
                                var cell = Math.floor(width / (matrix.length + 8));
                                var offset = Math.floor((width - matrix.length * cell) / 2);
                                ctx.fillStyle = "#000000";
                                for (var y = 0; y < matrix.length; y++) for (var x = 0; x < matrix.length; x++)
                                    if (matrix[y][x]) ctx.fillRect(offset + x * cell, offset + y * cell, cell, cell);
                            }
                        }
                    }
                    Label { width: parent.width; text: root.stateData.url || ""; textFormat: Text.PlainText; wrapMode: Text.WrapAnywhere; horizontalAlignment: Text.AlignHCenter; fontSize: "small" }
                    Label { width: parent.width; text: root.stateData.claimed ? "Connected · link already used" : "Waiting for one browser"; horizontalAlignment: Text.AlignHCenter; color: "#1c7058"; font.bold: true }
                    Label { width: parent.width; text: (root.stateData.interface || "") + " · " + Math.ceil((root.stateData.seconds_left || 0) / 60) + " min remaining"; horizontalAlignment: Text.AlignHCenter; textFormat: Text.PlainText }
                    Button { width: parent.width; text: "Copy link"; onClicked: Clipboard.push(root.stateData.url) }
                    Repeater {
                        model: root.stateData.files || []
                        Label { width: content.width; text: (modelData.downloaded ? "✓ " : "↓ ") + modelData.name; textFormat: Text.PlainText; wrapMode: Text.WrapAnywhere }
                    }
                    Repeater {
                        model: root.stateData.pending || []
                        Column {
                            width: content.width; spacing: units.gu(1)
                            Label { text: "Upload needs your approval"; font.bold: true }
                            Label { width: parent.width; text: modelData.name + " · " + modelData.size + " bytes"; textFormat: Text.PlainText; wrapMode: Text.WrapAnywhere }
                            Row {
                                width: parent.width; spacing: units.gu(1)
                                Button { width: (parent.width - units.gu(1)) / 2; text: "Keep file"; color: "#1c7058"; enabled: !root.busy; onClicked: { root.busy = true; root.api("approve", modelData.id, function (ok, path) { root.busy = false; if (ok) root.notice = "Saved to " + path; root.refresh(); }); } }
                                Button { width: (parent.width - units.gu(1)) / 2; text: "Reject"; enabled: !root.busy; onClicked: root.api("reject", modelData.id, function () { root.refresh(); }) }
                            }
                        }
                    }
                    Column {
                        visible: !!root.stateData.text; width: parent.width; spacing: units.gu(1)
                        Label { text: "Received text"; font.bold: true }
                        Label { width: parent.width; text: root.stateData.text || ""; textFormat: Text.PlainText; wrapMode: Text.WrapAnywhere }
                        Button { width: parent.width; text: "Copy received text"; onClicked: root.api("takeText", null, function (ok, value) { if (ok) { Clipboard.push(value); root.notice = "Copied to clipboard"; root.refresh(); } }) }
                    }
                    Button { width: parent.width; text: "Stop sharing"; color: LomiriColors.red; onClicked: root.api("stop", null, function () { root.refresh(); }) }
                }
                Label { visible: root.notice.length > 0; width: parent.width; text: root.notice; textFormat: Text.PlainText; wrapMode: Text.WrapAnywhere; color: "#6b4f20" }
                Label { width: parent.width; text: "No account. No cloud. No telemetry."; color: "#60776e"; fontSize: "small"; horizontalAlignment: Text.AlignHCenter }
            }
        }
        Loader {
            id: pickerLoader; anchors.fill: parent; active: !root.previewMode
            sourceComponent: Component {
                ContentPeerPicker {
                    id: picker; anchors.fill: parent; visible: false
                    contentType: ContentType.All; handler: ContentHandler.Source
                    onPeerSelected: {
                        peer.selectionType = ContentTransfer.Multiple;
                        root.acceptTransfer(peer.request()); picker.visible = false;
                    }
                    onCancelPressed: visible = false
                }
            }
        }

    }
}
