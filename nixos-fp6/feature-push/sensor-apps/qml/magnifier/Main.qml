import QtQuick 2.12
import Qt.labs.settings 1.0
import Lomiri.Components 1.3
import Fp6.Sensors 1.0
import "../common"
import "../common/Fmt.js" as Fmt

MainView {
    id: root
    applicationName: appId
    width: units.gu(45)
    height: units.gu(80)

    readonly property bool appActive: Qt.application.state === Qt.ApplicationActive
    readonly property bool demo: fp6Preview.indexOf("demo") === 0
    property real zoom: 2
    property bool frozen: false
    property var light: ({})
    readonly property bool lightOn: light.mode === "on"

    // the Magnifier's light: same LED path as the Flashlight
    SensorFeed {
        id: torch
        kind: "torch"
        interval: 500
        active: root.appActive
        onUpdated: root.light = data
    }

    Settings {
        id: settings
        category: "magnifier"
        property real zoom: 2
    }

    Component.onCompleted: {
        zoom = fp6Preview === "demo-zoom" ? 4 : settings.zoom;
        torch.command("setLevel", "60");
    }
    onAppActiveChanged: {
        // light and freeze belong to this view: off when leaving the app
        if (!appActive) {
            if (root.lightOn) root.light = torch.command("setMode", "off");
            root.frozen = false;
        }
    }
    Component.onDestruction: if (root.lightOn) torch.command("setMode", "off")
    onZoomChanged: settings.zoom = zoom

    Page {
        anchors.fill: parent
        header: PageHeader {
            id: header
            title: i18n.tr("Magnifier")
            trailingActionBar.actions: [
                Action {
                    iconName: root.lightOn ? "torch-on" : "torch-off"
                    text: root.lightOn ? i18n.tr("Light off") : i18n.tr("Light on")
                    onTriggered: root.light = torch.command("setMode", root.lightOn ? "off" : "on")
                },
                Action {
                    iconName: root.frozen ? "media-playback-start" : "media-playback-pause"
                    text: root.frozen ? i18n.tr("Live") : i18n.tr("Freeze")
                    onTriggered: root.freeze(!root.frozen)
                }
            ]
        }

        Item {
            id: stage
            anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: panel.top }
            clip: true

            Loader {
                id: viewLoader
                anchors.fill: parent
                source: root.demo ? "DemoView.qml" : "CameraView.qml"
                onLoaded: {
                    item.zoom = Qt.binding(function () { return root.zoom });
                    item.active = Qt.binding(function () { return root.appActive || root.demo });
                }
            }

            Image {
                id: still
                anchors.fill: parent
                visible: root.frozen
                fillMode: Image.PreserveAspectFit
            }

            PinchArea {
                anchors.fill: parent
                enabled: !root.frozen
                property real startZoom: 1
                onPinchStarted: startZoom = root.zoom
                onPinchUpdated: root.zoom = Math.max(1, Math.min(8, startZoom * pinch.scale))
                MouseArea {
                    anchors.fill: parent
                    // tap: focus there; double tap: 1x <-> 4x
                    onClicked: if (viewLoader.item) viewLoader.item.focusAt(mouse.x, mouse.y)
                    onDoubleClicked: root.zoom = root.zoom < 2.5 ? 4 : 1
                }
            }

            Notice {
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                visible: viewLoader.status === Loader.Error || (viewLoader.item && viewLoader.item.error !== "")
                iconName: "camera-photo-symbolic"
                accent: theme.palette.normal.negative
                text: viewLoader.status === Loader.Error ? i18n.tr("The camera stack is not installed.")
                      : viewLoader.item && viewLoader.item.error === "busy" ? i18n.tr("The camera is in use by another app. Close it and come back.")
                      : i18n.tr("The camera is not available: %1").arg(viewLoader.item ? viewLoader.item.error : "")
            }

            Rectangle {
                anchors { right: parent.right; top: parent.top; margins: units.gu(1.5) }
                width: zl.width + units.gu(2); height: zl.height + units.gu(1)
                radius: height / 2
                color: "#80000000"
                Label { id: zl; anchors.centerIn: parent; text: Fmt.num(root.zoom, 1) + "×"; color: "white" }
            }
        }

        Rectangle {
            id: panel
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
            height: pc.height + units.gu(3)
            color: theme.palette.normal.background
            Column {
                id: pc
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                spacing: units.gu(1)
                Row {
                    width: parent.width
                    spacing: units.gu(1)
                    Button {
                        id: minus
                        width: units.gu(5)
                        iconName: "zoom-out"
                        onClicked: root.zoom = Math.max(1, Math.round((root.zoom - 0.5) * 2) / 2)
                    }
                    Slider {
                        width: parent.width - 2 * minus.width - 2 * parent.spacing
                        anchors.verticalCenter: parent.verticalCenter
                        minimumValue: 1
                        maximumValue: 8
                        value: root.zoom
                        live: true
                        function formatValue(v) { return v.toFixed(1) + "×" }
                        onValueChanged: if (pressed) root.zoom = value
                    }
                    Button {
                        width: units.gu(5)
                        iconName: "zoom-in"
                        onClicked: root.zoom = Math.min(8, Math.round((root.zoom + 0.5) * 2) / 2)
                    }
                }
                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("Pinch or drag the slider to zoom, tap to focus, double-tap for 4×. The camera is used only while this app is open in front.")
                }
            }
        }
    }

    function freeze(on) {
        if (!on) { root.frozen = false; return; }
        stage.grabToImage(function (r) { still.source = r.url; root.frozen = true; });
    }
}
