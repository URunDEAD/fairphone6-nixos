// One conversation: streamed replies (Markdown), tool chips, attachments,
// voice input, Stop, the activity log, and the cloud-model notice.
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3

Page {
    id: page
    property string chatId: ""
    property var chat: ({items: [], activity: [], grants: []})
    property string live: ""
    property string status: ""
    property bool running: false
    property var attachments: []
    property bool listening: false

    function load() {
        root.callJson("GetChat", [chatId], function(v, err) {
            if (err || !v) return
            page.chat = v
            page.running = v.running
            if (!v.running) page.live = ""
            Qt.callLater(function() { messages.positionViewAtEnd() })
        })
    }
    function send() {
        var t = composer.text.trim()
        if (t === "" && attachments.length === 0) return
        root.call("Send", [chatId, t, JSON.stringify(attachments)], function(r, err) {
            if (err) { root.showError(err); return }
            composer.text = ""
            page.attachments = []
            page.running = true
            page.status = i18n.tr("Thinking…")
            page.load()
        })
    }
    function toolTitle(name) {
        var t = {
            contacts_search: i18n.tr("Looked up contacts"), messages_send: i18n.tr("Sent a text message"),
            messages_read: i18n.tr("Read messages"), messages_threads: i18n.tr("Listed conversations"),
            messages_search: i18n.tr("Searched messages"), calls_recent: i18n.tr("Read the call history"),
            call_place: i18n.tr("Started a call"), email_list: i18n.tr("Listed e-mail"), email_read: i18n.tr("Read an e-mail"),
            email_search: i18n.tr("Searched e-mail"), email_send: i18n.tr("Sent an e-mail"), files_list: i18n.tr("Listed a folder"),
            files_read: i18n.tr("Read a file"), files_find: i18n.tr("Searched files"), files_write: i18n.tr("Wrote a file"),
            files_delete: i18n.tr("Moved a file to the trash"), screenshot_latest: i18n.tr("Looked at the latest screenshot"),
            screenshot_take: i18n.tr("Asked for a screenshot"), camera_photo: i18n.tr("Took a photo"), ocr_image: i18n.tr("Read text in an image"),
            calendar_list: i18n.tr("Read the calendar"), calendar_add: i18n.tr("Added an event"), alarm_set: i18n.tr("Set an alarm"),
            timer_set: i18n.tr("Started a timer"), settings_get: i18n.tr("Read phone settings"), settings_set: i18n.tr("Changed a setting"),
            notifications_recent: i18n.tr("Read notifications"), location_get: i18n.tr("Got the location"), web_fetch: i18n.tr("Fetched a web page"),
            web_search: i18n.tr("Searched the web"), app_open: i18n.tr("Opened an app"), url_open: i18n.tr("Opened a link"),
            share_open: i18n.tr("Opened the share sheet"), kdeconnect_send: i18n.tr("Sent to a KDE Connect device"), phone_status: i18n.tr("Checked the phone")
        }
        return t[name] || name
    }

    Component.onCompleted: load()
    Connections {
        target: root
        function onChatDelta(chat, text) { if (chat === page.chatId) { page.live += text; page.status = ""; messages.positionViewAtEnd() } }
        function onChatChanged(chat) { if (chat === page.chatId || chat === "") page.load() }
        function onTurnState(chat, state) {
            if (chat !== page.chatId) return
            if (state === "idle" || state.indexOf("error:") === 0) {
                page.running = false
                page.status = ""
                page.live = ""
                if (state.indexOf("error:") === 0) page.status = state.substring(6)
                page.load()
            } else if (state === "thinking") {
                page.running = true
                if (page.live === "") page.status = i18n.tr("Thinking…")
            } else if (state.indexOf("tool:") === 0) {
                page.running = true
                page.live = ""
                page.status = page.toolTitle(state.substring(5)) + "…"
                page.load()
            }
        }
    }

    header: PageHeader {
        id: header
        title: page.chat.title || i18n.tr("Chat")
        subtitle: page.chat.model ? (page.chat.remote ? i18n.tr("%1 · cloud").arg(page.chat.model) : i18n.tr("%1 · on this phone").arg(page.chat.model)) : ""
        trailingActionBar.actions: [
            Action {
                iconName: "media-playback-stop"
                text: i18n.tr("Stop")
                visible: page.running
                onTriggered: root.call("Stop", [page.chatId])
            },
            Action {
                iconName: "history"
                text: i18n.tr("Activity")
                onTriggered: stack.push(Qt.resolvedUrl("ActivityPage.qml"), {chatId: page.chatId, chat: page.chat})
            }
        ]
    }

    Rectangle {
        id: cloud
        visible: page.chat.remote === true
        anchors { top: header.bottom; left: parent.left; right: parent.right }
        height: visible ? cloudLabel.height + units.gu(2) : 0
        color: Qt.rgba(theme.palette.normal.activity.r, theme.palette.normal.activity.g, theme.palette.normal.activity.b, 0.15)
        Label {
            id: cloudLabel
            anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: units.gu(2) }
            wrapMode: Text.Wrap
            textSize: Label.Small
            text: i18n.tr("This chat uses a cloud model: your messages and what the tools find (messages, e-mail, files…) are sent to %1.").arg(page.chat.provider)
        }
    }

    ListView {
        id: messages
        anchors { top: cloud.bottom; left: parent.left; right: parent.right; bottom: composerBox.top }
        clip: true
        spacing: units.gu(1)
        topMargin: units.gu(1)
        bottomMargin: units.gu(1)
        model: page.chat.items
        delegate: Item {
            width: messages.width
            height: modelData.kind === "tool" ? chip.height : bub.height
            MessageBubble {
                id: bub
                visible: modelData.kind !== "tool"
                width: parent.width
                text: modelData.text || ""
                mine: modelData.kind === "user"
                attachments: modelData.attachments || []
            }
            ToolChip {
                id: chip
                visible: modelData.kind === "tool"
                label: modelData.kind === "tool" ? page.toolTitle(modelData.name) : ""
                onClicked: stack.push(Qt.resolvedUrl("ActivityPage.qml"), {chatId: page.chatId, chat: page.chat})
            }
        }
        footer: Column {
            width: messages.width
            spacing: units.gu(1)
            Loader {
                width: parent.width
                active: page.live !== ""
                sourceComponent: MessageBubble { text: page.live; mine: false; width: parent ? parent.width : 0 }
            }
            Row {
                visible: page.status !== ""
                spacing: units.gu(1)
                x: units.gu(2)
                ActivityIndicator { running: page.running; visible: page.running; width: units.gu(2); height: width }
                Label { text: page.status; color: theme.palette.normal.backgroundSecondaryText; width: messages.width - units.gu(7); wrapMode: Text.Wrap }
            }
            Label {
                visible: !!page.chat.error && !page.running
                x: units.gu(2)
                width: messages.width - units.gu(4)
                wrapMode: Text.Wrap
                color: theme.palette.normal.negative
                text: page.chat.error || ""
            }
        }
    }

    Rectangle {
        id: composerBox
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        height: col.height + units.gu(2)
        color: theme.palette.normal.background
        Rectangle { anchors { left: parent.left; right: parent.right; top: parent.top } height: units.dp(1); color: theme.palette.normal.base }
        Column {
            id: col
            anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; leftMargin: units.gu(1); rightMargin: units.gu(1) }
            spacing: units.gu(1)
            Flow {
                width: parent.width
                spacing: units.gu(1)
                visible: page.attachments.length > 0
                Repeater {
                    model: page.attachments
                    Rectangle {
                        height: units.gu(3.5); radius: units.gu(1)
                        width: attLabel.width + units.gu(4)
                        color: theme.palette.normal.base
                        Label { id: attLabel; anchors { left: parent.left; leftMargin: units.gu(1); verticalCenter: parent.verticalCenter }
                                text: modelData.split("/").pop(); textSize: Label.Small; elide: Text.ElideMiddle; width: Math.min(implicitWidth, units.gu(20)) }
                        Icon { anchors { right: parent.right; rightMargin: units.gu(0.5); verticalCenter: parent.verticalCenter }
                               name: "close"; width: units.gu(2); height: width
                               MouseArea { anchors.fill: parent; onClicked: page.attachments = page.attachments.filter(function(a) { return a !== modelData }) } }
                    }
                }
            }
            Row {
                width: parent.width
                spacing: units.gu(1)
                AbstractButton {
                    id: attach
                    width: units.gu(4.5); height: units.gu(4.5)
                    anchors.bottom: parent.bottom
                    Icon { anchors.centerIn: parent; name: "attachment"; width: units.gu(3); height: width }
                    onClicked: {
                        var p = stack.push(Qt.resolvedUrl("AttachPicker.qml"))
                        p.picked.connect(function(paths) { page.attachments = page.attachments.concat(paths) })
                    }
                }
                TextArea {
                    id: composer
                    objectName: "chatComposer"
                    width: parent.width - attach.width - mic.width - sendBtn.width - 3 * parent.spacing
                    autoSize: true
                    maximumLineCount: 6
                    placeholderText: i18n.tr("Message")
                }
                AbstractButton {
                    id: mic
                    width: units.gu(4.5); height: units.gu(4.5)
                    anchors.bottom: parent.bottom
                    Icon { anchors.centerIn: parent; name: "audio-input-microphone-symbolic"; width: units.gu(3); height: width
                           color: page.listening ? theme.palette.normal.negative : theme.palette.normal.baseText }
                    onClicked: {
                        if (!page.listening) {
                            root.call("dictationStart", [], function(r, err) {
                                if (err) root.showError(i18n.tr("Voice input is not available: ") + err)
                                else page.listening = true
                            })
                        } else {
                            page.listening = false
                            root.call("dictationStop", [], function(text, err) {
                                if (!err && text) composer.text = (composer.text ? composer.text + " " : "") + text
                            })
                        }
                    }
                }
                AbstractButton {
                    id: sendBtn
                    width: units.gu(4.5); height: units.gu(4.5)
                    anchors.bottom: parent.bottom
                    Rectangle { anchors.fill: parent; radius: width / 2
                                color: page.running ? theme.palette.normal.negative : theme.palette.normal.positive }
                    Icon { anchors.centerIn: parent; width: units.gu(2.5); height: width; color: "white"
                           name: page.running ? "media-playback-stop" : "send" }
                    onClicked: page.running ? root.call("Stop", [page.chatId]) : page.send()
                }
            }
        }
    }
}
