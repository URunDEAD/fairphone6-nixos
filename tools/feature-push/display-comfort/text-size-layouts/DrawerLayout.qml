// Layout check of the Lomiri app drawer cells and the panel labels at a
// given text size: the geometry of qml/Launcher/Drawer.qml (drawer delegate
// Column: 6 gu icon at 7.5/8, 1 gu spacing, "small" label, 2 lines) and
// qml/Panel/Indicators/IndicatorItem.qml (3 gu panel, "medium" labels; the
// patch leaves the panel alone), stock and with lomiri-text-size.patch
// (drawer-patched.qml). Synthetic app names.
// Run with GRID_UNIT_PX=24 (the shell's grid unit), window 1116 px wide.
// Prints LAYOUT-RESULT {overlaps, elided, panelOverflow} and saves grab.png.
import QtQuick 2.12
import Lomiri.Components 1.3

Rectangle {
    id: root
    width: 1116
    height: 2000
    color: "#202020"

    property bool patched: false
    readonly property real textScale: Math.max(1, FontUtils.sizeToPixels("medium") / units.dp(14))
    readonly property real delegateWidth: patched ? units.gu(10) * textScale : units.gu(10)
    readonly property real delegateHeight: patched ? units.gu(11) + (textScale - 1) * units.gu(4) : units.gu(11)
    readonly property var names: ["Phone", "Messages", "Contacts", "Camera", "Gallery", "Calculator",
        "Clock", "Calendar", "System Settings", "File Manager", "Media Player", "Pure Maps",
        "Terminal", "Weather", "Document Viewer", "Music", "Notes", "Firefox"]

    // Panel (status bar): fixed 3 gu.
    Rectangle {
        id: panel
        width: parent.width; height: units.gu(3); color: "black"
        Row {
            id: panelRow
            anchors { right: parent.right; rightMargin: units.gu(1); verticalCenter: parent.verticalCenter }
            spacing: units.gu(1)
            Repeater {
                model: ["87%", "12:45"]
                Label {
                    id: pl
                    text: modelData; color: "white"; font.weight: Font.Light
                    fontSize: "medium"
                }
            }
        }
    }

    GridView {
        id: grid
        anchors { top: panel.bottom; topMargin: units.gu(2); left: parent.left; right: parent.right; bottom: parent.bottom }
        readonly property int columns: Math.floor(width / root.delegateWidth)
        cellWidth: root.delegateWidth + (width - columns * root.delegateWidth) / columns
        cellHeight: root.delegateHeight
        interactive: false
        model: root.names
        delegate: Item {
            width: grid.cellWidth
            height: root.delegateHeight
            property alias label: label
            property alias column: column
            Column {
                id: column
                width: root.patched ? root.delegateWidth - units.gu(1) : units.gu(9)
                anchors.horizontalCenter: parent.horizontalCenter
                height: childrenRect.height
                spacing: units.gu(1)
                Rectangle {
                    width: units.gu(6); height: 7.5 / 8 * width; radius: units.gu(1.5)
                    anchors.horizontalCenter: parent.horizontalCenter
                    color: "#e95420"
                }
                Label {
                    id: label
                    text: modelData
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    fontSize: "small"
                    wrapMode: Text.WordWrap
                    maximumLineCount: 2
                    elide: Text.ElideRight
                    color: "white"
                }
            }
        }
    }

    Timer {
        interval: 500; running: true
        onTriggered: {
            var overlaps = 0, elided = [];
            for (var i = 0; i < grid.count; i++) {
                var it = grid.itemAtIndex(i);
                if (!it) continue;
                // Label bottom (painted) beyond the cell = runs into the next row.
                var bottom = it.column.y + it.label.y + it.label.contentHeight;
                if (bottom > root.delegateHeight) overlaps++;
                if (it.label.truncated) elided.push(it.label.text);
            }
            var panelOverflow = 0;
            for (var j = 0; j < panelRow.children.length; j++) {
                var c = panelRow.children[j];
                if (c.contentHeight) panelOverflow = Math.max(panelOverflow, c.contentHeight - panel.height);
            }
            console.log("LAYOUT-RESULT " + JSON.stringify({ scale: root.textScale, patched: root.patched,
                columns: grid.columns, cellHeight: root.delegateHeight, overlaps: overlaps, elided: elided,
                panelOverflow: panelOverflow }));
            root.grabToImage(function (r) { r.saveToFile("grab.png"); Qt.quit(); });
        }
    }
}
