// Offscreen preview of System Settings > USB (the real page) and of the
// connection notification / "Use USB for" question (mock of Lomiri's
// notification bubble with the agent's exact texts).
import QtQuick 2.12
import Lomiri.Components 1.3
import GSettings 1.0
import "Shot.js" as Shot

MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(103.5)
    applicationName: "lomiri-system-settings"
    theme.name: Shot.dark ? "Lomiri.Components.Themes.SuruDark" : "Lomiri.Components.Themes.Ambiance"
    Rectangle { anchors.fill: parent; color: theme.palette.normal.background; z: -1 }
    GSettings { id: gs; schema.id: "com.fairphone6.usb-modes" }
    PageStack {
        id: pageStack
        visible: Shot.kind === "page"
        Component.onCompleted: {
            if (Shot.mode) gs.defaultWhenConnected = Shot.mode;
            if (Shot.kind === "page") push(Qt.resolvedUrl("PageComponent.qml"), { statusUrl: Shot.status });
        }
    }
    // Notification mock (Lomiri ephemeral / snap decision layout).
    Rectangle {
        visible: Shot.kind !== "page"
        anchors.fill: parent
        color: "#2c2c2c"
        Rectangle {
            id: bubble
            anchors { top: parent.top; left: parent.left; right: parent.right; margins: units.gu(1); topMargin: units.gu(4) }
            height: col.height + units.gu(3)
            radius: units.gu(1)
            color: theme.palette.normal.overlay
            Column {
                id: col
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1.5) }
                spacing: units.gu(1)
                Row {
                    spacing: units.gu(1.5)
                    Icon { name: "stock_usb"; width: units.gu(4); height: width; color: theme.palette.normal.overlayText }
                    Column {
                        width: bubble.width - units.gu(8.5)
                        Label { text: Shot.summary; font.weight: Font.Medium; color: theme.palette.normal.overlayText; width: parent.width; elide: Text.ElideRight }
                        Label { text: Shot.body; fontSize: "small"; color: theme.palette.normal.overlayText; width: parent.width; wrapMode: Text.WordWrap }
                    }
                }
                Repeater {
                    model: Shot.actions
                    Button {
                        width: col.width
                        text: modelData
                        color: index === 0 ? theme.palette.normal.positive : theme.palette.normal.base
                    }
                }
            }
        }
    }
    Timer {
        interval: 2200; running: true
        onTriggered: {
            if (Shot.scroll && pageStack.currentPage) pageStack.currentPage.flickable.contentY = units.gu(Shot.scroll);
            later.start();
        }
    }
    Timer { id: later; interval: 500; onTriggered: mv.grabToImage(function(r) { r.saveToFile(Shot.out); Qt.quit(); }) }
}
