import QtQuick 2.12
import Lomiri.Content 1.3
Item {
    id: root
    signal shared(string url)
    property var activeTransfer: null
    function collect() {
        if (activeTransfer && activeTransfer.state === ContentTransfer.Charged && activeTransfer.items.length > 0)
            shared(activeTransfer.items[0].url.toString())
    }
    Connections {
        target: ContentHub
        function onShareRequested(transfer) { root.activeTransfer = transfer; root.collect() }
        function onImportRequested(transfer) { root.activeTransfer = transfer; root.collect() }
    }
    Connections { target: root.activeTransfer; function onStateChanged() { root.collect() } }
}
