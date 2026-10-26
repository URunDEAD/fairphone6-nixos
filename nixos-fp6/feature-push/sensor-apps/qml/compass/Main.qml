import QtQuick 2.12
import QtPositioning 5.12
import Qt.labs.settings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import Fp6.Sensors 1.0
import "../common"
import "../common/Fmt.js" as Fmt

MainView {
    id: root
    applicationName: appId
    anchorToKeyboard: true
    width: units.gu(45)
    height: units.gu(80)

    readonly property bool appActive: Qt.application.state === Qt.ApplicationActive
    readonly property var d: feed.data
    readonly property bool hasTrue: d.declination !== undefined && settings.showTrue
    readonly property real shownHeading: hasTrue ? d.trueHeading : (d.heading !== undefined ? d.heading : NaN)

    SensorFeed {
        id: feed
        kind: "compass"
        interval: 40
        active: root.appActive
    }

    Settings {
        id: settings
        category: "compass"
        property bool showTrue: true
        property bool useLocation: true
        property real damping: 0.35
        property real lastLat: NaN
        property real lastLon: NaN
        property real lastAlt: 0
        property string positionSource: ""
    }

    function pushPosition() {
        if (isFinite(settings.lastLat) && isFinite(settings.lastLon))
            feed.command("setPosition", settings.lastLat + "," + settings.lastLon + "," + settings.lastAlt);
    }

    Component.onCompleted: {
        feed.command("setDamping", String(settings.damping));
        pushPosition();
        stack.push(mainPage);
        if (fp6Preview === "calibrate") stack.push(Qt.resolvedUrl("CalibrationPage.qml"), { feed: feed });
        if (fp6Preview === "settings") stack.push(settingsPage);
    }

    // GeoClue position (only while visible and allowed); used for the
    // declination model, remembered for the next start.
    PositionSource {
        id: positionSource
        active: root.appActive && settings.useLocation
        updateInterval: 60000
        onPositionChanged: {
            var c = position.coordinate;
            if (position.latitudeValid && position.longitudeValid) {
                settings.lastLat = c.latitude;
                settings.lastLon = c.longitude;
                settings.lastAlt = position.altitudeValid ? c.altitude : 0;
                // QtPositioning replays its cached last position first
                settings.positionSource = (Date.now() - position.timestamp.getTime() > 600000) ? "cached" : "gps";
                root.pushPosition();
            }
        }
    }

    PageStack {
        id: stack
    }

    Page {
        id: mainPage
        visible: false
        header: PageHeader {
            id: header
            title: i18n.tr("Compass")
            trailingActionBar.actions: [
                Action {
                    iconName: "settings"
                    text: i18n.tr("Settings")
                    onTriggered: stack.push(settingsPage)
                },
                Action {
                    iconName: "reload"
                    text: i18n.tr("Calibrate")
                    onTriggered: stack.push(Qt.resolvedUrl("CalibrationPage.qml"), { feed: feed })
                }
            ]
        }

        Flickable {
            anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            contentHeight: content.height + units.gu(4)
            clip: true

            Column {
                id: content
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                spacing: units.gu(2)

                Notice {
                    visible: d.error !== undefined
                    iconName: "dialog-warning-symbolic"
                    accent: theme.palette.normal.negative
                    text: d.error || ""
                }

                Notice {
                    visible: d.error === undefined && d.calibrated === false && !d.calibrating
                    iconName: "dialog-warning-symbolic"
                    text: i18n.tr("The compass is not calibrated yet. The phone's own magnets offset the sensor; calibrate once by turning the phone in a figure 8.")
                    buttonText: i18n.tr("Calibrate")
                    onClicked: stack.push(Qt.resolvedUrl("CalibrationPage.qml"), { feed: feed })
                }

                Notice {
                    visible: d.interference === true
                    iconName: "dialog-warning-symbolic"
                    accent: theme.palette.normal.negative
                    text: i18n.tr("Magnetic interference: the field is %1 % of the calibrated strength. Move away from metal, magnets or chargers, or recalibrate.").arg(Math.round((d.fieldRel || 0) * 100))
                }

                // heading readout
                Column {
                    width: parent.width
                    spacing: units.gu(0.5)
                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: isFinite(root.shownHeading) ? Math.round(root.shownHeading) % 360 + "° " + Fmt.compassPoint(root.shownHeading) : "–"
                        textSize: Label.XLarge
                        font.pixelSize: units.gu(6)
                        font.weight: Font.Light
                    }
                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        textSize: Label.Small
                        color: theme.palette.normal.backgroundSecondaryText
                        text: (root.hasTrue ? i18n.tr("true north") : i18n.tr("magnetic north"))
                              + " · " + (d.axis === "back" ? i18n.tr("camera direction") : i18n.tr("top edge"))
                    }
                }

                CompassRose {
                    id: rose
                    width: Math.min(parent.width, units.gu(40))
                    height: width
                    anchors.horizontalCenter: parent.horizontalCenter
                    heading: isFinite(root.shownHeading) ? root.shownHeading : 0
                    declination: d.declination !== undefined ? d.declination : 0
                    showMagnetic: root.hasTrue
                    upright: d.axis === "back"
                }

                Column {
                    width: parent.width
                    InfoRow {
                        name: i18n.tr("Magnetic heading")
                        value: d.heading !== undefined ? Fmt.num(d.heading, 0) + "°" : "–"
                    }
                    InfoRow {
                        name: i18n.tr("Declination")
                        value: d.declination !== undefined
                               ? Fmt.signed(d.declination, 1) + "° " + (d.declination >= 0 ? i18n.tr("E") : i18n.tr("W"))
                                 + " (WMM2025" + (d.modelValid === false ? i18n.tr(", expired") : "") + ")"
                               : i18n.tr("unknown – needs a position")
                    }
                    InfoRow {
                        name: i18n.tr("Position for the model")
                        value: isFinite(settings.lastLat)
                               ? Fmt.num(settings.lastLat, 2) + ", " + Fmt.num(settings.lastLon, 2)
                                 + (settings.positionSource === "manual" ? i18n.tr(" (entered)") : settings.positionSource === "cached" ? i18n.tr(" (last known)") : i18n.tr(" (GPS)"))
                               : (settings.useLocation ? i18n.tr("waiting for location (turn Location on)") : i18n.tr("none"))
                    }
                    InfoRow {
                        name: i18n.tr("Field strength")
                        value: d.fieldUT !== undefined
                               ? Fmt.num(d.fieldUT, 0) + " µT" + (d.modelFieldUT !== undefined ? i18n.tr(" (expected %1)").arg(Fmt.num(d.modelFieldUT, 0)) : "")
                               : "–"
                    }
                    InfoRow {
                        name: i18n.tr("Inclination (dip)")
                        value: d.inclination !== undefined
                               ? Fmt.num(d.inclination, 0) + "°" + (d.modelInclination !== undefined ? i18n.tr(" (expected %1°)").arg(Fmt.num(d.modelInclination, 0)) : "")
                               : "–"
                    }
                    InfoRow {
                        name: i18n.tr("Tilt")
                        value: d.pitch !== undefined ? i18n.tr("pitch %1°, roll %2°").arg(Fmt.num(d.pitch, 0)).arg(Fmt.num(d.roll, 0)) : "–"
                    }
                }

                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("Hold the phone flat to use the top edge, or upright to use the camera direction. True north uses the World Magnetic Model at your last known position.")
                }
            }
        }
    }

    Page {
        id: settingsPage
        visible: false
        header: PageHeader {
            id: sh
            title: i18n.tr("Compass settings")
        }
        Flickable {
            anchors { top: sh.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            contentHeight: sc.height
            clip: true
            Column {
                id: sc
                width: parent.width

                ListItem {
                    height: l1.height + divider.height
                    ListItemLayout {
                        id: l1
                        title.text: i18n.tr("Show true north")
                        subtitle.text: i18n.tr("Correct for magnetic declination")
                        Switch {
                            SlotsLayout.position: SlotsLayout.Trailing
                            checked: settings.showTrue
                            onCheckedChanged: settings.showTrue = checked
                        }
                    }
                }
                ListItem {
                    height: l2.height + divider.height
                    ListItemLayout {
                        id: l2
                        title.text: i18n.tr("Use my location")
                        subtitle.text: i18n.tr("Needs Location turned on. Only used for the declination model.")
                        subtitle.maximumLineCount: 3
                        Switch {
                            SlotsLayout.position: SlotsLayout.Trailing
                            checked: settings.useLocation
                            onCheckedChanged: settings.useLocation = checked
                        }
                    }
                }
                ListItem {
                    height: manual.height + units.gu(2)
                    Column {
                        id: manual
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                        spacing: units.gu(1)
                        Label { text: i18n.tr("Position (manual)") }
                        Row {
                            spacing: units.gu(1)
                            TextField {
                                id: latField
                                width: units.gu(14)
                                placeholderText: i18n.tr("latitude")
                                inputMethodHints: Qt.ImhFormattedNumbersOnly
                                text: isFinite(settings.lastLat) ? settings.lastLat.toFixed(4) : ""
                            }
                            TextField {
                                id: lonField
                                width: units.gu(14)
                                placeholderText: i18n.tr("longitude")
                                inputMethodHints: Qt.ImhFormattedNumbersOnly
                                text: isFinite(settings.lastLon) ? settings.lastLon.toFixed(4) : ""
                            }
                            Button {
                                text: i18n.tr("Set")
                                onClicked: {
                                    var la = parseFloat(latField.text.replace(",", "."));
                                    var lo = parseFloat(lonField.text.replace(",", "."));
                                    if (isFinite(la) && isFinite(lo) && Math.abs(la) <= 90 && Math.abs(lo) <= 180) {
                                        settings.lastLat = la;
                                        settings.lastLon = lo;
                                        settings.lastAlt = 0;
                                        settings.positionSource = "manual";
                                        root.pushPosition();
                                    }
                                }
                            }
                        }
                    }
                }
                ListItem {
                    height: damp.height + units.gu(2)
                    Column {
                        id: damp
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                        Label { text: i18n.tr("Damping: %1 s").arg(settings.damping.toFixed(2)) }
                        Slider {
                            width: parent.width
                            minimumValue: 0
                            maximumValue: 2
                            value: settings.damping
                            live: true
                            function formatValue(v) { return v.toFixed(2) }
                            onValueChanged: {
                                settings.damping = value;
                                feed.command("setDamping", String(value));
                            }
                        }
                    }
                }
                ListItem {
                    height: l3.height + divider.height
                    onClicked: PopupUtils.open(resetDialog)
                    ListItemLayout {
                        id: l3
                        title.text: i18n.tr("Forget calibration")
                        title.color: theme.palette.normal.negative
                        subtitle.text: d.calibrated ? i18n.tr("A calibration is stored") : i18n.tr("Not calibrated")
                    }
                }
            }
        }
    }

    Component {
        id: resetDialog
        Dialog {
            id: dlg
            title: i18n.tr("Forget calibration?")
            text: i18n.tr("The stored hard/soft-iron correction will be removed.")
            Button {
                text: i18n.tr("Forget")
                color: theme.palette.normal.negative
                onClicked: { feed.command("calReset"); PopupUtils.close(dlg) }
            }
            Button {
                text: i18n.tr("Cancel")
                onClicked: PopupUtils.close(dlg)
            }
        }
    }
}
