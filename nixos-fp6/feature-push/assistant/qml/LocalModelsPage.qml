// The on-device runtime's model page (fp6-llm's Fairphone.LocalLlm
// ModelsView: catalogue, downloads, active model, context size, idle
// timeout, GPU), embedded in the assistant's settings. Loaded on demand by
// BackendPage: when the local-llm module is not installed the import
// fails and BackendPage opens the standalone launcher instead.
import QtQuick 2.12
import Lomiri.Components 1.3
import Fairphone.LocalLlm 1.0

Page {
    id: page
    header: PageHeader { id: header; title: i18n.tr("On-device AI models") }
    ModelsView {
        anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
    }
}
