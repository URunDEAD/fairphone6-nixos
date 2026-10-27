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

    // command replies carry the same state as the feed, so taps show at once
    property var ls: ({})
    readonly property var d: ls
    readonly property string mode: d.mode || "off"
    readonly property bool lit: mode !== "off"
    readonly property var modes: ["on", "strobe", "sos"]
    property int pattern: 0     // 0 steady, 1 strobe, 2 SOS

    SensorFeed {
        id: feed
        kind: "torch"
        interval: 250
        active: Qt.application.state === Qt.ApplicationActive
        onUpdated: root.ls = data
    }

    Settings {
        id: settings
        category: "flashlight"
        property real level: 50
        property real strobeHz: 5
    }

    Component.onCompleted: {
        feed.command("setLevel", String(settings.level));
        root.ls = feed.command("setStrobe", String(settings.strobeHz));
        if (fp6Preview === "strobe" || fp6Preview === "sos") {
            pattern = fp6Preview === "strobe" ? 1 : 2;
            root.ls = feed.command("setMode", fp6Preview);
        } else if (fp6Preview === "on") {
            root.ls = feed.command("setMode", "on");
        }
    }

    function toggle() {
        root.ls = feed.command("setMode", root.lit ? "off" : root.modes[root.pattern]);
    }

    Page {
        anchors.fill: parent
        header: PageHeader {
            id: header
            title: i18n.tr("Flashlight")
            extension: Sections {
                anchors { left: parent.left; bottom: parent.bottom }
                model: [i18n.tr("Steady"), i18n.tr("Strobe"), "SOS"]
                selectedIndex: root.pattern
                onSelectedIndexChanged: {
                    root.pattern = selectedIndex;
                    if (root.lit) root.ls = feed.command("setMode", root.modes[selectedIndex]);
                }
            }
        }

        Flickable {
            anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            contentHeight: col.height + units.gu(4)
            clip: true
            Column {
                id: col
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                spacing: units.gu(3)

                Notice {
                    visible: d.error !== undefined
                    iconName: "dialog-warning-symbolic"
                    accent: theme.palette.normal.negative
                    text: i18n.tr("The flashlight LED cannot be controlled: %1").arg(d.error || "")
                }

                Item { width: 1; height: units.gu(1) }

                // big round switch
                AbstractButton {
                    id: power
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: units.gu(22); height: width
                    onClicked: root.toggle()
                    Rectangle {
                        anchors.centerIn: parent
                        width: parent.width + units.gu(3); height: width; radius: width / 2
                        color: "transparent"
                        border.width: units.gu(0.6)
                        border.color: LomiriColors.orange
                        opacity: root.lit ? 0.35 : 0
                        Behavior on opacity { NumberAnimation { duration: 150 } }
                    }
                    Rectangle {
                        anchors.fill: parent
                        radius: width / 2
                        color: root.lit ? LomiriColors.orange : theme.palette.normal.foreground
                        border.width: root.lit ? 0 : units.dp(2)
                        border.color: theme.palette.normal.base
                        opacity: power.pressed ? 0.8 : 1
                        Behavior on color { ColorAnimation { duration: 150 } }
                    }
                    Icon {
                        anchors.centerIn: parent
                        width: units.gu(9); height: width
                        name: root.lit ? "torch-on" : "torch-off"
                        color: root.lit ? "white" : theme.palette.normal.backgroundText
                    }
                }

                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: !root.lit ? i18n.tr("Off – tap to turn on")
                          : root.mode === "on" ? i18n.tr("On")
                          : root.mode === "strobe" ? i18n.tr("Strobe, %1 flashes/s").arg(Fmt.num(settings.strobeHz, 0))
                          : i18n.tr("Signalling SOS")
                    textSize: Label.Large
                }

                Column {
                    width: parent.width
                    spacing: units.gu(1)
                    Row {
                        width: parent.width
                        spacing: units.gu(1)
                        Icon { name: "display-brightness-min"; width: units.gu(2.5); height: width; anchors.verticalCenter: parent.verticalCenter }
                        Slider {
                            id: levelSlider
                            width: parent.width - units.gu(7)
                            minimumValue: 1
                            maximumValue: 100
                            value: settings.level
                            live: true
                            function formatValue(v) { return v.toFixed(0) + " %" }
                            onValueChanged: {
                                settings.level = Math.round(value);
                                feed.command("setLevel", String(settings.level));
                            }
                        }
                        Icon { name: "display-brightness-max"; width: units.gu(2.5); height: width; anchors.verticalCenter: parent.verticalCenter }
                    }
                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: i18n.tr("Brightness %1 %").arg(settings.level.toFixed(0))
                        color: theme.palette.normal.backgroundSecondaryText
                    }
                }

                Column {
                    width: parent.width
                    spacing: units.gu(1)
                    visible: root.pattern === 1
                    Label { text: i18n.tr("Strobe speed: %1 flashes per second").arg(settings.strobeHz.toFixed(0)) }
                    Slider {
                        width: parent.width
                        minimumValue: 1
                        maximumValue: 15
                        value: settings.strobeHz
                        live: true
                        function formatValue(v) { return v.toFixed(0) }
                        onValueChanged: {
                            settings.strobeHz = Math.round(value);
                            feed.command("setStrobe", String(settings.strobeHz));
                        }
                    }
                    Notice {
                        iconName: "dialog-warning-symbolic"
                        accent: LomiriColors.orange
                        text: i18n.tr("Flashing light can trigger seizures in people with photosensitive epilepsy. Do not point it at people or drivers.")
                    }
                }

                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("The steady light stays on when you leave the app; turn it off here or from the status bar (battery menu › Flashlight). Strobe and SOS stop when you leave the app.")
                }
            }
        }
    }
}
