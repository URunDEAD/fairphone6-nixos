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

    property var ts: ({})
    readonly property var d: ts
    readonly property bool playing: d.playing === true
    readonly property var waves: ["sine", "square", "triangle", "saw"]

    SensorFeed {
        id: feed
        kind: "tone"
        interval: 200
        // sound only while the app is in front
        active: Qt.application.state === Qt.ApplicationActive
        onUpdated: root.ts = data
    }

    Settings {
        id: settings
        category: "tone"
        property real freq: 440
        property real volume: 30
        property int wave: 0
    }

    Component.onCompleted: {
        feed.command("setFreq", String(settings.freq));
        feed.command("setWave", waves[settings.wave]);
        root.ts = feed.command("setVolume", String(settings.volume));
    }

    // slider position 0..1 <-> 20 Hz .. 20 kHz (logarithmic)
    function freqOf(p) { return 20 * Math.pow(1000, p) }
    function posOf(f) { return Math.log(f / 20) / Math.log(1000) }
    function setFreq(f) {
        f = Math.max(20, Math.min(20000, f));
        settings.freq = f < 100 ? Math.round(f * 10) / 10 : Math.round(f);
        root.ts = feed.command("setFreq", String(settings.freq));
    }
    function fmtFreq(f) { return f >= 1000 ? Fmt.num(f / 1000, f >= 10000 ? 2 : 3) + " kHz" : Fmt.num(f, f < 100 ? 1 : 0) + " Hz" }

    Page {
        anchors.fill: parent
        header: PageHeader {
            id: header
            title: i18n.tr("Tone generator")
            extension: Sections {
                anchors { left: parent.left; bottom: parent.bottom }
                model: [i18n.tr("Sine"), i18n.tr("Square"), i18n.tr("Triangle"), i18n.tr("Sawtooth")]
                selectedIndex: settings.wave
                onSelectedIndexChanged: { settings.wave = selectedIndex; root.ts = feed.command("setWave", root.waves[selectedIndex]) }
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
                    iconName: "audio-volume-muted"
                    accent: theme.palette.normal.negative
                    text: d.error || ""
                }

                Column {
                    anchors.horizontalCenter: parent.horizontalCenter
                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: root.fmtFreq(settings.freq)
                        font.pixelSize: units.gu(9)
                        font.weight: Font.Light
                        color: root.playing ? theme.palette.normal.activity : theme.palette.normal.backgroundText
                    }
                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: !d.note ? "" : Math.abs(d.cents) < 0.5 ? i18n.tr("= %1").arg(d.note)
                              : i18n.tr("≈ %1 %2 cents").arg(d.note).arg(Fmt.signed(d.cents, 0))
                        color: theme.palette.normal.backgroundSecondaryText
                    }
                }

                // waveform preview
                Canvas {
                    id: wave
                    width: parent.width
                    height: units.gu(7)
                    property int kind: settings.wave
                    onKindChanged: requestPaint()
                    onWidthChanged: requestPaint()
                    onPaint: {
                        var ctx = getContext("2d");
                        ctx.reset();
                        ctx.strokeStyle = theme.palette.normal.activity;
                        ctx.lineWidth = units.dp(2);
                        ctx.beginPath();
                        var w = width, h = height, n = 200;
                        for (var i = 0; i <= n; i++) {
                            var t = (i / n * 2) % 1, v;
                            if (kind === 0) v = Math.sin(2 * Math.PI * t);
                            else if (kind === 1) v = t < 0.5 ? 1 : -1;
                            else if (kind === 2) v = 1 - 4 * Math.abs(t - 0.5);
                            else v = 2 * t - 1;
                            var x = i / n * w, y = h / 2 - v * (h / 2 - units.dp(2));
                            if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
                        }
                        ctx.stroke();
                    }
                }

                Slider {
                    width: parent.width
                    minimumValue: 0
                    maximumValue: 1
                    value: root.posOf(settings.freq)
                    live: true
                    function formatValue(v) { return root.fmtFreq(root.freqOf(v)) }
                    onValueChanged: if (pressed) root.setFreq(root.freqOf(value))
                }

                // fine steps
                Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: units.gu(1)
                    Button { width: units.gu(8); text: "÷2"; onClicked: root.setFreq(settings.freq / 2) }
                    Button { width: units.gu(8); text: "−1 st"; onClicked: root.setFreq(settings.freq / Math.pow(2, 1 / 12)) }
                    Button { width: units.gu(8); text: "+1 st"; onClicked: root.setFreq(settings.freq * Math.pow(2, 1 / 12)) }
                    Button { width: units.gu(8); text: "×2"; onClicked: root.setFreq(settings.freq * 2) }
                }
                Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: units.gu(1)
                    Repeater {
                        model: [[50, "50 Hz"], [440, "A4"], [1000, "1 kHz"], [10000, "10 kHz"]]
                        Button {
                            width: units.gu(8)
                            text: modelData[1]
                            onClicked: root.setFreq(modelData[0])
                        }
                    }
                }

                Column {
                    width: parent.width
                    spacing: units.gu(0.5)
                    Label { text: i18n.tr("Volume: %1 %").arg(settings.volume.toFixed(0)) }
                    Row {
                        width: parent.width
                        spacing: units.gu(1)
                        Icon { name: "audio-volume-low"; width: units.gu(2.5); height: width; anchors.verticalCenter: parent.verticalCenter }
                        Slider {
                            width: parent.width - units.gu(7)
                            minimumValue: 0
                            maximumValue: 100
                            value: settings.volume
                            live: true
                            function formatValue(v) { return v.toFixed(0) + " %" }
                            onValueChanged: { settings.volume = Math.round(value); root.ts = feed.command("setVolume", String(settings.volume)) }
                        }
                        Icon { name: "audio-volume-high"; width: units.gu(2.5); height: width; anchors.verticalCenter: parent.verticalCenter }
                    }
                }

                Button {
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: units.gu(24)
                    height: units.gu(6)
                    iconName: root.playing ? "media-playback-stop" : "media-playback-start"
                    text: root.playing ? i18n.tr("Stop") : i18n.tr("Play")
                    color: root.playing ? theme.palette.normal.negative : theme.palette.normal.positive
                    onClicked: root.ts = feed.command("play", root.playing ? "false" : "true")
                }

                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("Start quietly: high or loud tones can damage hearing and speakers, especially with headphones. The phone's speaker reproduces little below about 300 Hz. The tone stops when you leave the app.")
                }
            }
        }
    }
}
