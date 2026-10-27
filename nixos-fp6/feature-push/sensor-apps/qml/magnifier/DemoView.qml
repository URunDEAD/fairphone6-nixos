// Laptop preview stand-in for the camera (no camera is opened there):
// a page of small print, scaled like the viewfinder.
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    id: view
    property real zoom: 1
    property bool active: false
    readonly property bool ready: true
    readonly property string error: ""
    clip: true
    function focusAt(x, y) {}

    Rectangle {
        anchors.fill: parent
        color: "#f4efe3"
        scale: view.zoom
        Column {
            anchors.centerIn: parent
            width: parent.width - units.gu(4)
            spacing: units.gu(1)
            Text {
                width: parent.width
                text: "INGREDIENTS"
                font.pixelSize: units.gu(1.6); font.bold: true; color: "#222"
            }
            Text {
                width: parent.width
                wrapMode: Text.Wrap
                font.pixelSize: units.gu(1.1)
                color: "#333"
                text: "Wheat flour, water, sugar, sunflower oil, yeast, salt, emulsifier (E471), flour treatment agent (E300). May contain traces of sesame, soya and nuts. Store in a cool, dry place. Best before: see top of pack. Nutrition per 100 g: energy 1067 kJ / 253 kcal, fat 3.1 g, of which saturates 0.4 g, carbohydrate 46 g, of which sugars 4.5 g, fibre 2.7 g, protein 8.6 g, salt 0.98 g."
            }
            Row {
                spacing: units.gu(0.5)
                Repeater {
                    model: ["#c0392b", "#e67e22", "#f1c40f", "#27ae60", "#2980b9", "#8e44ad"]
                    Rectangle { width: units.gu(4); height: units.gu(2); color: modelData }
                }
            }
            Text {
                width: parent.width
                wrapMode: Text.Wrap
                font.pixelSize: units.gu(0.9)
                color: "#555"
                text: "Lot 2610-27 · 500 g ℮ · Recycle the bag with plastic film. Packed in a protective atmosphere."
            }
        }
    }
}
