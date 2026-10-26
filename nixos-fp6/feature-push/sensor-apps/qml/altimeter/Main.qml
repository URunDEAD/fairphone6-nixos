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
    anchorToKeyboard: true
    width: units.gu(45)
    height: units.gu(80)

    readonly property var d: feed.data
    readonly property bool feet: settings.altUnit === 1
    readonly property var pUnits: [
        { name: "hPa", k: 1, dec: 1 },
        { name: "inHg", k: 0.0295300, dec: 2 },
        { name: "mmHg", k: 0.750062, dec: 1 }
    ]
    readonly property var pu: pUnits[settings.pressureUnit]

    function alt(m) { return feet ? Fmt.num(m * 3.28084, 0) + " ft" : Fmt.num(m, 0) + " m" }
    function pres(h, extraDec) { return Fmt.num(h * pu.k, pu.dec + (extraDec || 0)) + " " + pu.name }

    SensorFeed {
        id: feed
        kind: "altimeter"
        interval: 250
        active: Qt.application.state === Qt.ApplicationActive
    }

    Settings {
        id: settings
        category: "altimeter"
        property real qnh: 1013.25
        property string qnhSource: "standard"
        property int altUnit: 0
        property int pressureUnit: 0
        property int windowIndex: 1
    }

    readonly property var windows: [600, 3600, 3 * 3600, 12 * 3600, 24 * 3600]
    readonly property var windowNames: [i18n.tr("10 min"), i18n.tr("1 h"), i18n.tr("3 h"), i18n.tr("12 h"), i18n.tr("24 h")]

    Component.onCompleted: {
        feed.command("setQnh", String(settings.qnh));
        feed.command("setWindow", String(windows[settings.windowIndex]));
        stack.push(mainPage);
        if (fp6Preview === "calibrate") stack.push(calPage);
    }

    function setQnh(v, source) {
        var r = feed.command("setQnh", String(v));
        if (r.ok) { settings.qnh = r.qnh; settings.qnhSource = source; }
        return r.ok;
    }
    function setElevation(h, source) {
        var r = feed.command("setElevation", String(h));
        if (r.ok) { settings.qnh = r.qnh; settings.qnhSource = source; }
        return r;
    }

    PageStack { id: stack }

    Page {
        id: mainPage
        visible: false
        header: PageHeader {
            id: header
            title: i18n.tr("Altimeter")
            trailingActionBar.actions: [
                Action {
                    iconName: "settings"
                    text: i18n.tr("Settings")
                    onTriggered: stack.push(settingsPage)
                },
                Action {
                    iconName: "location"
                    text: i18n.tr("Calibrate")
                    onTriggered: stack.push(calPage)
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
                        width: parent.width / 2
                        Label { text: i18n.tr("Altitude"); textSize: Label.Small; color: theme.palette.normal.backgroundSecondaryText }
                        Label {
                            text: d.altitude !== undefined ? root.alt(d.altitude) : "–"
                            font.pixelSize: units.gu(5.5)
                            font.weight: Font.Light
                        }
                    }
                    Column {
                        width: parent.width / 2
                        Label { text: i18n.tr("Pressure"); textSize: Label.Small; color: theme.palette.normal.backgroundSecondaryText }
                        Label {
                            text: d.hpa !== undefined ? root.pres(d.hpa) : "–"
                            font.pixelSize: units.gu(4)
                            font.weight: Font.Light
                        }
                    }
                }
                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: settings.qnhSource === "standard"
                          ? i18n.tr("Not calibrated: standard sea-level pressure 1013.25 hPa. Weather changes this by ±200 m – calibrate for accuracy.")
                          : i18n.tr("Sea-level pressure %1 (from %2)").arg(root.pres(settings.qnh, 1)).arg(
                                settings.qnhSource === "gps" ? i18n.tr("GPS altitude") : settings.qnhSource === "elevation" ? i18n.tr("known elevation") : i18n.tr("entered value"))
                }

                Sections {
                    id: win
                    model: root.windowNames
                    selectedIndex: settings.windowIndex
                    onSelectedIndexChanged: {
                        settings.windowIndex = selectedIndex;
                        feed.command("setWindow", String(root.windows[selectedIndex]));
                    }
                }
                Graph {
                    width: parent.width
                    height: units.gu(22)
                    minSpan: 0.5
                    decimals: settings.pressureUnit === 1 ? 2 : 1
                    unit: root.pu.name
                    leftLabel: "−" + root.windowNames[win.selectedIndex]
                    rightLabel: i18n.tr("now")
                    series: [{ values: (d.graphP || []).map(function (p) { return p * root.pu.k }), color: LomiriColors.blue }]
                }

                Column {
                    width: parent.width
                    InfoRow {
                        name: i18n.tr("Trend (3 h)")
                        value: d.trend3h !== undefined
                               ? Fmt.signed(d.trend3h * root.pu.k, root.pu.dec) + " " + root.pu.name + " · " + d.tendency
                               : i18n.tr("collecting (%1 of 20 min)").arg(Math.floor((d.trendSpan || 0) / 60))
                    }
                    InfoRow { name: i18n.tr("Session min / max"); value: d.sessionMin !== undefined ? root.pres(d.sessionMin) + " / " + root.pres(d.sessionMax) : "–" }
                    InfoRow { name: i18n.tr("Pressure altitude (1013.25)"); value: d.stdAltitude !== undefined ? root.alt(d.stdAltitude) : "–" }
                    InfoRow { name: i18n.tr("Sensor temperature"); value: d.temperature !== undefined ? Fmt.num(d.temperature, 1) + " °C" : "–" }
                }
                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("History is kept only while the app is open (and from the background logger if you turned it on). The temperature is the sensor's own, inside the phone.")
                }
            }
        }
    }

    // ---- calibration ----------------------------------------------------------
    Page {
        id: calPage
        visible: false
        header: PageHeader { id: calHeader; title: i18n.tr("Calibrate altitude") }

        PositionSource {
            id: gps
            active: calPage.visible && Qt.application.state === Qt.ApplicationActive
            updateInterval: 2000
            preferredPositioningMethods: PositionSource.SatellitePositioningMethods
            // QtPositioning replays its cached last position first: only
            // fixes from this session are used.
            property real since: 0
            onActiveChanged: if (active) since = Date.now()
            readonly property bool fresh: position.altitudeValid && position.timestamp.getTime() >= since - 2000
        }

        Flickable {
            anchors { top: calHeader.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            contentHeight: cc.height + units.gu(4)
            clip: true
            Column {
                id: cc
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                spacing: units.gu(1.5)
                property string msg: ""

                Label { text: i18n.tr("Known elevation here"); font.weight: Font.DemiBold }
                Row {
                    spacing: units.gu(1)
                    TextField {
                        id: elevField
                        width: units.gu(16)
                        placeholderText: root.feet ? i18n.tr("feet") : i18n.tr("metres")
                        inputMethodHints: Qt.ImhFormattedNumbersOnly
                    }
                    Button {
                        text: i18n.tr("Set")
                        color: theme.palette.normal.positive
                        onClicked: {
                            var h = parseFloat(elevField.text.replace(",", "."));
                            if (!isFinite(h)) return;
                            if (root.feet) h = h / 3.28084;
                            var r = root.setElevation(h, "elevation");
                            cc.msg = r.ok ? i18n.tr("Calibrated: sea-level pressure %1").arg(root.pres(r.qnh, 1)) : (r.reason || "");
                        }
                    }
                }

                Label { text: i18n.tr("Sea-level pressure (QNH) from a weather report"); font.weight: Font.DemiBold }
                Row {
                    spacing: units.gu(1)
                    TextField {
                        id: qnhField
                        width: units.gu(16)
                        placeholderText: "hPa"
                        inputMethodHints: Qt.ImhFormattedNumbersOnly
                    }
                    Button {
                        text: i18n.tr("Set")
                        color: theme.palette.normal.positive
                        onClicked: {
                            var v = parseFloat(qnhField.text.replace(",", "."));
                            cc.msg = root.setQnh(v, "manual") ? i18n.tr("Sea-level pressure set to %1 hPa").arg(v.toFixed(1)) : i18n.tr("Enter a value between 800 and 1100 hPa");
                        }
                    }
                }

                Label { text: i18n.tr("GPS altitude"); font.weight: Font.DemiBold }
                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    text: gps.fresh
                          ? i18n.tr("GPS: %1 (vertical accuracy %2)").arg(root.alt(gps.position.coordinate.altitude)).arg(gps.position.verticalAccuracyValid ? "±" + root.alt(gps.position.verticalAccuracy) : i18n.tr("unknown"))
                          : i18n.tr("Waiting for a GPS fix with altitude. Location must be turned on; outdoors it can take a minute.")
                }
                Button {
                    text: i18n.tr("Use GPS altitude")
                    enabled: gps.fresh
                    onClicked: {
                        var r = root.setElevation(gps.position.coordinate.altitude, "gps");
                        cc.msg = r.ok ? i18n.tr("Calibrated from GPS: sea-level pressure %1").arg(root.pres(r.qnh, 1)) : (r.reason || "");
                    }
                }

                Button {
                    text: i18n.tr("Use standard 1013.25 hPa")
                    onClicked: { root.setQnh(1013.25, "standard"); cc.msg = i18n.tr("Standard atmosphere") }
                }
                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    visible: cc.msg !== ""
                    text: cc.msg
                    color: theme.palette.normal.positive
                }
            }
        }
    }

    // ---- settings ---------------------------------------------------------------
    Page {
        id: settingsPage
        visible: false
        header: PageHeader { id: sh; title: i18n.tr("Altimeter settings") }
        Flickable {
            anchors { top: sh.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            contentHeight: sc.height
            clip: true
            Column {
                id: sc
                width: parent.width
                OptionSelector {
                    width: parent.width - units.gu(4)
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: i18n.tr("Altitude unit")
                    model: [i18n.tr("metres"), i18n.tr("feet")]
                    selectedIndex: settings.altUnit
                    onSelectedIndexChanged: settings.altUnit = selectedIndex
                }
                Item { width: 1; height: units.gu(2) }
                OptionSelector {
                    width: parent.width - units.gu(4)
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: i18n.tr("Pressure unit")
                    model: ["hPa", "inHg", "mmHg"]
                    selectedIndex: settings.pressureUnit
                    onSelectedIndexChanged: settings.pressureUnit = selectedIndex
                }
                Item { width: 1; height: units.gu(2) }
                ListItem {
                    height: ll.height + divider.height
                    ListItemLayout {
                        id: ll
                        title.text: i18n.tr("Background pressure log")
                        subtitle.text: i18n.tr("Off by default. When on, one pressure reading every 15 minutes is stored on the phone (7 days kept) for the trend graph.")
                        subtitle.maximumLineCount: 4
                        Switch {
                            id: logSwitch
                            SlotsLayout.position: SlotsLayout.Trailing
                            enabled: logState.available === true
                            property var logState: feed.command("loggerState")
                            Component.onCompleted: checked = logState.enabled === true
                            onClicked: {
                                var r = feed.command("setLogger", checked ? "on" : "off");
                                checked = r.enabled === true;
                            }
                        }
                    }
                }
                ListItem {
                    height: l2.height + divider.height
                    onClicked: feed.command("clearHistory")
                    ListItemLayout {
                        id: l2
                        title.text: i18n.tr("Clear the in-app history")
                    }
                }
            }
        }
    }
}
