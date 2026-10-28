import QtQuick 2.12
import Lomiri.Components 1.3
import Fairphone.LocalLlm 1.0

Flickable {
    id: view
    property bool preview: false
    property string fixture: ""
    property var snapshot: {
        try { return JSON.parse(preview ? fixture : manager.snapshot); }
        catch (e) { return {status: {}, models: []}; }
    }
    property var status: snapshot.status || ({})
    property string pendingDelete: ""
    function gib(bytes) { return (Number(bytes || 0) / 1073741824).toFixed(2) + " GiB"; }
    function configure(idle, gpu) {
        manager.act("Configure", JSON.stringify({idle_seconds: idle, use_gpu: gpu, battery_saver: !!status.battery_saver_override}));
    }
    LocalLlmBackend { id: manager; preview: view.preview }
    Timer { interval: 250; repeat: true; running: !view.preview; triggeredOnStart: true; onTriggered: manager.poll() }
    contentHeight: column.height + units.gu(4)
    clip: true
    Column {
        id: column
        x: units.gu(2); y: units.gu(2)
        width: parent.width - units.gu(4)
        spacing: units.gu(1.5)
        Label { width: parent.width; wrapMode: Text.Wrap; text: "Run the assistant on this phone"; fontSize: "large" }
        Label { width: parent.width; wrapMode: Text.Wrap; text: "Downloads use your connection only when you tap Download. Chats run on the phone. Start with Qwen3 1.7B." }
        Label { width: parent.width; wrapMode: Text.Wrap; text: "Storage used: " + view.gib(view.status.storage_bytes) + " · Free RAM: " + (view.status.available_mib || 0) + " MiB" }
        Label { width: parent.width; wrapMode: Text.Wrap; text: view.status.battery_saver ? "Paused while battery saver is on" : (view.status.loaded ? "Model loaded" : "Model unloaded") + " · " + (view.status.reason || "Connecting…"); color: "#666666" }
        Label { visible: manager.message.length > 0; width: parent.width; wrapMode: Text.Wrap; text: manager.message; color: "#c7162b" }
        Row {
            width: parent.width; spacing: units.gu(1)
            Label { text: "Unload after idle"; anchors.verticalCenter: parent.verticalCenter; width: parent.width - timeout.width - units.gu(1) }
            OptionSelector {
                id: timeout
                width: units.gu(17); containerHeight: units.gu(20)
                model: ["30 seconds", "2 minutes", "5 minutes", "15 minutes"]
                selectedIndex: [30,120,300,900].indexOf(Number(view.status.idle_seconds || 120))
                onDelegateClicked: view.configure([30,120,300,900][index], !!view.status.use_gpu)
            }
        }
        Row {
            width: parent.width; spacing: units.gu(1)
            Label { text: "Context size"; anchors.verticalCenter: parent.verticalCenter; width: parent.width - contextSel.width - units.gu(1) }
            OptionSelector {
                id: contextSel
                readonly property var sizes: [0, 2048, 4096, 8192, 16384]
                width: units.gu(17); containerHeight: units.gu(25)
                model: ["Automatic", "2048 tokens", "4096 tokens", "8192 tokens", "16384 tokens"]
                selectedIndex: Math.max(0, sizes.indexOf(Number(view.status.context_tokens || 0)))
                onDelegateClicked: manager.act("SetContext", String(sizes[index]))
            }
        }
        Label {
            width: parent.width; wrapMode: Text.Wrap; color: "#666666"
            text: "The assistant uses " + (view.status.n_ctx || 0) + " tokens" + (Number(view.status.n_ctx || 0) < Number(view.status.context_requested || 0) ? " (less than " + view.status.context_requested + ": not enough free RAM)" : "") + ". Larger contexts keep more of the chat and use more RAM."
        }
        Row {
            width: parent.width; spacing: units.gu(1)
            Label { text: "Use GPU (experimental)"; width: parent.width - gpu.width - units.gu(1); anchors.verticalCenter: parent.verticalCenter }
            Switch { id: gpu; checked: !!view.status.use_gpu; enabled: !!view.status.gpu_available || !!view.status.use_gpu; onClicked: view.configure(Number(view.status.idle_seconds || 120), checked) }
        }
        Label { width: parent.width; wrapMode: Text.Wrap; color: "#666666"; text: view.status.gpu_available ? "Turnip Vulkan may use more memory or fail to load. Switch off to return to CPU." : "GPU support can be installed through the system's local LLM module." }
        Button { text: "Unload model now"; enabled: !!view.status.loaded; onClicked: manager.act("Unload", "") }
        Repeater {
            model: view.snapshot.models || []
            delegate: Column {
                id: card
                width: column.width
                spacing: units.gu(0.8)
                property var entry: modelData
                property var progress: entry.progress || ({})
                property bool downloading: ["queued", "downloading", "verifying"].indexOf(progress.state) >= 0
                Rectangle { width: parent.width; height: 1; color: "#d6d6d6" }
                Label { width: parent.width; wrapMode: Text.Wrap; fontSize: "large"; text: card.entry.name + (card.entry.active ? " · Active" : "") }
                Label { width: parent.width; wrapMode: Text.Wrap; text: card.entry.quantization + " · " + view.gib(card.entry.bytes) + " download · ~" + (Number(card.entry.ram_mib_ctx || card.entry.ram_mib) / 1024).toFixed(1) + " GiB RAM" }
                Label { width: parent.width; wrapMode: Text.Wrap; color: "#666666"; text: card.entry.notes }
                Label { width: parent.width; wrapMode: Text.Wrap; text: "Licence: <a href='" + card.entry.license_url + "'>" + card.entry.license + "</a>"; onLinkActivated: Qt.openUrlExternally(link) }
                ProgressBar { width: parent.width; visible: card.downloading; minimumValue: 0; maximumValue: Number(card.entry.bytes); value: Number(card.progress.received || 0) }
                Label { width: parent.width; visible: card.downloading || card.progress.state === "failed"; wrapMode: Text.Wrap; text: card.progress.state === "failed" ? card.progress.message : card.progress.state === "verifying" ? "Verifying SHA-256…" : "Downloading " + view.gib(card.progress.received) + " / " + view.gib(card.entry.bytes) }
                Row {
                    spacing: units.gu(1)
                    Button { visible: !card.entry.installed && !card.downloading; text: card.entry.partial_bytes > 0 ? "Resume download" : "Download"; onClicked: manager.act("Download", card.entry.id) }
                    Button { visible: card.entry.installed; enabled: !card.entry.active; text: card.entry.active ? "Active model" : "Use model"; onClicked: manager.act("SetActive", card.entry.id) }
                    Button { visible: (card.entry.installed || card.entry.partial_bytes > 0) && !card.downloading; text: "Delete"; onClicked: view.pendingDelete = card.entry.id }
                }
                Column {
                    width: parent.width; spacing: units.gu(1); visible: view.pendingDelete === card.entry.id
                    Label { text: "Delete this model from the phone?"; width: parent.width; wrapMode: Text.Wrap }
                    Row {
                        spacing: units.gu(1)
                        Button { text: "Delete model"; color: "#c7162b"; onClicked: { manager.act("Delete", card.entry.id); view.pendingDelete = ""; } }
                        Button { text: "Keep"; onClicked: view.pendingDelete = "" }
                    }
                }
            }
        }
    }
}
