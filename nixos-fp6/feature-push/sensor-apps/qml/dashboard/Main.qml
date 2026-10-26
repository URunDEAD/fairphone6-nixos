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

    readonly property var d: feed.data
    readonly property var groups: d.groups || []
    readonly property var proxy: d.proxy || ({})
    readonly property var lineColors: [LomiriColors.red, LomiriColors.green, LomiriColors.blue, LomiriColors.ash]

    SensorFeed {
        id: feed
        kind: "dashboard"
        interval: 100
        active: Qt.application.state === Qt.ApplicationActive
    }

    Settings {
        id: settings
        category: "dashboard"
        property bool graphs: true
        property bool raw: false
    }

    function decimalsFor(id) {
        return id === "baro" ? 2 : (id === "prox" || id === "rgbc" ? 0 : (id === "light" ? 1 : 2));
    }

    Page {
        anchors.fill: parent
        header: PageHeader {
            id: header
            title: i18n.tr("Sensors")
            trailingActionBar.actions: [
                Action {
                    iconName: settings.graphs ? "view-list-symbolic" : "view-grid-symbolic"
                    text: settings.graphs ? i18n.tr("Hide graphs") : i18n.tr("Show graphs")
                    onTriggered: settings.graphs = !settings.graphs
                },
                Action {
                    iconName: "info"
                    text: settings.raw ? i18n.tr("Hide raw values") : i18n.tr("Show raw values")
                    onTriggered: settings.raw = !settings.raw
                }
            ]
        }

        ListView {
            id: list
            anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            clip: true
            spacing: units.gu(1)
            topMargin: units.gu(1)
            bottomMargin: units.gu(2)
            model: root.groups.length
            header: Item {
                width: list.width
                height: sysCol.height + units.gu(1)
                Column {
                    id: sysCol
                    anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                    Label { text: i18n.tr("System view (iio-sensor-proxy)"); font.weight: Font.DemiBold }
                    InfoRow {
                        name: i18n.tr("Orientation / tilt")
                        value: (proxy.AccelerometerOrientation || "–") + " / " + (proxy.AccelerometerTilt || "–")
                    }
                    InfoRow { name: i18n.tr("Light level"); value: proxy.LightLevel !== undefined ? proxy.LightLevel + " " + (proxy.LightLevelUnit || "") : "–" }
                    InfoRow { name: i18n.tr("Proximity near"); value: proxy.ProximityNear || "–" }
                    InfoRow { name: i18n.tr("Orientation computed here"); value: d.orientation || "–" }
                }
            }
            delegate: Rectangle {
                readonly property var g: root.groups[index] || ({})
                x: units.gu(1)
                width: list.width - units.gu(2)
                height: card.height + units.gu(2)
                radius: units.gu(1)
                color: theme.palette.normal.foreground

                Column {
                    id: card
                    anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1) }
                    spacing: units.gu(0.5)
                    Row {
                        width: parent.width
                        Label {
                            width: parent.width / 2
                            text: g.name || ""
                            font.weight: Font.DemiBold
                        }
                        Label {
                            width: parent.width / 2
                            horizontalAlignment: Text.AlignRight
                            text: g.device || ""
                            textSize: Label.XSmall
                            elide: Text.ElideLeft
                            color: theme.palette.normal.backgroundSecondaryText
                        }
                    }
                    Flow {
                        width: parent.width
                        spacing: units.gu(2)
                        Repeater {
                            model: g.labels || []
                            Row {
                                spacing: units.gu(0.5)
                                Rectangle {
                                    visible: (g.labels || []).length > 1
                                    width: units.gu(1); height: width; radius: width / 2
                                    anchors.verticalCenter: parent.verticalCenter
                                    color: root.lineColors[index % root.lineColors.length]
                                }
                                Label {
                                    text: modelData + " " + Fmt.num((g.values || [])[index], root.decimalsFor(g.id)) + " " + (g.unit || "")
                                          + (settings.raw && g.raw && g.raw[index] !== null && g.raw[index] !== undefined ? "  [" + g.raw[index] + "]" : "")
                                    font.family: "Ubuntu Mono"
                                }
                            }
                        }
                    }
                    Graph {
                        visible: settings.graphs
                        width: parent.width
                        height: units.gu(11)
                        decimals: root.decimalsFor(g.id)
                        minSpan: g.id === "baro" ? 0.2 : (g.id === "temp" ? 1 : 0.5)
                        unit: g.unit || ""
                        series: (g.hist || []).map(function (h, i) { return { values: h, color: root.lineColors[i % root.lineColors.length] } })
                    }
                    Label {
                        visible: (g.extra || "") !== ""
                        width: parent.width
                        wrapMode: Text.Wrap
                        text: g.extra || ""
                        textSize: Label.XSmall
                        color: theme.palette.normal.backgroundSecondaryText
                    }
                }
            }
        }
    }
}
