// Attach files/pictures from another app (Content Hub source picker).
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Content 1.3

Page {
    id: page
    signal picked(var paths)
    property var activeTransfer: null
    header: PageHeader {
        title: i18n.tr("Attach")
        leadingActionBar.actions: [Action { iconName: "back"; onTriggered: stack.pop() }]
    }
    ContentPeerPicker {
        anchors { top: page.header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
        showTitle: false
        contentType: ContentType.All
        handler: ContentHandler.Source
        onPeerSelected: {
            peer.selectionType = ContentTransfer.Multiple
            page.activeTransfer = peer.request()
        }
        onCancelPressed: stack.pop()
    }
    Connections {
        target: page.activeTransfer
        ignoreUnknownSignals: true
        function onStateChanged() {
            var t = page.activeTransfer
            if (t && t.state === ContentTransfer.Charged) {
                var out = []
                for (var i = 0; i < t.items.length; i++) out.push(String(t.items[i].url).replace("file://", ""))
                page.picked(out)
                stack.pop()
            }
        }
    }
    ContentTransferHint { anchors.fill: parent; activeTransfer: page.activeTransfer }
}
