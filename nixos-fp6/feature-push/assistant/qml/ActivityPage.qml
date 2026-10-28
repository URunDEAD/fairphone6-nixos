// Activity log of one chat: every tool call with its arguments, what it
// needed, how it was allowed, and what came back. Grants for this chat
// can be withdrawn here.
import QtQuick 2.12
import Lomiri.Components 1.3

Page {
    id: page
    property string chatId: ""
    property var chat: ({activity: [], grants: []})

    function reload() { root.callJson("GetChat", [chatId], function(v) { if (v) page.chat = v }) }
    Connections {
        target: root
        function onActivity(chat, entry) { if (chat === page.chatId) page.reload() }
    }
    function when(t) { return Qt.formatDateTime(new Date(t * 1000), "dd MMM hh:mm:ss") }

    header: PageHeader { id: header; title: i18n.tr("Activity") }

    Flickable {
        anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
        contentHeight: col.height + units.gu(4)
        clip: true
        Column {
            id: col
            width: parent.width
            ListItem {
                height: units.gu(6)
                visible: (page.chat.grants || []).length > 0
                Label { anchors { left: parent.left; leftMargin: units.gu(2); verticalCenter: parent.verticalCenter }
                        text: i18n.tr("Allowed for this chat"); font.bold: true }
            }
            Repeater {
                model: page.chat.grants || []
                ListItem {
                    height: gl.height
                    ListItemLayout { id: gl; title.text: modelData.title
                        Button { SlotsLayout.position: SlotsLayout.Trailing; text: i18n.tr("Revoke"); width: units.gu(10)
                                 onClicked: root.call("RevokeChatGrant", [page.chatId, modelData.key], function() { page.reload() }) } }
                }
            }
            ListItem {
                height: units.gu(6)
                Label { anchors { left: parent.left; leftMargin: units.gu(2); verticalCenter: parent.verticalCenter }
                        text: (page.chat.activity || []).length ? i18n.tr("Tool calls (newest first)") : i18n.tr("No tools used in this chat yet.")
                        font.bold: true }
            }
            Repeater {
                model: (page.chat.activity || []).slice().reverse()
                ListItem {
                    id: li
                    property bool open: false
                    height: entry.height + units.gu(2)
                    onClicked: open = !open
                    Column {
                        id: entry
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2); topMargin: units.gu(1) }
                        spacing: units.gu(0.5)
                        Row {
                            spacing: units.gu(1)
                            Icon { name: modelData.ok ? "ok" : "dialog-warning-symbolic"; width: units.gu(2); height: width
                                   color: modelData.ok ? theme.palette.normal.positive : theme.palette.normal.negative }
                            Label { text: modelData.tool; font.bold: true }
                            Label { text: page.when(modelData.t) + " · " + modelData.ms + " ms"; textSize: Label.Small
                                    color: theme.palette.normal.backgroundSecondaryText }
                        }
                        Label { width: parent.width; wrapMode: Text.Wrap; textSize: Label.Small; text: modelData.decision }
                        Label { width: parent.width; wrapMode: Text.WrapAnywhere; textSize: Label.Small; font.family: "Ubuntu Mono"
                                text: JSON.stringify(modelData.args) }
                        Label { width: parent.width; wrapMode: Text.WrapAnywhere; textSize: Label.Small; font.family: "Ubuntu Mono"
                                maximumLineCount: li.open ? 400 : 3; elide: Text.ElideRight
                                color: theme.palette.normal.backgroundSecondaryText; text: modelData.result }
                    }
                }
            }
        }
    }
}
