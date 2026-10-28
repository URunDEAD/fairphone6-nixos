import QtQuick 2.12
import QtQuick.Window 2.2
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
Window {
    id: window
    width: 450; height: 920; visible: true
    MainView {
        id: view
        anchors.fill: parent
        applicationName: "fp6-pairing-preview"
        Rectangle { anchors.fill: parent; color: "#fafafa" }
        Label { x: units.gu(2); y: units.gu(3); text: "Bluetooth"; fontSize: "large" }
        Timer {
            interval: 300; running: true
            onTriggered: {
                var c=Qt.createComponent("../tests/qml/generated/ConfirmPasskeyDialog.qml")
                PopupUtils.open(c,view,{name:"Test Headphones",passkey:"000001"})
            }
        }
        Timer {
            interval: 1600; running: true
            onTriggered: window.contentItem.grabToImage(function(result) {
                var args=Qt.application.arguments
                result.saveToFile(args.filter(function(a) { return a.indexOf("--output=") === 0 })[0].substring(9))
                Qt.quit()
            })
        }
    }
}
