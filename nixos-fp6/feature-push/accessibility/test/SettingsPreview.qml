// Preview of System Settings > Accessibility pages (the real pages, offscreen).
import QtQuick 2.12
import Lomiri.Components 1.3
import GSettings 1.0
import "Shot.js" as Shot

MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(103.5)
    applicationName: "lomiri-system-settings"
    Rectangle { anchors.fill: parent; color: theme.palette.normal.background; z: -1 }
    GSettings { id: gs; schema.id: "com.fairphone6.accessibility" }
    PageStack { id: pageStack }
    Component.onCompleted: {
        if (Shot.shot === "settings-dark") theme.name = "Lomiri.Components.Themes.SuruDark";
        if (Shot.shot.indexOf("settings-on") === 0 || Shot.shot === "settings-dark") {
            gs.magnificationTap = true; gs.monoAudio = true; gs.audioBalance = -0.3;
            gs.colourCorrection = "deuteranomaly"; gs.colourCorrectionStrength = 0.8;
        }
        var page = Shot.shot.indexOf("magnification") >= 0 ? "MagnificationPage.qml"
                 : Shot.shot.indexOf("colour") >= 0 ? "ColourCorrectionPage.qml" : "PageComponent.qml";
        if (page === "PageComponent.qml") pageStack.push(Qt.resolvedUrl(page));
        else pageStack.push(Qt.resolvedUrl(page), { settings: gs });
    }
    Timer { interval: 1500; running: true; onTriggered: later.start() }
    Timer { id: later; interval: 500; onTriggered: mv.grabToImage(function(r) { r.saveToFile(Shot.out); Qt.quit(); }) }
}
