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
    readonly property bool useG: settings.unit === 1
    readonly property real k: useG ? 1 / 9.80665 : 1
    readonly property string unit: useG ? "g" : "m/s²"
    readonly property int dec: useG ? 4 : 3

    SensorFeed {
        id: feed
        kind: "vibration"
        interval: 100
        active: Qt.application.state === Qt.ApplicationActive
    }

    Settings {
        id: settings
        category: "vibration"
        property int unit: 0
    }

    // rough perception scale (whole-body vibration, RMS m/s²)
    function feel(rms) {
        if (!isFinite(rms)) return "";
        if (rms < 0.02) return i18n.tr("still (sensor noise)");
        if (rms < 0.1) return i18n.tr("barely perceptible");
        if (rms < 0.5) return i18n.tr("clearly perceptible");
        if (rms < 2) return i18n.tr("strong");
        return i18n.tr("very strong");
    }

    Page {
        anchors.fill: parent
        header: PageHeader {
            id: header
            title: i18n.tr("Vibration meter")
            trailingActionBar.actions: [
                Action {
                    iconName: "reset"
                    text: i18n.tr("Reset peak")
                    onTriggered: feed.command("reset")
                }
            ]
            extension: Sections {
                anchors { left: parent.left; bottom: parent.bottom }
                model: ["m/s²", "g"]
                selectedIndex: settings.unit
                onSelectedIndexChanged: settings.unit = selectedIndex
            }
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

                Column {
                    anchors.horizontalCenter: parent.horizontalCenter
                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: Fmt.num(d.rms * root.k, root.dec - 1)
                        font.pixelSize: units.gu(11)
                        font.weight: Font.Light
                        color: theme.palette.normal.activity
                    }
                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: root.unit + " RMS · " + root.feel(d.rms)
                        color: theme.palette.normal.backgroundSecondaryText
                    }
                }

                Graph {
                    width: parent.width
                    height: units.gu(14)
                    fixedMin: 0
                    minSpan: 0.05 * root.k
                    decimals: root.dec - 1
                    unit: root.unit
                    leftLabel: i18n.tr("−5 s")
                    rightLabel: i18n.tr("now")
                    series: [{ values: (d.trace || []).map(function (v) { return v * root.k }), color: theme.palette.normal.activity }]
                }

                Row {
                    width: parent.width
                    Reading { width: parent.width / 3; caption: i18n.tr("Peak (2 s)"); value: Fmt.num(d.peak * root.k, root.dec - 1) }
                    Reading { width: parent.width / 3; caption: i18n.tr("Session peak"); value: Fmt.num(d.sessionPeak * root.k, root.dec - 1) }
                    Reading { width: parent.width / 3; caption: i18n.tr("Frequency"); value: isFinite(d.freq) ? Fmt.num(d.freq, 1) + " Hz" : "–" }
                }

                Graph {
                    width: parent.width
                    height: units.gu(14)
                    fixedMin: 0
                    minSpan: 0.05 * root.k
                    decimals: root.dec - 1
                    unit: root.unit + " RMS"
                    leftLabel: i18n.tr("−60 s")
                    rightLabel: i18n.tr("now")
                    series: [{ values: (d.history || []).map(function (v) { return v * root.k }), color: LomiriColors.orange }]
                }

                Column {
                    width: parent.width
                    InfoRow { name: i18n.tr("Strongest axis"); value: d.axis ? d.axis.toUpperCase() + (d.axis === "z" ? i18n.tr(" (through the screen)") : d.axis === "y" ? i18n.tr(" (along the long edge)") : i18n.tr(" (along the short edge)")) : "–" }
                    InfoRow { name: i18n.tr("Sample rate"); value: Fmt.num(d.rate, 0) + " Hz" + (isFinite(d.odr) ? i18n.tr(" (sensor %1 Hz)").arg(Fmt.num(d.odr, 0)) : "") }
                    InfoRow { name: i18n.tr("Highest measurable frequency"); value: isFinite(d.rate) && d.rate > 0 ? Fmt.num(d.rate / 2, 0) + " Hz" : "–" }
                }

                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("Lay the phone flat on the vibrating surface. Gravity and slow movement (below about 0.5 Hz) are filtered out. The accelerometer is sampled at its own rate, so faster vibrations (above half the sample rate) cannot be measured and may show up at a wrong, lower frequency. A still phone reads about 0.01 m/s² of sensor noise.")
                }
            }
        }
    }
}
