import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3

ItemPage {
    id: page
    title: "Run self-test"
    flickable: flick
    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        clip: true
        Column {
            id: content
            x: units.gu(2)
            y: units.gu(2)
            width: parent.width - units.gu(4)
            spacing: units.gu(2)
            Label { text: "Check this phone after an update"; fontSize: "large"; width: parent.width; wrapMode: Text.Wrap }
            Label { text: "Check system health, installed features, settings and app launchers. No settings are changed, no audio is played and no data is uploaded."; width: parent.width; wrapMode: Text.Wrap }
            Label { text: "Results open in Self-test and are saved privately in Documents / Self-tests."; width: parent.width; wrapMode: Text.Wrap; color: "#666666" }
            Button { text: "Run self-test"; width: parent.width; color: "#e95420"; onClicked: { if (!Qt.openUrlExternally("fp6-selftest:run")) failure.visible = true; } }
            Label { id: failure; visible: false; text: "Could not open Self-test. Open it from the app drawer."; width: parent.width; wrapMode: Text.Wrap }
        }
    }
}
