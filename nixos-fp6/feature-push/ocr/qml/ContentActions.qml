import QtQuick 2.12
import Lomiri.Content 1.3

Item {
    id: content
    visible: picker.visible
    property var transfer: null
    property var consumedTransfer: null
    property bool sharing: false
    property string shareUrl: ""
    property string shareValue: ""
    signal imageReceived(string path)
    signal failed(string error)
    function receive(value) { transfer = value; sharing = false; collect(); }
    function collect() {
        if (transfer && !sharing && transfer !== consumedTransfer &&
                (transfer.state === ContentTransfer.Charged || transfer.state === ContentTransfer.Collected) && transfer.items.length) {
            consumedTransfer = transfer;
            imageReceived(transfer.items[0].url.toString());
            // Keep transfer alive until the next import/window close; OCR snapshots it asynchronously.
        }
    }
    function openImage() { sharing = false; picker.contentType = ContentType.Pictures; picker.handler = ContentHandler.Source; picker.visible = true; }
    function shareText(url, text) { sharing = true; shareUrl = url; shareValue = text; picker.contentType = ContentType.Text; picker.handler = ContentHandler.Share; picker.visible = true; }
    ContentStore { id: store; scope: ContentScope.App }
    Component { id: itemFactory; ContentItem {} }
    Connections {
        target: ContentHub
        function onImportRequested(transfer) { content.receive(transfer); }
        function onShareRequested(transfer) { content.receive(transfer); }
    }
    Connections {
        target: content.transfer
        function onItemsChanged() { content.collect(); }
        function onStateChanged() {
            if (content.transfer.state === ContentTransfer.Aborted) content.failed("Content transfer was canceled.");
            content.collect();
        }
    }
    ContentPeerPicker {
        id: picker; visible: false; anchors.fill: parent; contentType: ContentType.Pictures; handler: ContentHandler.Source
        onPeerSelected: {
            peer.selectionType = ContentTransfer.Single;
            peer.contentType = contentType;
            peer.handler = handler;
            content.transfer = peer.request(store);
            if (content.sharing) {
                content.transfer.items = [itemFactory.createObject(content, {url: content.shareUrl, text: content.shareValue})];
                content.transfer.state = ContentTransfer.Charged;
            }
            visible = false;
            content.collect();
        }
        onCancelPressed: visible = false
    }
}
