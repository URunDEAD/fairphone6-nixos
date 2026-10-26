import QtQuick 2.12
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
    readonly property string unit: settings.aWeighted ? "dB(A)" : "dB"

    SensorFeed {
        id: feed
        kind: "sound"
        interval: 100
        // the microphone is only open while the app is in the foreground
        active: Qt.application.state === Qt.ApplicationActive
    }

    Settings {
        id: settings
        category: "sound"
        property real offset: 110
        property bool aWeighted: true
        property bool fast: true
    }

    Component.onCompleted: {
        feed.command("setOffset", String(settings.offset));
        feed.command("setWeighting", settings.aWeighted ? "A" : "Z");
        feed.command("setResponse", settings.fast ? "fast" : "slow");
        stack.push(mainPage);
        if (fp6Preview === "settings") stack.push(settingsPage);
    }

    // statistics of digital silence (no input) are meaningless
    function stat(v) { return typeof v === "number" && v - settings.offset > -110 ? Fmt.num(v, 0) : "–" }

    function levelColor(v) {
        if (!isFinite(v)) return theme.palette.normal.backgroundText;
        if (v >= 85) return theme.palette.normal.negative;
        if (v >= 70) return LomiriColors.orange;
        return theme.palette.normal.positive;
    }

    PageStack { id: stack }

    Page {
        id: mainPage
        visible: false
        header: PageHeader {
            id: header
            title: i18n.tr("Sound meter")
            trailingActionBar.actions: [
                Action {
                    iconName: "settings"
                    text: i18n.tr("Settings")
                    onTriggered: stack.push(settingsPage)
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
                    iconName: "audio-input-microphone-muted-symbolic"
                    accent: theme.palette.normal.negative
                    text: d.error || ""
                }

                Column {
                    anchors.horizontalCenter: parent.horizontalCenter
                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: typeof d.dbfs === "number" && d.dbfs < -110 ? i18n.tr("silent") : Fmt.num(d.level, 0)
                        font.pixelSize: units.gu(12)
                        font.weight: Font.Light
                        color: root.levelColor(d.level)
                    }
                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: root.unit + " · " + (settings.fast ? i18n.tr("fast") : i18n.tr("slow")) + " · " + i18n.tr("uncalibrated estimate")
                        textSize: Label.Small
                        color: theme.palette.normal.backgroundSecondaryText
                    }
                }

                // bar meter 30..110
                Item {
                    width: parent.width
                    height: units.gu(3)
                    Rectangle {
                        anchors.fill: parent
                        radius: height / 2
                        color: theme.palette.normal.foreground
                    }
                    Rectangle {
                        height: parent.height
                        radius: height / 2
                        width: Math.max(height, parent.width * Math.max(0, Math.min(1, ((d.level || 30) - 30) / 80)))
                        color: root.levelColor(d.level)
                        Behavior on width { NumberAnimation { duration: 90 } }
                    }
                    Rectangle {
                        visible: isFinite(d.max)
                        width: units.dp(3); height: parent.height
                        x: parent.width * Math.max(0, Math.min(1, ((d.max || 30) - 30) / 80)) - width
                        color: theme.palette.normal.backgroundText
                    }
                }

                Row {
                    width: parent.width
                    Reading { width: parent.width / 3; caption: i18n.tr("Min"); value: root.stat(d.min) }
                    Reading { width: parent.width / 3; caption: i18n.tr("Average (Leq)"); value: root.stat(d.leq) }
                    Reading { width: parent.width / 3; caption: i18n.tr("Max"); value: root.stat(d.max) }
                }

                Graph {
                    width: parent.width
                    height: units.gu(20)
                    fixedMin: 30
                    fixedMax: 110
                    decimals: 0
                    unit: root.unit
                    leftLabel: i18n.tr("−30 s")
                    rightLabel: i18n.tr("now")
                    series: [{ values: d.history || [], color: LomiriColors.purple }]
                }

                Column {
                    width: parent.width
                    InfoRow { name: i18n.tr("Raw level"); value: Fmt.num(d.dbfs, 1) + " dBFS" }
                    InfoRow { name: i18n.tr("Peak"); value: Fmt.num(d.peakDbfs, 1) + " dBFS" + (typeof d.peakDbfs === "number" && d.peakDbfs > -1 ? i18n.tr(" (clipping)") : "") }
                    InfoRow { name: i18n.tr("Calibration offset"); value: Fmt.signed(settings.offset, 1) + " dB" }
                }

                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("The phone's microphone is not a calibrated instrument: the value is the A-weighted microphone level plus an offset you can set against a real meter. Audio is analysed in memory only; nothing is recorded or saved. The microphone is used only while this app is open in front.")
                }

                Column {
                    width: parent.width
                    Label { text: i18n.tr("Typical levels"); font.weight: Font.DemiBold }
                    Repeater {
                        model: [["30", i18n.tr("quiet bedroom at night")], ["45", i18n.tr("quiet office, library")], ["60", i18n.tr("normal conversation")],
                                ["70", i18n.tr("busy street, vacuum cleaner")], ["85", i18n.tr("heavy traffic – hearing risk with long exposure")], ["100", i18n.tr("concert, power tools")]]
                        InfoRow { name: modelData[0] + " dB(A)"; value: modelData[1] }
                    }
                }
            }
        }
    }

    Page {
        id: settingsPage
        visible: false
        header: PageHeader { id: sh; title: i18n.tr("Sound meter settings") }
        Flickable {
            anchors { top: sh.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            contentHeight: sc.height + units.gu(2)
            clip: true
            Column {
                id: sc
                width: parent.width
                ListItem {
                    height: l1.height + divider.height
                    ListItemLayout {
                        id: l1
                        title.text: i18n.tr("A-weighting")
                        subtitle.text: i18n.tr("Off: flat (Z) frequency weighting")
                        Switch {
                            SlotsLayout.position: SlotsLayout.Trailing
                            checked: settings.aWeighted
                            onCheckedChanged: { settings.aWeighted = checked; feed.command("setWeighting", checked ? "A" : "Z") }
                        }
                    }
                }
                ListItem {
                    height: l2.height + divider.height
                    ListItemLayout {
                        id: l2
                        title.text: i18n.tr("Fast response (125 ms)")
                        subtitle.text: i18n.tr("Off: slow (1 s)")
                        Switch {
                            SlotsLayout.position: SlotsLayout.Trailing
                            checked: settings.fast
                            onCheckedChanged: { settings.fast = checked; feed.command("setResponse", checked ? "fast" : "slow") }
                        }
                    }
                }
                ListItem {
                    height: oc.height + units.gu(3)
                    Column {
                        id: oc
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                        spacing: units.gu(1)
                        Label { text: i18n.tr("Calibration offset: %1 dB").arg(settings.offset.toFixed(1)) }
                        Label {
                            width: parent.width
                            wrapMode: Text.Wrap
                            textSize: Label.Small
                            color: theme.palette.normal.backgroundSecondaryText
                            text: i18n.tr("Shown level = microphone dBFS + offset. Compare with a sound level meter and adjust. Default 110 dB (a rough value for this phone: a quiet room reads about −85 dBFS A-weighted).")
                        }
                        Slider {
                            width: parent.width
                            minimumValue: 60
                            maximumValue: 130
                            value: settings.offset
                            live: true
                            function formatValue(v) { return v.toFixed(1) }
                            onValueChanged: { settings.offset = Math.round(value * 2) / 2; feed.command("setOffset", String(settings.offset)) }
                        }
                        Row {
                            spacing: units.gu(1)
                            Button { text: "−0.5"; onClicked: { settings.offset -= 0.5; feed.command("setOffset", String(settings.offset)) } }
                            Button { text: "+0.5"; onClicked: { settings.offset += 0.5; feed.command("setOffset", String(settings.offset)) } }
                            Button { text: i18n.tr("Default"); onClicked: { settings.offset = 110; feed.command("setOffset", "110") } }
                        }
                    }
                }
            }
        }
    }
}
