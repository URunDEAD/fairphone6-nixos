// Offscreen preview of Settings > Background & Appearance with the FP6
// wallpapers changes (test/preview.sh "settings"). LomiriBackgroundPanel and
// Content Hub are stubs; Fp6.Wallpapers is the real plugin (x86 build) on a
// temporary XDG_GREETER_DATA_DIR.
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import "Shot.js" as Shot

MainView {
    id: view
    width: 1116
    height: 2484
    applicationName: "lomiri-system-settings"

    AdaptivePageLayout {
        id: pageStack
        anchors.fill: parent
        primaryPageSource: Qt.resolvedUrl("background/MainPage.qml")
        // lomiri-system-settings' page stack API used by MainPage.preview().
        function addFileToNextColumnSync(source, url, props) {
            var page = Qt.createComponent(url).createObject(pageStack, props);
            addPageToNextColumn(source, page);
            return page;
        }
    }

    Timer {
        interval: 2500; running: true
        onTriggered: {
            if (Shot.shot === "settings-dialog") {
                var main = pageStack.primaryPage;
                main.preview({ uri: "file://" + Shot.pick });
                setTimer.start();
                return;
            }
            if (Shot.shot.indexOf("scrolled") >= 0) {
                pageStack.primaryPage.flickable.contentY = 1150;
                shotTimer.start();
                return;
            }
            view.grabToImage(function(r) { r.saveToFile(Shot.out); Qt.quit(); });
        }
    }
    Timer {
        id: setTimer
        interval: 1500
        onTriggered: {
            var items = [];
            function walk(it) { if (!it) return; if (it.objectName === "fp6PreviewPage") items.push(it); for (var i = 0; i < it.children.length; i++) walk(it.children[i]); }
            walk(pageStack);
            if (items.length > 0) items[0].fp6OpenWhere();
            shotTimer.start();
        }
    }
    Timer {
        id: shotTimer
        interval: 1200
        onTriggered: view.grabToImage(function(r) { r.saveToFile(Shot.out); Qt.quit(); })
    }
}
