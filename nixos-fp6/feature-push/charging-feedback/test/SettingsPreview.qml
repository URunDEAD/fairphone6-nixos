// Preview of System Settings > Battery > Charging feedback (the real page, offscreen).
import QtQuick 2.12
import GSettings 1.0
import Lomiri.Components 1.3
import "Shot.js" as Shot

MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(103.5)
    applicationName: "lomiri-system-settings"
    GSettings { id: gs; schema.id: "com.fairphone6.charging-feedback" }
    Rectangle { anchors.fill: parent; color: theme.palette.normal.background; z: -1 }
    PageStack { id: pageStack; Component.onCompleted: push(Qt.resolvedUrl("Fp6ChargingFeedbackPage.qml")) }
    Component.onCompleted: if (Shot.shot === "settings-off") { gs.animation = false; gs.sound = false; }
    Timer {
        interval: 1500; running: true
        onTriggered: {
            var p = pageStack.currentPage;
            console.log((p && p.objectName === "fp6ChargingFeedbackPage" ? "PASS " : "FAIL ") + Shot.shot + ": page loaded");
            later.start();
        }
    }
    Timer { id: later; interval: 500; onTriggered: mv.grabToImage(function(r) { r.saveToFile(Shot.out); Qt.quit(); }) }
}
