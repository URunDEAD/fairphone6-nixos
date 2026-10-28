// Ask the camera app for a photo over Content Hub; the user takes it.
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Content 1.3

Page {
    id: page
    property string reason: ""
    property bool finished: false
    signal done(string path)

    function finish(path) {
        if (finished) return
        finished = true
        done(path)
        stack.pop()
    }

    header: PageHeader {
        title: i18n.tr("Photo for the assistant")
        leadingActionBar.actions: [Action { iconName: "close"; onTriggered: page.finish("") }]
    }
    Column {
        anchors { top: page.header.bottom; left: parent.left; right: parent.right; margins: units.gu(2) }
        spacing: units.gu(2)
        Label { width: parent.width; wrapMode: Text.Wrap; text: page.reason }
        Label { width: parent.width; wrapMode: Text.Wrap
                text: i18n.tr("The camera opens; take the photo and confirm it. Only that photo goes to the assistant.") }
        Button { text: i18n.tr("Open the camera"); color: theme.palette.normal.positive; onClicked: page.request() }
    }
    property var transfer: null
    function request() {
        transfer = cameraPeer.request()
    }
    ContentPeer {
        id: cameraPeer
        appId: "lomiri-camera-app"
        contentType: ContentType.Pictures
        handler: ContentHandler.Source
        selectionType: ContentTransfer.Single
    }
    Connections {
        target: page.transfer
        ignoreUnknownSignals: true
        function onStateChanged() {
            if (!page.transfer) return
            if (page.transfer.state === ContentTransfer.Charged && page.transfer.items.length > 0)
                page.finish(String(page.transfer.items[0].url).replace("file://", ""))
            else if (page.transfer.state === ContentTransfer.Aborted)
                page.finish("")
        }
    }
    Component.onCompleted: request()
}
