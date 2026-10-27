// Preview of the whole-screen accessibility effects at the FP6 size
// (1116x2484, 24 px/gu): a "shell" item (System Settings > Colour correction
// + a hue chart) under the Magnifier's Scale/Translate exactly as ShellScreen
// binds them, the real FP6.DisplayComfort ColorFilter on top with the
// FP6.Accessibility ColourMatrix as preMatrix, and the magnified frame.
// Grabbed by fp6-a11y-previewer (window read-back, so the GL filter pass is in it).
import QtQuick 2.15
import Lomiri.Components 1.3
import GSettings 1.0
import FP6.DisplayComfort 1.0 as DC
import FP6.Accessibility 1.0 as A11y
import "Shot.js" as Shot

Item {
    id: screen
    width: 1116
    height: 2484
    readonly property string shot: Shot.shot

    GSettings { id: gs; schema.id: "com.fairphone6.accessibility" }

    Item {
        id: shell
        width: screen.width
        height: screen.height
        transform: [
            Scale { xScale: mag.zoom; yScale: mag.zoom },
            Translate { x: -mag.originX * mag.zoom; y: -mag.originY * mag.zoom }
        ]
        MainView {
            id: mv
            width: parent.width
            height: units.gu(76)
            applicationName: "lomiri-system-settings"
            Rectangle { anchors.fill: parent; color: theme.palette.normal.background; z: -1 }
            PageStack { id: pageStack }
            Component.onCompleted: pageStack.push(Qt.resolvedUrl("ColourCorrectionPage.qml"), { settings: gs })
        }
        // hue x lightness chart
        Column {
            anchors { top: mv.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            Repeater {
                model: 8
                Row {
                    readonly property int r: index
                    Repeater {
                        model: 24
                        Rectangle {
                            width: screen.width / 24
                            height: (screen.height - units.gu(76)) / 8
                            color: Qt.hsla(index / 24, 0.85, 0.15 + r * 0.1, 1)
                        }
                    }
                }
            }
        }
    }

    A11y.ColourMatrix {
        id: cm
        inversion: screen.shot.indexOf("inversion") === 0
        correction: ["deuteranomaly", "protanomaly", "tritanomaly", "greyscale"].indexOf(screen.shot) >= 0 ? screen.shot
                    : (screen.shot.indexOf("-after") > 0 ? screen.shot.replace("sim-", "").replace("-after", "") : "none")
    }
    DC.ColorFilter {
        id: filter
        anchors.fill: parent
        z: 1000000
        visible: active
        temperature: screen.shot === "inversion-nightlight" ? 3400 : 6500
        // sim-*: what a viewer with that deficiency sees (before/after correction)
        preMatrix: screen.shot.indexOf("sim-") === 0
                   ? cm.simulated(screen.shot.replace("sim-", "").replace("-before", "").replace("-after", ""))
                   : cm.matrix
    }
    A11y.Magnifier { id: mag; anchors.fill: parent; animationMs: 0 }
    Rectangle {
        anchors.fill: parent
        z: 999999
        visible: mag.zoomed
        color: "transparent"
        border.width: units.dp(3)
        border.color: "#E95420"
    }
    Component.onCompleted: {
        theme.name = "Lomiri.Components.Themes.Ambiance";
        if (shot === "magnified") mag.setView(2.5, 80, 260);
    }
}
