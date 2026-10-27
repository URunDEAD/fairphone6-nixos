// The control centre (quick-settings' own preview harness Main.qml, with the
// fully patched QsBackend.qml) with the Colour inversion and Magnification
// tiles in the grid. A Magnifier instance makes MagnifierControl available.
import QtQuick 2.15
import GSettings 1.0
import FP6.Accessibility 1.0 as A11y
import "Shot.js" as Shot

Item {
    width: 1116
    height: 2484
    A11y.Magnifier { id: mag; width: 1116; height: 2484; animationMs: 0 }
    GSettings { id: qs; schema.id: "com.fairphone6.quick-settings" }
    GSettings { id: a; schema.id: "com.fairphone6.accessibility" }
    Loader { id: l; anchors.fill: parent; active: false; source: "Main.qml" }
    Component.onCompleted: {
        qs.tiles = ["wifi", "bluetooth", "colour-inversion", "magnification", "mobile-data", "flashlight", "dnd",
                    "night-light", "rotation-lock", "airplane", "location", "battery-saver"];
        if (Shot.shot === "qs-on") { a.colourInversion = true; mag.setView(2, 0, 0); }
        l.active = true;
        if (Shot.shot === "qs-toggletest") toggleTest.start();
    }
    // The two tiles' toggles (the real patched QsBackend): inversion flips the
    // key; magnification asks the panel to close, then zooms in 450 ms later;
    // when zoomed it zooms out at once.
    property int hides: 0
    property bool ok: true
    function check(name, cond) { console.log((cond ? "PASS " : "FAIL ") + name); ok = ok && cond; }
    Timer {
        id: toggleTest
        interval: 50
        onTriggered: {
            var b = l.item.backend;
            b.fp6HideRequested.connect(function() { hides++; });
            var inv = b.tile("colour-inversion"), m = b.tile("magnification");
            check("tiles in the catalogue and available", !!inv && !!m && inv.available && m.available);
            inv.toggle();
            invCheck.start();
        }
    }
    // GSettings change notifications arrive from the main loop
    Timer {
        id: invCheck
        interval: 100
        property int step: 0
        onTriggered: {
            var b = l.item.backend, inv = b.tile("colour-inversion"), m = b.tile("magnification");
            if (step === 0) {
                check("inversion tile sets colour-inversion", a.colourInversion === true && inv.active);
                inv.toggle(); step = 1; start(); return;
            }
            check("inversion tile clears it", a.colourInversion === false && !inv.active);
            m.toggle();
            check("magnification: panel asked to close, zoom not yet", hides === 1 && !mag.zoomed);
            zoomCheck.start();
        }
    }
    Timer {
        id: zoomCheck
        interval: 550
        onTriggered: {
            var m = l.item.backend.tile("magnification");
            check("magnification: zoomed after the panel closed", mag.zoomed && m.active && mag.zoom === 2);
            m.toggle();
            check("magnification tile zooms out at once", !mag.zoomed && !m.active && hides === 2);
            console.log(ok ? "QS TOGGLETEST OK" : "QS TOGGLETEST FAILED");
        }
    }
}
