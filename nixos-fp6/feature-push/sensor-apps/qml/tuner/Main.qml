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
    readonly property bool voiced: typeof d.freq === "number" && isFinite(d.freq)
    readonly property var instruments: ["chromatic", "guitar", "bass", "ukulele", "violin", "viola", "cello", "mandolin"]
    readonly property var instrumentNames: [i18n.tr("Chromatic"), i18n.tr("Guitar (E A D G B E)"), i18n.tr("Bass (E A D G)"), i18n.tr("Ukulele (G C E A)"),
                                            i18n.tr("Violin (G D A E)"), i18n.tr("Viola (C G D A)"), i18n.tr("Cello (C G D A)"), i18n.tr("Mandolin (G D A E)")]

    SensorFeed {
        id: feed
        kind: "tuner"
        interval: 50
        // the microphone is only open while the app is in the foreground
        active: Qt.application.state === Qt.ApplicationActive
    }

    Settings {
        id: settings
        category: "tuner"
        property real a4: 440
        property int instrument: 0
    }

    Component.onCompleted: {
        feed.command("setA4", String(settings.a4));
        feed.command("setInstrument", instruments[settings.instrument]);
        stack.push(mainPage);
        if (fp6Preview === "settings") stack.push(settingsPage);
    }

    PageStack { id: stack }

    Page {
        id: mainPage
        visible: false
        header: PageHeader {
            id: header
            title: i18n.tr("Tuner")
            trailingActionBar.actions: [
                Action {
                    iconName: "settings"
                    text: i18n.tr("Settings")
                    onTriggered: stack.push(settingsPage)
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
                    iconName: "audio-input-microphone-muted-symbolic"
                    accent: theme.palette.normal.negative
                    text: d.error || ""
                }

                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: root.instrumentNames[settings.instrument]
                    color: theme.palette.normal.backgroundSecondaryText
                }

                // note name with octave
                Item {
                    width: parent.width
                    height: units.gu(16)
                    Row {
                        anchors.centerIn: parent
                        Label {
                            id: noteLabel
                            text: root.voiced ? d.note : "–"
                            font.pixelSize: units.gu(15)
                            font.weight: Font.Light
                            color: !root.voiced ? theme.palette.normal.backgroundSecondaryText
                                   : meter.inTune ? theme.palette.normal.positive : theme.palette.normal.backgroundText
                        }
                        Label {
                            anchors.baseline: noteLabel.baseline
                            visible: root.voiced
                            text: d.octave !== undefined ? d.octave : ""
                            font.pixelSize: units.gu(5)
                            color: theme.palette.normal.backgroundSecondaryText
                        }
                    }
                }

                CentsMeter {
                    id: meter
                    width: parent.width
                    cents: root.voiced ? d.cents : NaN
                }

                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    textSize: Label.Large
                    text: !root.voiced ? i18n.tr("Play a note")
                          : meter.inTune ? i18n.tr("In tune")
                          : d.cents < 0 ? i18n.tr("%1 cents flat – tune up").arg(Fmt.num(-d.cents, 0))
                          : i18n.tr("%1 cents sharp – tune down").arg(Fmt.num(d.cents, 0))
                }

                // strings of the selected instrument
                Row {
                    visible: (d.strings || []).length > 0
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: units.gu(1)
                    Repeater {
                        model: d.strings || []
                        Rectangle {
                            readonly property bool current: root.voiced && d.string === index
                            width: units.gu(5.5); height: width; radius: width / 2
                            color: current ? (meter.inTune ? theme.palette.normal.positive : theme.palette.normal.activity) : theme.palette.normal.foreground
                            border.width: current ? 0 : units.dp(1)
                            border.color: theme.palette.normal.base
                            Label {
                                anchors.centerIn: parent
                                text: modelData
                                textSize: Label.Small
                                color: parent.current ? "white" : theme.palette.normal.backgroundText
                            }
                        }
                    }
                }

                Row {
                    width: parent.width
                    Reading { width: parent.width / 3; caption: i18n.tr("Frequency"); value: root.voiced ? Fmt.num(d.freq, 1) + " Hz" : "–" }
                    Reading { width: parent.width / 3; caption: i18n.tr("Target"); value: root.voiced ? Fmt.num(d.target, 1) + " Hz" : "–" }
                    Reading { width: parent.width / 3; caption: i18n.tr("Reference"); value: "A4 = " + Fmt.num(settings.a4, 0) }
                }

                // input level
                Item {
                    width: parent.width
                    height: units.gu(1)
                    Rectangle { anchors.fill: parent; radius: height / 2; color: theme.palette.normal.foreground }
                    Rectangle {
                        height: parent.height; radius: height / 2
                        width: parent.width * Math.max(0, Math.min(1, ((d.level || -90) + 70) / 70))
                        color: (d.level || -90) > -60 ? theme.palette.normal.activity : theme.palette.normal.base
                    }
                }
                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("Play one string at a time close to the phone. Audio is analysed in memory only; nothing is recorded or saved. The microphone is used only while this app is open in front.")
                }
            }
        }
    }

    Page {
        id: settingsPage
        visible: false
        header: PageHeader { id: sh; title: i18n.tr("Tuner settings") }
        Flickable {
            anchors { top: sh.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            contentHeight: sc.height + units.gu(2)
            clip: true
            Column {
                id: sc
                width: parent.width
                spacing: units.gu(1)
                Item { width: 1; height: units.gu(1) }
                OptionSelector {
                    width: parent.width - units.gu(4)
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: i18n.tr("Instrument")
                    model: root.instrumentNames
                    containerHeight: itemHeight * 5
                    selectedIndex: settings.instrument
                    onSelectedIndexChanged: { settings.instrument = selectedIndex; feed.command("setInstrument", root.instruments[selectedIndex]) }
                }
                ListItem {
                    height: ac.height + units.gu(3)
                    Column {
                        id: ac
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                        spacing: units.gu(1)
                        Label { text: i18n.tr("Reference pitch: A4 = %1 Hz").arg(settings.a4.toFixed(0)) }
                        Slider {
                            width: parent.width
                            minimumValue: 415
                            maximumValue: 466
                            value: settings.a4
                            live: true
                            function formatValue(v) { return v.toFixed(0) }
                            onValueChanged: { settings.a4 = Math.round(value); feed.command("setA4", String(settings.a4)) }
                        }
                        Row {
                            spacing: units.gu(1)
                            Button { text: "−1"; onClicked: { settings.a4 = Math.max(415, settings.a4 - 1); feed.command("setA4", String(settings.a4)) } }
                            Button { text: "+1"; onClicked: { settings.a4 = Math.min(466, settings.a4 + 1); feed.command("setA4", String(settings.a4)) } }
                            Button { text: "440"; onClicked: { settings.a4 = 440; feed.command("setA4", "440") } }
                        }
                    }
                }
            }
        }
    }
}
