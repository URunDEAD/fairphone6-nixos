// Assistant (FP6): type-first Springboard + chat with a local or remote
// model that uses the phone's apps through permission-gated tools.
// The bridge `assistant` (src/ui.rs) talks to fp6-assistantd.
import QtQuick 2.12
import QtQuick.Window 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3

MainView {
    id: root
    objectName: "assistantMain"
    applicationName: "com.fairphone6.Assistant"
    width: units.gu(50)
    height: units.gu(100)
    anchorToKeyboard: true

    readonly property bool preview: assistant.previewScene !== ""
    property var _cbs: ({})
    property int _seq: 0
    property var promptQueue: []
    property var currentPrompt: null
    property var currentPopup: null
    property bool daemonUp: true

    signal chatDelta(string chat, string text)
    signal chatChanged(string chat)
    signal turnState(string chat, string state)
    signal activity(string chat, var entry)
    signal promptClosed(string id)

    function call(method, args, cb) {
        var t = "t" + (++_seq)
        if (cb) _cbs[t] = cb
        assistant.request(t, method, JSON.stringify(args || []))
    }
    function callJson(method, args, cb) {
        call(method, args, function(result, error) {
            var v = null
            if (!error && result !== "") { try { v = JSON.parse(result) } catch (e) { v = result } }
            if (cb) cb(v, error)
        })
    }
    function respond(id, answer) {
        call("Respond", [id, JSON.stringify(answer)])
    }

    function openChat(id) {
        while (stack.depth > 1) stack.pop()
        stack.push(Qt.resolvedUrl("ChatPage.qml"), {chatId: id})
    }
    function ask(text) {
        callJson("Quick", [text], function(id, err) {
            if (err) { showError(err); return }
            openChat(id)
        })
    }
    function showError(text) {
        PopupUtils.open(errorDialog, root, {text: text})
    }

    // ---- dialogs: one at a time, in order
    function enqueue(p) {
        if (p.kind === "ui") { ops.handle(p); return }
        promptQueue.push(p)
        nextPrompt()
    }
    function nextPrompt() {
        if (currentPrompt || promptQueue.length === 0) return
        currentPrompt = promptQueue.shift()
        var comp = currentPrompt.kind === "permission" ? permissionDialog : confirmDialog
        currentPopup = PopupUtils.open(comp, root, {prompt: currentPrompt})
    }
    function answerPrompt(answer) {
        if (!currentPrompt) return
        respond(currentPrompt.id, answer)
        finishPrompt()
    }
    function finishPrompt() {
        if (currentPopup) PopupUtils.close(currentPopup)
        currentPopup = null
        currentPrompt = null
        nextPrompt()
    }

    function handleEvent(kind, json) {
        var v = null
        try { v = JSON.parse(json) } catch (e) { return }
        if (kind === "Prompt") enqueue(v)
        else if (kind === "PromptClosed") {
            promptQueue = promptQueue.filter(function(p) { return p.id !== v })
            if (currentPrompt && currentPrompt.id === v) finishPrompt()
            ops.closed(v)
            promptClosed(v)
        }
        else if (kind === "Delta") chatDelta(v.chat, v.text)
        else if (kind === "ChatChanged") chatChanged(v)
        else if (kind === "TurnState") turnState(v.chat, v.state)
        else if (kind === "Activity") activity(v.chat, v.entry)
        else if (kind === "Daemon") { daemonUp = v.up; if (v.up) register() }
    }

    function register() {
        callJson("RegisterUi", [], function(v, err) {
            if (err) { daemonUp = false; return }
            daemonUp = true
            if (v && v.pending) for (var i = 0; i < v.pending.length; i++) enqueue(v.pending[i])
        })
    }

    // assistant:///chat/<id>  assistant:///ask?q=<text>  assistant:///?q=<text>
    function openUrl(url) {
        if (!url) return
        var m = /^assistant:\/*chat\/([0-9a-f-]+)/.exec(url)
        if (m) { openChat(m[1]); return }
        var q = /[?&]q=([^&]*)/.exec(url)
        if (q) springboard.prefill(decodeURIComponent(q[1].replace(/\+/g, " ")))
    }

    Connections {
        target: assistant
        function onReply(token, result, error) {
            var cb = root._cbs[token]
            delete root._cbs[token]
            if (cb) cb(result, error)
        }
        function onEvent(kind, json) { root.handleEvent(kind, json) }
    }

    Connections {
        target: UriHandler
        function onOpened(uris) { if (uris.length > 0) root.openUrl(uris[0]) }
    }

    Component.onCompleted: {
        register()
        openUrl(assistant.initialUrl)
        if (preview) previewScenes.start()
    }
    Connections {
        target: Qt.application
        function onAboutToQuit() { assistant.quit_ui() }
    }

    PageStack {
        id: stack
        Component.onCompleted: push(springboard)
    }

    SpringboardPage {
        id: springboard
        visible: false
    }

    OpsHandler { id: ops }

    // Screenshot request: the user takes it with Power + Volume Down.
    Rectangle {
        id: shotBanner
        visible: ops.screenshotPrompt !== null
        z: 10
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        height: shotColumn.height + units.gu(3)
        color: theme.palette.normal.foreground
        Column {
            id: shotColumn
            anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: units.gu(2) }
            spacing: units.gu(1)
            Label {
                width: parent.width
                wrapMode: Text.Wrap
                font.bold: true
                text: i18n.tr("The assistant wants a screenshot")
            }
            Label {
                width: parent.width
                wrapMode: Text.Wrap
                text: (ops.screenshotPrompt && ops.screenshotPrompt.args.reason ? ops.screenshotPrompt.args.reason + "\n" : "")
                      + i18n.tr("Switch to the screen you want and press Power + Volume Down. Nothing is captured until you do.")
            }
            Button {
                text: i18n.tr("Cancel")
                onClicked: ops.cancelScreenshot()
            }
        }
    }

    Component {
        id: permissionDialog
        PermissionDialog {
            onAnswered: root.answerPrompt(answer)
        }
    }
    Component {
        id: confirmDialog
        ConfirmDialog {
            onAnswered: root.answerPrompt(answer)
        }
    }
    Component {
        id: errorDialog
        Dialog {
            id: ed
            title: i18n.tr("Assistant")
            Button { text: i18n.tr("OK"); onClicked: PopupUtils.close(ed) }
        }
    }

    PreviewScenes { id: previewScenes }
}
