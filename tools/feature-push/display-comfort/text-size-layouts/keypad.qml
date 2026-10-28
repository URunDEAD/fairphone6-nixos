// The installed Phone app's DialerPage/Keypad.qml + KeypadButton.qml (copied
// into ./dialer by run.sh) at the phone's keypad size; grab.png and
// LAYOUT-RESULT with the lowest label bottom relative to its key.
import QtQuick 2.12
import Lomiri.Components 1.3
import "dialer"

Rectangle {
    id: root
    width: 372 * units.gu(1) / 8
    height: units.gu(44)
    color: "white"
    Keypad {
        id: keypad
        anchors { fill: parent; margins: units.gu(2) }
        showVoicemail: true
        labelPixelSize: units.dp(30)
        spacing: 5
    }
    function labelsOf(item, out) {
        for (var i = 0; i < item.children.length; i++) {
            var c = item.children[i];
            if (c.hasOwnProperty("textSize") && c.text !== "") out.push(c);
            labelsOf(c, out);
        }
        return out;
    }
    Timer {
        interval: 800; running: true
        onTriggered: {
            var worst = 0, keyH = keypad.keysHeight;
            var ls = labelsOf(keypad, []);
            for (var i = 0; i < ls.length; i++) {
                var b = ls[i].mapToItem(ls[i].parent.parent, 0, 0).y + ls[i].contentHeight;
                worst = Math.max(worst, b - ls[i].parent.parent.height);
            }
            console.log("LAYOUT-RESULT " + JSON.stringify({ scale: FontUtils.sizeToPixels("medium") / units.dp(14), keyHeight: keyH, labels: ls.length, labelBelowKey: Math.max(0, worst) }));
            root.grabToImage(function (g) { g.saveToFile("grab.png"); Qt.quit(); });
        }
    }
}
