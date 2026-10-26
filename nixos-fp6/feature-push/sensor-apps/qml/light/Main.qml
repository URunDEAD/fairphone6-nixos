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

    property bool hold: false
    property var held: ({})
    readonly property var d: hold ? held : feed.data
    readonly property var isos: [50, 100, 200, 400, 800, 1600, 3200, 6400]

    SensorFeed {
        id: feed
        kind: "light"
        interval: 200
        active: Qt.application.state === Qt.ApplicationActive
    }

    Settings {
        id: settings
        category: "light"
        property int isoIndex: 1
        property real compensation: 0
    }

    Component.onCompleted: {
        feed.command("setIso", String(isos[settings.isoIndex]));
        feed.command("setCompensation", String(settings.compensation));
    }

    function luxText(l) {
        if (l === undefined || l === null || !isFinite(l)) return "–";
        if (l >= 10000) return Fmt.num(l / 1000, 1) + "k";
        return Fmt.num(l, l < 10 ? 1 : 0);
    }

    Page {
        anchors.fill: parent
        header: PageHeader {
            id: header
            title: i18n.tr("Light meter")
            trailingActionBar.actions: [
                Action {
                    iconName: root.hold ? "media-playback-start" : "media-playback-pause"
                    text: root.hold ? i18n.tr("Resume") : i18n.tr("Hold")
                    onTriggered: { root.held = feed.data; root.hold = !root.hold }
                },
                Action {
                    iconName: "reset"
                    text: i18n.tr("Reset min/max")
                    onTriggered: feed.command("reset")
                }
            ]
        }

        Flickable {
            anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            contentHeight: col.height + units.gu(4)
            clip: true
            Column {
                id: col
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                spacing: units.gu(2)

                Notice {
                    visible: d.error !== undefined
                    iconName: "dialog-warning-symbolic"
                    accent: theme.palette.normal.negative
                    text: d.error || ""
                }

                Row {
                    width: parent.width
                    Column {
                        width: parent.width * 0.55
                        Label { text: i18n.tr("Illuminance"); textSize: Label.Small; color: theme.palette.normal.backgroundSecondaryText }
                        Row {
                            spacing: units.gu(1)
                            Label {
                                text: root.luxText(d.lux)
                                font.pixelSize: units.gu(9)
                                font.weight: Font.Light
                            }
                            Label {
                                anchors.baseline: parent.children[0].baseline
                                text: "lx"
                                textSize: Label.Large
                            }
                        }
                    }
                    Column {
                        width: parent.width * 0.45
                        Label { text: i18n.tr("EV (ISO %1)").arg(d.iso || 100); textSize: Label.Small; color: theme.palette.normal.backgroundSecondaryText }
                        Label {
                            text: Fmt.num(d.ev, 1)
                            font.pixelSize: units.gu(6)
                            font.weight: Font.Light
                        }
                        Label {
                            text: d.scene || ""
                            textSize: Label.Small
                            width: parent.width
                            wrapMode: Text.Wrap
                            color: theme.palette.normal.backgroundSecondaryText
                        }
                    }
                }
                Label {
                    width: parent.width
                    visible: d.dark === true
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("0 lx: below the sensor's lowest step, or the sensor is covered.")
                }

                Graph {
                    width: parent.width
                    height: units.gu(14)
                    decimals: 0
                    minSpan: 5
                    fixedMin: 0
                    unit: "lx"
                    leftLabel: i18n.tr("−2 min")
                    rightLabel: i18n.tr("now")
                    series: [{ values: d.history || [], color: LomiriColors.orange }]
                }

                Row {
                    width: parent.width
                    Reading { width: parent.width / 3; caption: i18n.tr("Min"); value: root.luxText(d.min) + " lx" }
                    Reading { width: parent.width / 3; caption: i18n.tr("Max"); value: root.luxText(d.max) + " lx" }
                    Reading { width: parent.width / 3; caption: i18n.tr("EV100"); value: Fmt.num(d.ev100, 1) }
                }

                Label { text: i18n.tr("ISO"); font.weight: Font.DemiBold }
                Sections {
                    width: parent.width
                    model: root.isos.map(function (i) { return String(i) })
                    selectedIndex: settings.isoIndex
                    onSelectedIndexChanged: {
                        settings.isoIndex = selectedIndex;
                        feed.command("setIso", String(root.isos[selectedIndex]));
                    }
                }
                Label { text: i18n.tr("Exposure compensation: %1 EV").arg(Fmt.signed(settings.compensation, 1)) }
                Slider {
                    width: parent.width
                    minimumValue: -3
                    maximumValue: 3
                    value: settings.compensation
                    live: true
                    function formatValue(v) { return Fmt.signed(Math.round(v * 3) / 3, 1) }
                    onValueChanged: {
                        settings.compensation = Math.round(value * 3) / 3;
                        feed.command("setCompensation", String(settings.compensation));
                    }
                }

                Label { text: i18n.tr("Suggested exposures"); font.weight: Font.DemiBold }
                Flow {
                    width: parent.width
                    spacing: units.gu(1)
                    Repeater {
                        model: d.suggestions || []
                        Rectangle {
                            width: (col.width - units.gu(2)) / 3
                            height: units.gu(6)
                            radius: units.gu(1)
                            color: theme.palette.normal.foreground
                            opacity: modelData.inRange ? 1 : 0.45
                            Column {
                                anchors.centerIn: parent
                                Label { anchors.horizontalCenter: parent.horizontalCenter; text: "f/" + modelData.aperture; font.weight: Font.DemiBold }
                                Label { anchors.horizontalCenter: parent.horizontalCenter; text: modelData.label; textSize: Label.Small }
                            }
                        }
                    }
                }

                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("Incident-light reading: the sensor is at the top of the screen, next to the earpiece. Hold the phone at the subject with the screen facing the camera position. Exposure uses C = 250 (EV100 = log2(lux / 2.5)). The sensor's spectral response is not photometric, so treat the result as a guide.")
                }
            }
        }
    }
}
