import QtQuick 2.12
import QtQuick.Window 2.12
Window {
    title: "KDE Connect"
    width: 400
    height: 800
    visible: true
    Loader { anchors.fill: parent; source: "Main.qml" }
}
