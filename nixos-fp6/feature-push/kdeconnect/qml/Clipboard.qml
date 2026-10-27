import QtQuick 2.12
import QtQuick.Window 2.12

// Qt's native Lomiri clipboard integration runs in the graphical session.
// No clipboard is read until at least one paired, online peer has opted in.
Window {
    width: 1; height: 1; visible: false
    property int revision: -1
    property string lastText: ""
    TextEdit { id: clipboardText; visible: false }
    Timer {
        interval: 1000; running: true; repeat: true
        onTriggered: {
            connectBridge.poll()
            if (!connectBridge.clipboard) return
            var state = JSON.parse(connectBridge.clipboard)
            if (!state.enabled) { clipboardText.text = ""; lastText = ""; revision = -1; return }
            if (((revision >= 0 && state.revision !== revision) || (revision < 0 && state.content !== "")) && state.content !== lastText) {
                clipboardText.text = state.content
                clipboardText.selectAll()
                clipboardText.copy()
                lastText = state.content
            } else {
                clipboardText.selectAll()
                clipboardText.paste()
                if (clipboardText.text.length <= 16384 && clipboardText.text !== lastText) {
                    lastText = clipboardText.text
                    connectBridge.command("ReportClipboard", "", lastText, false)
                }
            }
            revision = state.revision
        }
    }
}
