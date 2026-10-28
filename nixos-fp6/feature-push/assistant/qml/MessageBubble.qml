// A chat bubble. The assistant's replies are Markdown (no raw HTML);
// links open only after a tap and only http(s).
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    id: bubble
    property string text: ""
    property bool mine: false
    property var attachments: []
    height: box.height
    width: parent ? parent.width : 0

    // Markdown without HTML: escape <, >, & before handing it to Qt.
    function safeMarkdown(t) {
        return t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>(?!\s)/g, "&gt;")
    }

    Rectangle {
        id: box
        anchors { right: bubble.mine ? parent.right : undefined; left: bubble.mine ? undefined : parent.left
                  rightMargin: units.gu(2); leftMargin: units.gu(2) }
        width: Math.min(bubble.width * 0.82, Math.max(body.implicitWidth, attCol.width) + units.gu(3))
        height: content.height + units.gu(2)
        radius: units.gu(1.5)
        color: bubble.mine ? theme.palette.normal.activity : theme.palette.normal.base
        Column {
            id: content
            anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1.5); topMargin: units.gu(1) }
            spacing: units.gu(0.5)
            Column {
                id: attCol
                visible: bubble.attachments.length > 0
                spacing: units.gu(0.5)
                Repeater {
                    model: bubble.attachments
                    Row {
                        spacing: units.gu(0.5)
                        Icon { name: "attachment"; width: units.gu(2); height: width; color: body.color }
                        Label { text: modelData.split("/").pop(); textSize: Label.Small; color: body.color }
                    }
                }
            }
            Text {
                id: body
                width: parent.width
                visible: bubble.text !== ""
                text: bubble.mine ? bubble.text : bubble.safeMarkdown(bubble.text)
                textFormat: bubble.mine ? Text.PlainText : Text.MarkdownText
                wrapMode: Text.Wrap
                color: bubble.mine ? "white" : theme.palette.normal.baseText
                linkColor: bubble.mine ? "white" : theme.palette.normal.activity
                font.pixelSize: units.gu(2)
                onLinkActivated: if (/^https?:\/\//.test(link)) Qt.openUrlExternally(link)
            }
        }
    }
}
