// Live UITK font scale check (offscreen, software Qt Quick). The test script
// changes $XDG_CONFIG_HOME/lomiri-ui-toolkit/font-scale while this runs; the
// sizes are printed before (t = 0.5 s) and after (t = 3 s) the change, and
// both states are grabbed (before.png, after.png).
import QtQuick 2.12
import Lomiri.Components 1.3

Rectangle {
    id: root
    width: 900
    height: 700
    color: "white"

    Column {
        id: column
        Label { id: medium; text: "Hamburgefonstiv 123"; color: "black" }
        Label { id: large; text: "Hamburgefonstiv 123"; textSize: Label.Large; color: "black" }
        // A binding on FontUtils (re-evaluated through the context property).
        Text { id: fu; text: "Hamburgefonstiv 123"; font.pixelSize: FontUtils.sizeToPixels("large"); color: "black" }
        // An explicit pixel size: must not change.
        Label { id: fixed; text: "fixed"; font.pixelSize: 20; color: "black" }
        Rectangle { id: box; width: units.gu(10); height: units.gu(2); color: "grey" }
    }

    function sizes() {
        return { medium: medium.font.pixelSize, large: large.font.pixelSize, fontUtilsLarge: fu.font.pixelSize,
                 fixed: fixed.font.pixelSize, gu: units.gu(1), box: box.width, columnHeight: column.height };
    }

    Timer {
        interval: 500; running: true
        onTriggered: {
            console.log("UITK-LIVE-BEFORE " + JSON.stringify(root.sizes()));
            console.log("UITK-LIVE-READY");
            column.grabToImage(function (r) { r.saveToFile("before.png"); });
        }
    }
    Timer {
        interval: 3000; running: true
        onTriggered: {
            console.log("UITK-LIVE-AFTER " + JSON.stringify(root.sizes()));
            column.grabToImage(function (r) { r.saveToFile("after.png"); Qt.quit(); });
        }
    }
}
