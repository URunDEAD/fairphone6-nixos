// Offscreen previews (fp6-assistant --preview <scene> --screenshot <png>):
// canned, synthetic data from the bridge; prints PASS/FAIL checks.
import QtQuick 2.12
import QtQuick.Window 2.12
import Lomiri.Components 1.3

Item {
    id: scenes
    property int fails: 0

    function find(item, name) {
        if (!item) return null
        if (item.objectName === name) return item
        var kids = item.children || []
        for (var i = 0; i < kids.length; i++) {
            var r = find(kids[i], name)
            if (r) return r
        }
        return null
    }
    function check(cond, what) {
        console.log((cond ? "PASS " : "FAIL ") + assistant.previewScene + ": " + what)
        if (!cond) fails++
    }
    readonly property var permissionPrompt: ({
        id: "p1", kind: "permission", chat: "c1", tool: "files_list", tool_title: "List a folder",
        resource: "files.read:/home/user/Documents", title: "Files: read ~/Documents",
        explain: "List and read files in this folder", scope: "~/Documents", allow_always: true, tainted: [], remote: false, provider: "this phone"
    })
    readonly property var taintedPrompt: ({
        id: "p2", kind: "permission", chat: "c2", tool: "messages_send", tool_title: "Send a text message",
        resource: "messages.send", title: "Messages: send", explain: "Send text messages (each one is shown to you first)",
        allow_always: false, tainted: ["Read messages"], remote: true, provider: "Anthropic (api.anthropic.com)"
    })
    readonly property var confirmPrompt: ({
        id: "p3", kind: "confirm", chat: "c1", tool: "messages_send", title: "Send this text message?",
        fields: [{label: "To", value: "+999 555 0100"}, {label: "Message", value: "Running 10 minutes late"}],
        action: "Send", destructive: false, tainted: [], remote: false
    })

    function start() {
        switch (assistant.previewScene) {
        case "home": break
        case "springboard": springboard.prefill("call test"); break
        case "chat": root.openChat("c1"); break
        case "cloud": root.openChat("c2"); break
        case "permission": root.enqueue(permissionPrompt); break
        case "permission-tainted": root.openChat("c2"); root.enqueue(taintedPrompt); break
        case "confirm": root.openChat("c1"); root.enqueue(confirmPrompt); break
        case "activity": stack.push(Qt.resolvedUrl("ActivityPage.qml"), {chatId: "c1"}); stack.currentPage.reload(); break
        case "settings": stack.push(Qt.resolvedUrl("SettingsPage.qml")); break
        case "backend":
            root.callJson("GetSettings", [], function(v) {
                stack.push(Qt.resolvedUrl("BackendPage.qml"), {backend: v.settings.backends[1], hasKey: true, settingsPage: null})
            })
            break
        case "tools": stack.push(Qt.resolvedUrl("ToolsPage.qml"), {settingsPage: null}); break
        case "screenshot": ops.screenshotPrompt = {id: "p4", op: "screenshot_hint", args: {reason: "To read the error message on screen"}}; break
        }
        shot.start()
    }

    Timer {
        id: shot
        interval: 2500
        onTriggered: {
            var s = assistant.previewScene
            if (s === "permission") {
                check(root.currentPrompt && root.currentPrompt.id === "p1", "permission dialog open")
                check(find(root.currentPopup, "allowAlways") && find(root.currentPopup, "allowAlways").visible, "Always allow offered")
            } else if (s === "permission-tainted") {
                var b = find(root.currentPopup, "allowAlways")
                check(b !== null && !b.visible, "no Always allow after untrusted content")
                check(find(root.currentPopup, "allowOnce") !== null, "Allow once offered")
            } else if (s === "confirm") {
                check(root.currentPrompt && root.currentPrompt.kind === "confirm", "confirmation open")
                check(find(root.currentPopup, "confirmAction") !== null, "Send button present")
            } else if (s === "springboard") {
                check(springboard.suggestions.length >= 3 && springboard.suggestions[0].kind === "ask", "Ask first, then actions")
            } else if (s === "chat" || s === "cloud") {
                check(stack.currentPage.chat.items.length >= 3, "conversation loaded")
            }
            root.Window.window.contentItem.grabToImage(function(r) {
                if (assistant.screenshotPath !== "" && !r.saveToFile(assistant.screenshotPath)) console.log("FAIL save " + assistant.screenshotPath)
                console.log("DONE " + s + " fails=" + scenes.fails)
                Qt.exit(scenes.fails > 0 ? 1 : 0)
            })
        }
    }
}
