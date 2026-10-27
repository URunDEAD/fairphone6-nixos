// Preview of System Settings > Gesture Shortcuts with the App switcher section (offscreen).
import QtQuick 2.12
import Lomiri.Components 1.3
import GSettings 1.0
import "Shot.js" as Shot

MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(103.5)
    applicationName: "lomiri-system-settings"
    GSettings { id: sw; schema.id: "com.fairphone6.app-switcher" }
    Component.onCompleted: if (Shot.shot === "settings-off") sw.closeAllKeepReady = false
    Rectangle { anchors.fill: parent; color: theme.palette.normal.background; z: -1 }
    PageStack { id: pageStack; Component.onCompleted: push(Qt.resolvedUrl("PageComponent.qml")) }
    Timer { interval: 2000; running: true; onTriggered: mv.grabToImage(function(r) { r.saveToFile(Shot.out); Qt.quit(); }) }
}
