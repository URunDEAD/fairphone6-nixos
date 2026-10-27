import QtQuick 2.12
import QtPositioning 5.12
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
    readonly property var d: feed.data
    readonly property bool locationOn: d.location === "on"

    // units: km/h, mph, knots, m/s
    readonly property var speedFactor: [3.6, 2.2369363, 1.9438445, 1][settings.unit]
    readonly property var speedUnit: ["km/h", "mph", "kn", "m/s"][settings.unit]
    readonly property var distFactor: [0.001, 0.000621371, 0.000539957, 0.001][settings.unit]
    readonly property var distUnit: ["km", "mi", "nmi", "km"][settings.unit]
    readonly property real gaugeMax: [[60, 120, 200, 300], [40, 80, 120, 200], [20, 40, 80, 160], [20, 40, 60, 100]][settings.unit][settings.scale]

    property real sessionStart: 0
    property real accuracy: NaN
    readonly property bool hasFix: isFinite(lastFixMs) && Date.now() - lastFixMs < 10000 && clock.running
    property real lastFixMs: NaN

    SensorFeed {
        id: feed
        kind: "speed"
        interval: 500
        active: root.appActive
    }

    Settings {
        id: settings
        category: "speed"
        property int unit: 0
        property int scale: 1
    }

    // re-evaluates hasFix while no fixes arrive
    Timer { id: clock; interval: 1000; repeat: true; running: root.appActive; onTriggered: root.lastFixMsChanged() }

    PositionSource {
        id: pos
        active: root.appActive && root.locationOn
        updateInterval: 1000
        preferredPositioningMethods: PositionSource.SatellitePositioningMethods
        onActiveChanged: if (active) root.sessionStart = Date.now()
        onPositionChanged: {
            var p = position;
            // QtPositioning first replays its cached last position: only
            // fixes from this session count.
            if (!(p.latitudeValid && p.longitudeValid) || p.timestamp.getTime() < root.sessionStart - 2000) return;
            root.lastFixMs = Date.now();
            root.accuracy = p.horizontalAccuracyValid ? p.horizontalAccuracy : NaN;
            feed.command("fix", [p.timestamp.getTime(), p.coordinate.latitude, p.coordinate.longitude,
                                 p.speedValid ? p.speed : NaN, root.accuracy].join(","));
        }
    }

    Component.onCompleted: {
        // laptop preview: replay a synthetic NMEA file instead of GeoClue
        var demo = feed.command("demoNmea").file;
        if (demo) pos.nmeaSource = "file://" + demo;
        stack.push(mainPage);
        if (fp6Preview === "settings") stack.push(settingsPage);
    }

    function spd(v) { return isFinite(v) ? v * root.speedFactor : NaN }
    function duration(s) {
        if (!isFinite(s)) return "–";
        var h = Math.floor(s / 3600), m = Math.floor(s % 3600 / 60), sec = Math.floor(s % 60);
        return (h > 0 ? h + ":" + (m < 10 ? "0" : "") : "") + m + ":" + (sec < 10 ? "0" : "") + sec;
    }

    PageStack { id: stack }

    Page {
        id: mainPage
        visible: false
        header: PageHeader {
            id: header
            title: i18n.tr("Speedometer")
            trailingActionBar.actions: [
                Action {
                    iconName: "settings"
                    text: i18n.tr("Settings")
                    onTriggered: stack.push(settingsPage)
                },
                Action {
                    iconName: "reset"
                    text: i18n.tr("Reset trip")
                    onTriggered: feed.command("reset")
                },
                Action {
                    iconName: d.paused ? "media-playback-start" : "media-playback-pause"
                    text: d.paused ? i18n.tr("Resume trip") : i18n.tr("Pause trip")
                    onTriggered: feed.command("pause", d.paused ? "false" : "true")
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
                    visible: d.location !== undefined && !root.locationOn
                    iconName: "location"
                    text: d.location === "starting"
                          ? i18n.tr("Location is turning on…")
                          : i18n.tr("Location is off. The speedometer needs Location turned on (status bar or System Settings › Security & Privacy › Location). Speed comes from the GPS receiver; nothing is saved.")
                    buttonText: d.location === "starting" ? "" : i18n.tr("Open location settings")
                    onClicked: Qt.openUrlExternally("settings:///system/location")
                }

                Notice {
                    visible: root.locationOn && !root.hasFix
                    iconName: "location"
                    accent: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("Waiting for a GPS fix… Outdoors with a view of the sky this takes a few seconds to a minute.")
                }

                ArcGauge {
                    id: gauge
                    width: Math.min(parent.width, units.gu(34))
                    anchors.horizontalCenter: parent.horizontalCenter
                    maximum: root.gaugeMax
                    value: root.hasFix ? root.spd(d.speed) : NaN
                    marker: d.max > 0 ? root.spd(d.max) : NaN
                    color: theme.palette.normal.activity
                    opacity: root.locationOn ? 1 : 0.4

                    Column {
                        anchors.centerIn: parent
                        Label {
                            anchors.horizontalCenter: parent.horizontalCenter
                            text: root.hasFix ? Fmt.num(root.spd(d.speed), root.spd(d.speed) < 10 && settings.unit === 3 ? 1 : 0) : "–"
                            font.pixelSize: units.gu(11)
                            font.weight: Font.Light
                        }
                        Label {
                            anchors.horizontalCenter: parent.horizontalCenter
                            text: root.speedUnit
                            textSize: Label.Large
                            color: theme.palette.normal.backgroundSecondaryText
                        }
                        Label {
                            anchors.horizontalCenter: parent.horizontalCenter
                            visible: d.paused === true
                            text: i18n.tr("trip paused")
                            textSize: Label.Small
                            color: LomiriColors.orange
                        }
                    }
                }

                Row {
                    width: parent.width
                    Reading { width: parent.width / 3; caption: i18n.tr("Max"); value: Fmt.num(root.spd(d.max), 0) + " " + root.speedUnit }
                    Reading { width: parent.width / 3; caption: i18n.tr("Average"); value: Fmt.num(root.spd(d.average), 0) + " " + root.speedUnit }
                    Reading { width: parent.width / 3; caption: i18n.tr("Distance"); value: Fmt.num((d.distance || 0) * root.distFactor, 2) + " " + root.distUnit }
                }

                Graph {
                    width: parent.width
                    height: units.gu(16)
                    fixedMin: 0
                    minSpan: 10
                    decimals: 0
                    unit: root.speedUnit
                    leftLabel: i18n.tr("−5 min")
                    rightLabel: i18n.tr("now")
                    series: [{ values: (d.history || []).map(function (v) { return v * root.speedFactor }), color: theme.palette.normal.activity }]
                }

                Column {
                    width: parent.width
                    InfoRow { name: i18n.tr("Moving time"); value: root.duration(d.movingTime) }
                    InfoRow { name: i18n.tr("Position accuracy"); value: isFinite(root.accuracy) && root.hasFix ? "± " + Fmt.num(root.accuracy, 0) + " m" : "–" }
                }

                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("Speed is the GPS receiver's Doppler speed (or the distance between fixes). Below 2 km/h the phone counts as standing still, so position jitter does not add distance. The trip is kept only while the app is open; nothing is saved.")
                }
            }
        }
    }

    Page {
        id: settingsPage
        visible: false
        header: PageHeader { id: sh; title: i18n.tr("Speedometer settings") }
        Flickable {
            anchors { top: sh.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            contentHeight: sc.height + units.gu(2)
            clip: true
            Column {
                id: sc
                width: parent.width
                spacing: units.gu(2)
                Item { width: 1; height: units.gu(0.5) }
                OptionSelector {
                    width: parent.width - units.gu(4)
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: i18n.tr("Units")
                    model: [i18n.tr("km/h and km"), i18n.tr("mph and miles"), i18n.tr("knots and nautical miles"), i18n.tr("m/s and km")]
                    selectedIndex: settings.unit
                    onSelectedIndexChanged: settings.unit = selectedIndex
                }
                OptionSelector {
                    width: parent.width - units.gu(4)
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: i18n.tr("Dial range")
                    model: [[60, 120, 200, 300], [40, 80, 120, 200], [20, 40, 80, 160], [20, 40, 60, 100]][settings.unit].map(function (v) { return "0–" + v + " " + root.speedUnit })
                    selectedIndex: settings.scale
                    onSelectedIndexChanged: settings.scale = selectedIndex
                }
            }
        }
    }
}
