import QtQuick 2.12
import QtQuick.Window 2.12
Window {
    id: window
    width: 432; height: previewMode ? 1260 : 940
    visible: true
    title: "Share"
    color: "#f6f6f6"
    Rectangle {
        id: canvas
        anchors.fill: parent
        color: "#f6f6f6"
        Main { id: sheet; anchors.fill: parent; onRequestOpened: { if (!previewMode) { window.show(); window.raise(); window.requestActivate() } } }
    }
    onClosing: {
        if (!previewMode && sheet.state.revision)
            shareBridge.command("Cancel", sheet.state.revision, "", "")
    }
    Timer {
        interval: 1500; running: previewMode && previewPath.length > 0
        onTriggered: canvas.grabToImage(function(result) {
            if (!result.saveToFile(previewPath)) console.error("Screenshot failed")
            Qt.quit()
        })
    }
}
