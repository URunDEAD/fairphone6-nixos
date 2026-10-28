// Springboard: type first. Instant local suggestions (apps, contacts,
// calculation, link, web search) and "Ask the assistant"; recent chats.
import QtQuick 2.12
import Lomiri.Components 1.3

Page {
    id: page
    property var suggestions: []
    property var chats: []
    property bool listening: false

    function prefill(text) {
        input.text = text
        input.forceActiveFocus()
    }
    function refreshChats() {
        root.callJson("ListChats", [], function(v) { if (v) page.chats = v })
    }
    function choose(s) {
        if (s.kind === "ask") root.ask(input.text)
        else if (s.kind === "calc") { input.text = s.value; return }
        else if (s.kind === "app") Qt.openUrlExternally("application:///" + s.app + ".desktop")
        else if (s.url) Qt.openUrlExternally(s.url)
        input.text = ""
    }

    header: PageHeader {
        id: header
        title: i18n.tr("Assistant")
        trailingActionBar.actions: [
            Action {
                iconName: "settings"
                text: i18n.tr("Settings")
                onTriggered: stack.push(Qt.resolvedUrl("SettingsPage.qml"))
            },
            Action {
                iconName: "media-playback-stop"
                text: i18n.tr("Stop all")
                visible: page.chats.some(function(c) { return c.running })
                onTriggered: root.call("StopAll", [])
            }
        ]
    }

    Timer {
        id: debounce
        interval: 150
        onTriggered: {
            if (input.text.trim() === "") { page.suggestions = []; return }
            root.callJson("Suggest", [input.text], function(v) { if (v) page.suggestions = v })
        }
    }

    Connections {
        target: root
        function onChatChanged(chat) { page.refreshChats() }
        function onTurnState(chat, state) { page.refreshChats() }
    }
    Component.onCompleted: refreshChats()
    onVisibleChanged: if (visible) refreshChats()

    Column {
        id: top
        anchors { top: header.bottom; left: parent.left; right: parent.right; margins: units.gu(2) }
        spacing: units.gu(1.5)

        Label {
            width: parent.width
            text: i18n.tr("What do you want to do?")
            textSize: Label.Large
            wrapMode: Text.Wrap
        }

        Row {
            width: parent.width
            spacing: units.gu(1)
            TextField {
                id: input
                objectName: "springboardInput"
                width: parent.width - mic.width - parent.spacing
                height: units.gu(6)
                placeholderText: i18n.tr("Type to do anything…")
                inputMethodHints: Qt.ImhNoPredictiveText
                onTextChanged: debounce.restart()
                onAccepted: if (text.trim() !== "") { root.ask(text); text = "" }
            }
            AbstractButton {
                id: mic
                width: units.gu(6)
                height: units.gu(6)
                Rectangle {
                    anchors.fill: parent
                    radius: width / 2
                    color: page.listening ? theme.palette.normal.negative : theme.palette.normal.base
                }
                Icon {
                    anchors.centerIn: parent
                    width: units.gu(3); height: width
                    name: "audio-input-microphone-symbolic"
                    color: page.listening ? "white" : theme.palette.normal.baseText
                }
                onClicked: {
                    if (!page.listening) {
                        root.call("dictationStart", [], function(r, err) {
                            if (err) root.showError(i18n.tr("Voice input is not available: ") + err)
                            else page.listening = true
                        })
                    } else {
                        page.listening = false
                        root.call("dictationStop", [], function(text, err) {
                            if (!err && text) input.text = (input.text ? input.text + " " : "") + text
                        })
                    }
                }
            }
        }
    }

    ListView {
        id: list
        anchors { top: top.bottom; topMargin: units.gu(1); left: parent.left; right: parent.right; bottom: parent.bottom }
        clip: true
        model: input.text.trim() !== "" ? page.suggestions : page.chats
        header: Item {
            width: list.width
            height: input.text.trim() === "" ? units.gu(5) : 0
            visible: height > 0
            Label {
                anchors { left: parent.left; leftMargin: units.gu(2); verticalCenter: parent.verticalCenter }
                text: page.chats.length ? i18n.tr("Recent chats") : i18n.tr("No chats yet. Try “text Test I'm on my way” or “flashlight on”.")
                color: theme.palette.normal.backgroundTertiaryText
                width: list.width - units.gu(4)
                wrapMode: Text.Wrap
            }
        }
        delegate: ListItem {
            readonly property bool isSuggestion: input.text.trim() !== ""
            height: layout.height + (divider.visible ? divider.height : 0)
            onClicked: isSuggestion ? page.choose(modelData) : root.openChat(modelData.id)
            ListItemLayout {
                id: layout
                title.text: isSuggestion ? modelData.title : modelData.title
                subtitle.text: isSuggestion ? (modelData.subtitle || "") : modelData.preview
                summary.text: !isSuggestion && modelData.remote ? i18n.tr("Cloud: %1").arg(modelData.provider) : ""
                Icon {
                    SlotsLayout.position: SlotsLayout.Leading
                    width: units.gu(3); height: width
                    name: {
                        if (!isSuggestion) return modelData.running ? "sync" : (modelData.remote ? "weather-few-clouds-symbolic" : "message")
                        switch (modelData.kind) {
                        case "ask": return "message-new"
                        case "call": return "call-start"
                        case "message": return "message"
                        case "app": return "view-grid-symbolic"
                        case "calc": return "calculator-app-symbolic"
                        case "url": return "external-link"
                        default: return "find"
                        }
                    }
                    color: isSuggestion && modelData.kind === "ask" ? theme.palette.normal.activity : theme.palette.normal.baseText
                }
            }
            leadingActions: isSuggestion ? null : deleteActions
            ListItemActions {
                id: deleteActions
                actions: [Action {
                    iconName: "delete"
                    onTriggered: root.call("DeleteChat", [modelData.id], function() { page.refreshChats() })
                }]
            }
        }
    }
}
