// Preview of System Settings > Quick settings (the real page, offscreen).
import QtQuick 2.12
import Lomiri.Components 1.3
import "Shot.js" as Shot

MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(103.5)
    applicationName: "lomiri-system-settings"
    Rectangle { anchors.fill: parent; color: theme.palette.normal.background; z: -1 }
    PageStack { id: pageStack; Component.onCompleted: push(Qt.resolvedUrl("PageComponent.qml")) }
    Timer {
        interval: 1500; running: true
        onTriggered: {
            if (Shot.shot === "settings-scrolled") pageStack.currentPage.flickable.contentY = units.gu(60);
            later.start();
        }
    }
    Timer { id: later; interval: 500; onTriggered: mv.grabToImage(function(r) { r.saveToFile(Shot.out); Qt.quit(); }) }
}
