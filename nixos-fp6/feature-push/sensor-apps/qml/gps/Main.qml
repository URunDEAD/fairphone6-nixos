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
    readonly property var sky: d.sky || ({})
    readonly property var sats: sky.sats || []
    readonly property var colors: ({ "GPS": LomiriColors.blue, "GLONASS": LomiriColors.red, "Galileo": LomiriColors.green,
                                     "BeiDou": LomiriColors.orange, "QZSS": LomiriColors.purple, "NavIC": LomiriColors.warmGrey })
    function satColor(c) { return colors[c] || LomiriColors.ash }

    // time to first fix, measured from when this app started asking
    property real sessionStart: 0
    property real ttff: NaN
    readonly property bool hasFix: pos.position.latitudeValid && pos.position.longitudeValid
                                   && pos.position.timestamp.getTime() >= sessionStart - 2000
    property var formats: ({})

    SensorFeed {
        id: feed
        kind: "gps"
        interval: 500
        active: root.appActive
    }

    Settings {
        id: settings
        category: "gps"
        property int format: 0
    }

    PositionSource {
        id: pos
        active: root.appActive && root.locationOn
        updateInterval: 1000
        preferredPositioningMethods: PositionSource.SatellitePositioningMethods
        onActiveChanged: if (active) { root.sessionStart = Date.now(); root.ttff = NaN }
        onPositionChanged: {
            // QtPositioning first replays its cached last position: only
            // positions from this session count as a fix.
            if (position.latitudeValid && position.longitudeValid && position.timestamp.getTime() >= root.sessionStart - 2000) {
                if (!isFinite(root.ttff) && root.sessionStart > 0) root.ttff = (Date.now() - root.sessionStart) / 1000;
                root.formats = feed.command("format", position.coordinate.latitude + "," + position.coordinate.longitude);
            }
        }
    }

    readonly property var formatKeys: ["dd", "dms", "ddm", "utm", "mgrs"]
    readonly property var formatNames: [i18n.tr("Decimal degrees"), i18n.tr("Degrees, minutes, seconds"), i18n.tr("Degrees, decimal minutes"), "UTM (WGS84)", "MGRS"]

    Page {
        anchors.fill: parent
        header: PageHeader {
            id: header
            title: i18n.tr("GPS status")
            trailingActionBar.actions: [
                Action {
                    iconName: "edit-copy"
                    text: i18n.tr("Copy coordinates")
                    enabled: root.hasFix
                    onTriggered: Clipboard.push(root.formats[root.formatKeys[settings.format]] || "")
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
                          : i18n.tr("Location is off. GPS status needs Location turned on (status bar or System Settings › Security & Privacy › Location). Turning it on starts the modem's GNSS receiver; turning it off stops it.")
                    buttonText: d.location === "starting" ? "" : i18n.tr("Open location settings")
                    onClicked: Qt.openUrlExternally("settings:///system/location")
                }

                // ---- fix summary -----------------------------------------------------
                Row {
                    width: parent.width
                    visible: root.locationOn
                    Reading {
                        width: parent.width / 3
                        caption: i18n.tr("Fix")
                        value: !root.hasFix ? i18n.tr("none") : (pos.position.altitudeValid ? "3D" : "2D")
                        valueColor: root.hasFix ? theme.palette.normal.positive : theme.palette.normal.negative
                    }
                    Reading {
                        width: parent.width / 3
                        caption: i18n.tr("Accuracy")
                        // GeoClue's NMEA source may report 0 (unknown) accuracy
                        value: pos.position.horizontalAccuracyValid && pos.position.horizontalAccuracy > 0 ? "±" + Fmt.num(pos.position.horizontalAccuracy, 0) + " m" : "–"
                    }
                    Reading {
                        width: parent.width / 3
                        caption: i18n.tr("Satellites")
                        value: (sky.used !== undefined && sky.used !== null ? sky.used + "/" : "") + (sky.tracked || 0) + "/" + (sky.visible || 0)
                    }
                }
                Label {
                    visible: root.locationOn
                    textSize: Label.XSmall
                    color: theme.palette.normal.backgroundTertiaryText
                    text: i18n.tr("satellites: used / with signal / in view")
                }

                // ---- coordinates -------------------------------------------------------
                Rectangle {
                    visible: root.locationOn
                    width: parent.width
                    height: cc.height + units.gu(2)
                    radius: units.gu(1)
                    color: theme.palette.normal.foreground
                    Column {
                        id: cc
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1) }
                        Label {
                            text: root.formatNames[settings.format] + " ▾"
                            textSize: Label.Small
                            color: theme.palette.normal.backgroundSecondaryText
                        }
                        Label {
                            width: parent.width
                            wrapMode: Text.WrapAnywhere
                            text: root.hasFix ? (root.formats[root.formatKeys[settings.format]] || "–") : i18n.tr("waiting for a fix…")
                            textSize: Label.Large
                        }
                    }
                    MouseArea {
                        anchors.fill: parent
                        onClicked: settings.format = (settings.format + 1) % root.formatKeys.length
                    }
                }

                Column {
                    width: parent.width
                    visible: root.locationOn
                    InfoRow {
                        name: i18n.tr("Altitude")
                        value: pos.position.altitudeValid
                               ? Fmt.num(pos.position.coordinate.altitude, 0) + " m" + (pos.position.verticalAccuracyValid ? " ±" + Fmt.num(pos.position.verticalAccuracy, 0) : "")
                               : "–"
                    }
                    InfoRow {
                        name: i18n.tr("Speed")
                        value: pos.position.speedValid ? Fmt.num(pos.position.speed * 3.6, 1) + " km/h" : "–"
                    }
                    InfoRow {
                        name: i18n.tr("Course")
                        value: pos.position.directionValid ? Fmt.num(pos.position.direction, 0) + "° " + Fmt.compassPoint(pos.position.direction) : "–"
                    }
                    InfoRow {
                        name: i18n.tr("HDOP")
                        value: sky.hdop !== undefined && sky.hdop !== null ? Fmt.num(sky.hdop, 1) : "–"
                    }
                    InfoRow {
                        name: i18n.tr("Time to fix")
                        value: isFinite(root.ttff) ? Fmt.num(root.ttff, 1) + " s"
                               : (pos.active ? i18n.tr("searching %1 s").arg(Math.round((clock.now - root.sessionStart) / 1000)) : "–")
                    }
                    InfoRow {
                        name: i18n.tr("Last fix")
                        value: root.hasFix ? Qt.formatTime(pos.position.timestamp, "hh:mm:ss") : "–"
                    }
                }

                // ---- sky plot ------------------------------------------------------------
                SkyPlot {
                    visible: root.locationOn
                    width: Math.min(parent.width, units.gu(40))
                    height: width
                    anchors.horizontalCenter: parent.horizontalCenter
                    sats: root.sats
                    colorFor: root.satColor
                }

                // ---- SNR bars per constellation ------------------------------------
                Label {
                    visible: root.locationOn
                    text: i18n.tr("Signal (C/N₀, dB-Hz)")
                    font.weight: Font.DemiBold
                }
                Label {
                    visible: root.locationOn && root.sats.length === 0
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: d.feed === "connected" ? i18n.tr("No satellites reported yet. Indoors the receiver may see none.")
                                                 : i18n.tr("Satellite feed unavailable (%1).").arg(d.feedError || d.feed || "")
                }
                Repeater {
                    model: root.locationOn ? ["GPS", "GLONASS", "Galileo", "BeiDou", "QZSS", "NavIC", "other"] : []
                    Column {
                        id: grp
                        readonly property var list: root.sats.filter(function (s) { return s.c === modelData })
                        visible: list.length > 0
                        width: col.width
                        spacing: units.gu(0.5)
                        Label {
                            text: modelData + " · " + grp.list.length
                            textSize: Label.Small
                            color: root.satColor(modelData)
                        }
                        Flow {
                            width: parent.width
                            spacing: units.gu(0.5)
                            Repeater {
                                model: grp.list
                                Column {
                                    width: units.gu(3.4)
                                    Item {
                                        width: parent.width
                                        height: units.gu(8)
                                        Rectangle {
                                            anchors.bottom: parent.bottom
                                            width: parent.width
                                            height: Math.max(units.dp(2), parent.height * Math.min(1, (modelData.snr || 0) / 50))
                                            radius: units.dp(2)
                                            color: root.satColor(modelData.c)
                                            opacity: modelData.snr ? 1 : 0.3
                                        }
                                        Label {
                                            anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: units.dp(2) }
                                            text: modelData.snr ? Math.round(modelData.snr) : ""
                                            textSize: Label.XSmall
                                            color: "white"
                                        }
                                    }
                                    Label {
                                        anchors.horizontalCenter: parent.horizontalCenter
                                        text: modelData.prn
                                        textSize: Label.XSmall
                                    }
                                }
                            }
                        }
                    }
                }
                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.XSmall
                    color: theme.palette.normal.backgroundTertiaryText
                    text: i18n.tr("Position, speed and altitude come from GeoClue (allowed while Location is on). Satellites come from the modem's GNSS receiver through a read-only feed without position data.")
                }
            }
        }
    }

    Timer {
        id: clock
        property real now: Date.now()
        interval: 1000
        repeat: true
        running: pos.active && !isFinite(root.ttff)
        onTriggered: now = Date.now()
    }
}
