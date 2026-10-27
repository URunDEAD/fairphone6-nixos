import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import Lomiri.Content 1.3

MainView {
    id: root
    objectName: "quickShareMain"
    applicationName: "fp6-quickshare"
    width: units.gu(54)
    height: units.gu(102)
    property bool sending: quickShare.startOnSend
    property var seenRequests: ({})
    property var activeTransfer: null

    function reviewRequests() {
        for (var i = 0; i < quickShare.transfers.length; ++i) {
            var transfer = quickShare.transfers[i]
            if (transfer.status === "waiting" && !seenRequests[transfer.id]) {
                seenRequests[transfer.id] = true
                PopupUtils.open(consentSheet, root, { "transfer": transfer })
            }
        }
    }
    function importItems(transfer) {
        var uris = []
        for (var i = 0; i < transfer.items.length; ++i) {
            var item = transfer.items[i]
            if (item.url && item.url.toString()) {
                var value = item.url.toString()
                uris.push(value.indexOf("http:") === 0 || value.indexOf("https:") === 0 ? "url:" + value : value)
            } else if (item.text) uris.push("text:" + item.text)
        }
        quickShare.setUris(uris)
        root.sending = true
    }
    Connections {
        target: quickShare
        function onChanged() { root.reviewRequests() }
    }
    Connections {
        target: quickShare.preview ? null : ContentHub
        function onShareRequested(transfer) {
            root.activeTransfer = transfer
            root.importItems(transfer)
            if (transfer.state === ContentTransfer.Charged) transfer.state = ContentTransfer.Collected
        }
        function onImportRequested(transfer) {
            root.activeTransfer = transfer
            if (transfer.state === ContentTransfer.Charged) { root.importItems(transfer); transfer.state = ContentTransfer.Collected }
        }
    }
    Connections {
        target: root.activeTransfer
        ignoreUnknownSignals: true
        function onStateChanged() {
            if (root.activeTransfer.state === ContentTransfer.Charged) {
                root.importItems(root.activeTransfer)
                root.activeTransfer.state = ContentTransfer.Collected
                if (pageStack.depth > 1) pageStack.pop()
            }
        }
    }
    Timer { interval: 200; running: true; repeat: false; onTriggered: { root.reviewRequests(); if (root.sending) quickShare.discover() } }

    PageStack {
        id: pageStack
        anchors.fill: parent
        Component.onCompleted: push(mainPage)
    }
    Component {
        id: mainPage
        Page {
            header: PageHeader { title: "Quick Share" }
            Flickable {
                anchors { fill: parent; topMargin: units.gu(8) }
                contentHeight: content.height + units.gu(5)
                clip: true
                Column {
                    id: content
                    anchors { left: parent.left; right: parent.right; margins: units.gu(3) }
                    spacing: units.gu(2.5)
                    Row {
                        spacing: units.gu(1)
                        Button { text: "Receive"; color: !root.sending ? LomiriColors.orange : theme.palette.normal.background; width: (content.width - units.gu(1)) / 2; onClicked: root.sending = false }
                        Button { text: "Send"; color: root.sending ? LomiriColors.orange : theme.palette.normal.background; width: (content.width - units.gu(1)) / 2; onClicked: { root.sending = true; quickShare.discover() } }
                    }
                    Label { text: quickShare.deviceName; textFormat: Text.PlainText; fontSize: "x-large"; font.bold: true; width: parent.width; wrapMode: Text.Wrap }
                    Label { text: quickShare.error; textFormat: Text.PlainText; color: LomiriColors.red; visible: text.length > 0; width: parent.width; wrapMode: Text.Wrap }
                    Column {
                        visible: !root.sending
                        width: parent.width
                        spacing: units.gu(2)
                        LomiriShape {
                            width: parent.width
                            height: receiveOptions.height + units.gu(4)
                            color: theme.palette.normal.foreground
                            Column {
                                id: receiveOptions
                                anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                                spacing: units.gu(1.5)
                                Row {
                                    width: parent.width
                                    Label { text: "Visible to nearby devices"; width: parent.width - units.gu(8); wrapMode: Text.Wrap; font.bold: true }
                                    Switch { checked: quickShare.visibility !== "off"; onClicked: quickShare.setVisible(checked) }
                                }
                                Label { text: quickShare.visibility === "off" ? "Visibility is off" : quickShare.visibility === "everyone" ? "Visible until turned off" : "Visible for 10 minutes"; color: LomiriColors.orange; fontSize: "small" }
                            }
                        }
                        Label { text: "Keep both phones on the same Wi-Fi network. Compare the PIN on both screens before accepting."; width: parent.width; wrapMode: Text.Wrap; color: theme.palette.normal.backgroundSecondaryText }
                        Label { text: "Files arrive in Downloads / Quick Share"; width: parent.width; wrapMode: Text.Wrap; fontSize: "small" }
                    }
                    Column {
                        visible: root.sending
                        width: parent.width
                        spacing: units.gu(2)
                        Button { text: "Choose files"; width: parent.width; onClicked: pageStack.push(pickerPage) }
                        Repeater {
                            model: quickShare.selectedUris
                            Label { text: decodeURIComponent(modelData.split("/").pop()); textFormat: Text.PlainText; width: content.width; elide: Text.ElideMiddle }
                        }
                        Button { text: "Clear selection"; visible: quickShare.selectedUris.length > 0; onClicked: quickShare.clearSelection() }
                        TextArea { id: shareText; width: parent.width; height: units.gu(12); placeholderText: "Or enter text or a web link" }
                        CheckBox { id: asLink }
                        Label { text: "Send as a web link"; fontSize: "small" }
                        Row {
                            width: parent.width
                            Label { text: "Nearby devices"; font.bold: true; width: parent.width - units.gu(12); anchors.verticalCenter: parent.verticalCenter }
                            Button { text: "Refresh"; width: units.gu(12); onClicked: quickShare.discover() }
                        }
                        Label { text: "Open Quick Share → Receive on the Android phone if it does not appear."; width: parent.width; wrapMode: Text.Wrap; fontSize: "small" }
                        Repeater {
                            model: quickShare.devices
                            Button { text: modelData.name; width: content.width; enabled: quickShare.selectedUris.length > 0 || shareText.text.length > 0; onClicked: quickShare.send(modelData.id, shareText.text, asLink.checked) }
                        }
                    }
                    Label { text: "Transfers"; font.bold: true; fontSize: "large" }
                    Label { visible: quickShare.transfers.length === 0; text: "Ready when you are"; color: theme.palette.normal.backgroundSecondaryText }
                    Repeater {
                        model: quickShare.transfers
                        Column {
                            width: content.width
                            spacing: units.gu(1)
                            property var transfer: modelData
                            Label { text: transfer.sender; textFormat: Text.PlainText; font.bold: true; width: parent.width; elide: Text.ElideRight }
                            Label { text: transfer.files && transfer.files.length ? transfer.files.join(", ") : "Text or link"; textFormat: Text.PlainText; width: parent.width; wrapMode: Text.Wrap }
                            Label { text: transfer.status + (transfer.pin ? " · PIN " + transfer.pin : ""); textFormat: Text.PlainText; color: LomiriColors.orange }
                            ProgressBar { width: parent.width; visible: transfer.status === "receiving" || transfer.status === "sending"; minimumValue: 0; maximumValue: Math.max(1, transfer.total); value: transfer.bytes }
                            Label { visible: transfer.total > 0; text: transfer.bytes + " / " + transfer.total + " bytes"; fontSize: "small" }
                            Button { text: "Review request"; visible: transfer.status === "waiting"; onClicked: PopupUtils.open(consentSheet, root, { "transfer": transfer }) }
                            Button { text: "Cancel"; visible: ["connecting", "accepting", "sending", "receiving"].indexOf(transfer.status) >= 0; onClicked: quickShare.cancel(transfer.id) }
                            Button { text: "Open"; visible: transfer.status === "finished" && (transfer.directory || transfer.is_url); onClicked: quickShare.openTransfer(transfer) }
                            Button { text: "Copy text"; visible: transfer.status === "finished" && transfer.text; onClicked: quickShare.copyText(transfer.text) }
                            Rectangle { width: parent.width; height: units.dp(1); color: theme.palette.normal.base }
                        }
                    }
                }
            }
        }
    }
    Component {
        id: pickerPage
        Page {
            header: PageHeader { title: "Choose files" }
            ContentPeerPicker {
                anchors { fill: parent; topMargin: units.gu(8) }
                handler: ContentHandler.Source
                contentType: ContentType.All
                onPeerSelected: {
                    root.activeTransfer = peer.request()
                    root.activeTransfer.selectionType = ContentTransfer.Multiple
                }
                onCancelPressed: pageStack.pop()
            }
        }
    }
    Component {
        id: consentSheet
        Dialog {
            id: sheet
            property var transfer
            title: "Incoming Quick Share"
            Label { text: sheet.transfer.sender; textFormat: Text.PlainText; font.bold: true; width: parent.width; wrapMode: Text.Wrap }
            Label { text: sheet.transfer.files && sheet.transfer.files.length ? sheet.transfer.files.slice(0, 6).join("\n") + (sheet.transfer.files.length > 6 ? "\n… and " + (sheet.transfer.files.length - 6) + " more" : "") : "Text or link"; textFormat: Text.PlainText; width: parent.width; wrapMode: Text.Wrap }
            Label { text: sheet.transfer.pin; textFormat: Text.PlainText; fontSize: "x-large"; font.bold: true; horizontalAlignment: Text.AlignHCenter; width: parent.width }
            Label { text: "Check that this PIN matches the sender’s screen."; width: parent.width; wrapMode: Text.Wrap }
            Button { text: "Accept"; color: LomiriColors.green; onClicked: { quickShare.decide(sheet.transfer.id, true); PopupUtils.close(sheet) } }
            Button { text: "Decline"; onClicked: { quickShare.decide(sheet.transfer.id, false); PopupUtils.close(sheet) } }
        }
    }
}
