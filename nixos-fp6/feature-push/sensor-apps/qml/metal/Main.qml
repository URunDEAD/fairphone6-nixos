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
    readonly property bool alarm: d.alarm === true
    readonly property color accent: alarm ? theme.palette.normal.negative : theme.palette.normal.activity

    SensorFeed {
        id: feed
        kind: "metal"
        interval: 50
        active: Qt.application.state === Qt.ApplicationActive
    }

    Settings {
        id: settings
        category: "metal"
        property real threshold: 20
        property bool beep: true
        property real volume: 40
    }

    Component.onCompleted: {
        feed.command("setThreshold", String(settings.threshold));
        feed.command("setVolume", String(settings.volume));
        feed.command("setBeep", settings.beep ? "true" : "false");
        stack.push(mainPage);
        if (fp6Preview === "settings") stack.push(settingsPage);
    }

    PageStack { id: stack }

    Page {
        id: mainPage
        visible: false
        header: PageHeader {
            id: header
            title: i18n.tr("Metal detector")
            trailingActionBar.actions: [
                Action {
                    iconName: "settings"
                    text: i18n.tr("Settings")
                    onTriggered: stack.push(settingsPage)
                },
                Action {
                    iconName: settings.beep ? "audio-volume-high" : "audio-volume-muted"
                    text: settings.beep ? i18n.tr("Beep off") : i18n.tr("Beep on")
                    onTriggered: { settings.beep = !settings.beep; feed.command("setBeep", settings.beep ? "true" : "false") }
                },
                Action {
                    iconName: "reset"
                    text: i18n.tr("Zero")
                    onTriggered: feed.command("zero")
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

                Column {
                    anchors.horizontalCenter: parent.horizontalCenter
                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: isFinite(d.baseline) ? Fmt.signed(d.delta, 0) : "…"
                        font.pixelSize: units.gu(12)
                        font.weight: Font.Light
                        color: root.accent
                    }
                    Label {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: isFinite(d.baseline) ? i18n.tr("µT change from zero") : i18n.tr("zeroing – hold the phone away from metal")
                        color: theme.palette.normal.backgroundSecondaryText
                    }
                }

                // strength bar: 0 .. 4 x threshold, threshold mark
                Item {
                    width: parent.width
                    height: units.gu(3)
                    readonly property real span: 4 * settings.threshold
                    Rectangle { anchors.fill: parent; radius: height / 2; color: theme.palette.normal.foreground }
                    Rectangle {
                        height: parent.height
                        radius: height / 2
                        width: Math.max(height, parent.width * Math.min(1, Math.abs(d.delta || 0) / parent.span))
                        color: root.accent
                        Behavior on width { NumberAnimation { duration: 60 } }
                    }
                    Rectangle {
                        x: parent.width / 4 - width / 2
                        width: units.dp(3); height: parent.height
                        color: theme.palette.normal.backgroundText
                    }
                }
                Label {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    textSize: Label.Small
                    color: root.alarm ? theme.palette.normal.negative : theme.palette.normal.backgroundSecondaryText
                    text: root.alarm ? i18n.tr("Metal or magnet nearby") : i18n.tr("Alarm at ±%1 µT").arg(Fmt.num(settings.threshold, 0))
                }

                Graph {
                    width: parent.width
                    height: units.gu(16)
                    minSpan: 2 * settings.threshold
                    decimals: 0
                    unit: "µT"
                    leftLabel: i18n.tr("−30 s")
                    rightLabel: i18n.tr("now")
                    series: [{ values: d.history || [], color: root.accent }]
                }

                Row {
                    width: parent.width
                    Reading { width: parent.width / 3; caption: i18n.tr("Field"); value: Fmt.num(d.field, 1) + " µT" }
                    Reading { width: parent.width / 3; caption: i18n.tr("Zero at"); value: Fmt.num(d.baseline, 1) + " µT" }
                    Reading { width: parent.width / 3; caption: i18n.tr("Largest change"); value: Fmt.num(d.maxDelta, 0) + " µT" }
                }

                Column {
                    width: parent.width
                    InfoRow { name: "X"; value: Fmt.num(d.x, 1) + " µT" }
                    InfoRow { name: "Y"; value: Fmt.num(d.y, 1) + " µT" }
                    InfoRow { name: "Z"; value: Fmt.num(d.z, 1) + " µT" }
                }

                Notice {
                    visible: d.calibrated === false
                    iconName: "info"
                    text: i18n.tr("The magnetometer is not calibrated, so the field value includes the phone's own magnetism. Changes are detected either way; calibrate in the Compass app for real field strength (Earth: 25–65 µT).")
                }

                Notice {
                    visible: d.audioError !== undefined
                    iconName: "audio-volume-muted"
                    accent: theme.palette.normal.negative
                    text: i18n.tr("Beep unavailable: %1").arg(d.audioError || "")
                }

                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("Tap Zero away from metal, then move the phone slowly over the surface. Iron and steel (screws, nails, studs, pipes), magnets and wires carrying current change the magnetic field. Aluminium, copper, brass, gold and silver are not detected.")
                }
            }
        }
    }

    Page {
        id: settingsPage
        visible: false
        header: PageHeader { id: sh; title: i18n.tr("Metal detector settings") }
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
                        title.text: i18n.tr("Beep")
                        subtitle.text: i18n.tr("Beeps faster the stronger the change")
                        Switch {
                            SlotsLayout.position: SlotsLayout.Trailing
                            checked: settings.beep
                            onCheckedChanged: { settings.beep = checked; feed.command("setBeep", checked ? "true" : "false") }
                        }
                    }
                }
                ListItem {
                    height: tc.height + units.gu(3)
                    Column {
                        id: tc
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                        spacing: units.gu(1)
                        Label { text: i18n.tr("Alarm threshold: %1 µT").arg(settings.threshold.toFixed(0)) }
                        Slider {
                            width: parent.width
                            minimumValue: 5
                            maximumValue: 200
                            value: settings.threshold
                            live: true
                            function formatValue(v) { return v.toFixed(0) }
                            onValueChanged: { settings.threshold = Math.round(value); feed.command("setThreshold", String(settings.threshold)) }
                        }
                        Label {
                            width: parent.width
                            wrapMode: Text.Wrap
                            textSize: Label.Small
                            color: theme.palette.normal.backgroundSecondaryText
                            text: i18n.tr("Lower finds smaller objects but also reacts to turning the phone (the Earth's field changes direction). 20 µT suits screws in a wall.")
                        }
                    }
                }
                ListItem {
                    height: vc.height + units.gu(3)
                    Column {
                        id: vc
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                        spacing: units.gu(1)
                        Label { text: i18n.tr("Beep volume: %1 %").arg(settings.volume.toFixed(0)) }
                        Slider {
                            width: parent.width
                            minimumValue: 5
                            maximumValue: 100
                            value: settings.volume
                            live: true
                            function formatValue(v) { return v.toFixed(0) }
                            onValueChanged: { settings.volume = Math.round(value); feed.command("setVolume", String(settings.volume)) }
                        }
                    }
                }
            }
        }
    }
}
