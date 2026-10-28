// One model service: on this phone (fp6-llm), OpenAI-compatible URL,
// Anthropic, or Ollama on a LAN machine. The key goes to the keyring and
// is never shown again.
import QtQuick 2.12
import Lomiri.Components 1.3

Page {
    id: page
    property var backend: ({})
    property bool hasKey: false
    property var settingsPage: null
    property var models: backend.models || []
    property string status: ""
    readonly property var kinds: ["local", "openai", "anthropic", "ollama"]
    readonly property bool isNew: backend.id === ""
    // The on-device runtime's effective context (Status.n_ctx; RAM-dependent).
    readonly property int localCtx: {
        try { return JSON.parse(settingsPage && settingsPage.localStatus ? settingsPage.localStatus : "{}").n_ctx || 0 }
        catch (e) { return 0 }
    }

    function slug(t) { return (t.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "service") + "-" + Date.now() % 100000 }
    function current() {
        return {
            id: isNew ? slug(nameField.text) : backend.id,
            name: nameField.text || kindSel.model[kindSel.selectedIndex],
            kind: kinds[kindSel.selectedIndex],
            base_url: urlField.text.trim(),
            model: modelField.text.trim(),
            effort: kinds[kindSel.selectedIndex] === "anthropic" ? ["", "low", "medium", "high"][effortSel.selectedIndex] : "",
            models: page.models,
            context_tokens: parseInt(contextField.text) || 0
        }
    }
    function save(thenDefault) {
        var b = current()
        settingsPage.save(function(c) {
            var found = false
            for (var i = 0; i < c.backends.length; i++) if (c.backends[i].id === b.id) { c.backends[i] = b; found = true }
            if (!found) c.backends.push(b)
            if (thenDefault) c.default_backend = b.id
        })
        if (keyField.text !== "") {
            var k = keyField.text
            keyField.text = ""
            // the backend must exist before its key is accepted
            keyTimer.key = "backend:" + b.id; keyTimer.value = k; keyTimer.start()
        }
        page.backend = b
    }
    Timer {
        id: keyTimer
        interval: 400
        property string key: ""
        property string value: ""
        onTriggered: root.call("SetKey", [key, value], function(r, err) {
            value = ""
            if (err) root.showError(err); else { page.hasKey = true; page.status = i18n.tr("Key saved.") }
        })
    }

    header: PageHeader {
        id: header
        title: isNew ? i18n.tr("Add a model service") : backend.name
        trailingActionBar.actions: [Action { iconName: "ok"; text: i18n.tr("Save"); onTriggered: { page.save(false); stack.pop() } }]
    }

    Flickable {
        anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
        contentHeight: col.height + units.gu(4)
        clip: true
        Column {
            id: col
            x: units.gu(2)
            width: parent.width - units.gu(4)
            spacing: units.gu(1.5)
            Item { width: 1; height: units.gu(1) }
            Label { text: i18n.tr("Name") }
            TextField { id: nameField; width: parent.width; text: backend.name || "" }
            OptionSelector {
                id: kindSel
                text: i18n.tr("Type")
                model: [i18n.tr("On this phone"), i18n.tr("OpenAI-compatible"), i18n.tr("Anthropic (Claude)"), i18n.tr("Ollama")]
                selectedIndex: Math.max(0, page.kinds.indexOf(backend.kind))
            }
            Label {
                width: parent.width; wrapMode: Text.Wrap; textSize: Label.Small
                color: theme.palette.normal.backgroundSecondaryText
                text: [i18n.tr("Uses the on-device model service. If it is not installed, the URL below (e.g. http://127.0.0.1:8080) is used."),
                       i18n.tr("Any OpenAI-compatible /v1/chat/completions server with tool calling (llama.cpp, vLLM, LM Studio, hosted services)."),
                       i18n.tr("Anthropic Messages API. Get an API key at console.anthropic.com."),
                       i18n.tr("Ollama on a computer on your network, e.g. http://192.168.1.20:11434. Chats are sent to that computer.")][kindSel.selectedIndex]
            }
            Label { text: i18n.tr("Address (URL)") }
            TextField {
                id: urlField; width: parent.width
                text: backend.base_url || (page.kinds[kindSel.selectedIndex] === "anthropic" ? "https://api.anthropic.com" : "")
                placeholderText: ["http://127.0.0.1:8080", "https://…/v1", "https://api.anthropic.com", "http://192.168.1.20:11434"][kindSel.selectedIndex]
                inputMethodHints: Qt.ImhUrlCharactersOnly | Qt.ImhNoPredictiveText
            }
            Label { text: i18n.tr("Model") }
            TextField {
                id: modelField; width: parent.width; text: backend.model || ""
                placeholderText: page.kinds[kindSel.selectedIndex] === "anthropic" ? "claude-sonnet-5-5" : i18n.tr("from the service")
                inputMethodHints: Qt.ImhNoPredictiveText | Qt.ImhNoAutoUppercase
            }
            Flow {
                width: parent.width
                spacing: units.gu(1)
                Repeater {
                    model: page.models
                    Button { text: modelData; width: Math.min(implicitWidth, col.width); onClicked: modelField.text = modelData
                             color: modelField.text === modelData ? theme.palette.normal.positive : theme.palette.normal.base }
                }
            }
            Button {
                text: i18n.tr("Fetch the model list")
                visible: !page.isNew
                onClicked: root.callJson("ListModels", [backend.id], function(v, err) {
                    if (err) page.status = err; else { page.models = v; page.status = i18n.tr("%1 models").arg(v.length) }
                })
            }
            OptionSelector {
                id: effortSel
                visible: page.kinds[kindSel.selectedIndex] === "anthropic"
                text: i18n.tr("Effort")
                model: [i18n.tr("Model default"), i18n.tr("Low (fast)"), i18n.tr("Medium"), i18n.tr("High")]
                selectedIndex: Math.max(0, ["", "low", "medium", "high"].indexOf(backend.effort || ""))
            }
            Label { text: i18n.tr("Context size (tokens; empty = automatic)") }
            TextField {
                id: contextField; width: parent.width
                text: backend.context_tokens ? String(backend.context_tokens) : ""
                placeholderText: page.kinds[kindSel.selectedIndex] !== "local" ? "128000"
                               : page.localCtx > 0 ? i18n.tr("%1 (set by the on-device service)").arg(page.localCtx) : "2048"
                inputMethodHints: Qt.ImhDigitsOnly
            }
            Label {
                width: parent.width; wrapMode: Text.Wrap; textSize: Label.Small
                color: theme.palette.normal.backgroundSecondaryText
                text: i18n.tr("Below 16000 the assistant sends only the tools your request is about and the recent part of the chat.")
            }
            Button {
                text: i18n.tr("Manage on-device models…")
                visible: page.kinds[kindSel.selectedIndex] === "local"
                onClicked: {
                    // In place when the runtime's QML module is installed, else its own app.
                    var c = Qt.createComponent(Qt.resolvedUrl("LocalModelsPage.qml"))
                    if (c.status === Component.Ready) stack.push(c)
                    else Qt.openUrlExternally("application:///com.fairphone6.local-llm.desktop")
                }
            }
            Label { text: page.hasKey ? i18n.tr("API key (saved; type to replace)") : i18n.tr("API key"); visible: page.kinds[kindSel.selectedIndex] !== "local" }
            TextField {
                id: keyField; width: parent.width
                visible: page.kinds[kindSel.selectedIndex] !== "local"
                echoMode: TextInput.Password
                placeholderText: page.hasKey ? "••••••••" : (page.kinds[kindSel.selectedIndex] === "anthropic" ? "sk-ant-…" : i18n.tr("optional"))
                inputMethodHints: Qt.ImhSensitiveData | Qt.ImhNoPredictiveText | Qt.ImhNoAutoUppercase
            }
            Label { width: parent.width; wrapMode: Text.Wrap; text: page.status; visible: text !== "" }
            Button { text: i18n.tr("Save and use by default"); color: theme.palette.normal.positive; onClicked: { page.save(true); stack.pop() } }
            Button {
                text: i18n.tr("Remove the key")
                visible: page.hasKey
                onClicked: root.call("SetKey", ["backend:" + backend.id, ""], function() { page.hasKey = false })
            }
            Button {
                text: i18n.tr("Delete this service")
                visible: !page.isNew && backend.id !== "local"
                color: theme.palette.normal.negative
                onClicked: {
                    root.call("SetKey", ["backend:" + backend.id, ""])
                    settingsPage.save(function(c) {
                        c.backends = c.backends.filter(function(b) { return b.id !== backend.id })
                        if (c.default_backend === backend.id) c.default_backend = "local"
                    })
                    stack.pop()
                }
            }
        }
    }
}
