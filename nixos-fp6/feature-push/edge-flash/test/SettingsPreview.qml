// Preview of System Settings > Edge Lighting (the real page, offscreen).
import QtQuick 2.12
import GSettings 1.0
import Lomiri.Components 1.3
import "Shot.js" as Shot

MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(103.5)
    applicationName: "lomiri-system-settings"
    GSettings { id: gs; schema.id: "com.fairphone6.edge-flash" }
    Rectangle { anchors.fill: parent; color: theme.palette.normal.background; z: -1 }
    PageStack { id: pageStack; Component.onCompleted: push(Qt.resolvedUrl("PageComponent.qml")) }
    Component.onCompleted: {
        if (Shot.shot !== "settings-off") gs.enabled = true;
        if (Shot.shot === "settings-fixed") { gs.colorMode = "fixed"; gs.color = "#a46cf0"; gs.reminderMinutes = 10; }
    }
    Timer {
        interval: 1500; running: true
        onTriggered: {
            if (Shot.shot === "settings-fixed") pageStack.currentPage.flickable.contentY = units.gu(30);
            later.start();
        }
    }
    Timer { id: later; interval: 500; onTriggered: mv.grabToImage(function(r) { r.saveToFile(Shot.out); Qt.quit(); }) }
}
