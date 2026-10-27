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
    readonly property int steps: d.steps || 0
    readonly property real km: steps * settings.stepCm / 100000

    SensorFeed {
        id: feed
        kind: "steps"
        interval: 100
        // counts only while the app is in front: no background tracking
        active: Qt.application.state === Qt.ApplicationActive
    }

    Settings {
        id: settings
        category: "steps"
        property real stepCm: 70
        property int goal: 1000
        property bool miles: false
    }

    Component.onCompleted: {
        stack.push(mainPage);
        if (fp6Preview === "settings") stack.push(settingsPage);
    }

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
            title: i18n.tr("Step counter")
            trailingActionBar.actions: [
                Action {
                    iconName: "settings"
                    text: i18n.tr("Settings")
                    onTriggered: stack.push(settingsPage)
                },
                Action {
                    iconName: "reset"
                    text: i18n.tr("Reset")
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
                    iconName: "dialog-warning-symbolic"
                    accent: theme.palette.normal.negative
                    text: d.error || ""
                }

                ArcGauge {
                    width: Math.min(parent.width, units.gu(32))
                    anchors.horizontalCenter: parent.horizontalCenter
                    showLabels: false
                    maximum: settings.goal
                    value: root.steps
                    color: root.steps >= settings.goal ? theme.palette.normal.positive : theme.palette.normal.activity

                    Column {
                        anchors.centerIn: parent
                        Label {
                            anchors.horizontalCenter: parent.horizontalCenter
                            text: root.steps
                            font.pixelSize: units.gu(10)
                            font.weight: Font.Light
                        }
                        Label {
                            anchors.horizontalCenter: parent.horizontalCenter
                            text: i18n.tr("of %1 steps").arg(settings.goal)
                            color: theme.palette.normal.backgroundSecondaryText
                        }
                        Row {
                            anchors.horizontalCenter: parent.horizontalCenter
                            spacing: units.gu(0.5)
                            Rectangle {
                                anchors.verticalCenter: parent.verticalCenter
                                width: units.gu(1); height: width; radius: width / 2
                                color: d.walking ? theme.palette.normal.positive : theme.palette.normal.base
                            }
                            Label {
                                text: d.walking ? i18n.tr("walking") : i18n.tr("waiting for steps")
                                textSize: Label.Small
                                color: theme.palette.normal.backgroundSecondaryText
                            }
                        }
                    }
                }

                Row {
                    width: parent.width
                    Reading {
                        width: parent.width / 3
                        caption: i18n.tr("Distance")
                        value: settings.miles ? Fmt.num(root.km * 0.621371, 2) + " mi" : Fmt.num(root.km, 2) + " km"
                    }
                    Reading { width: parent.width / 3; caption: i18n.tr("Cadence"); value: d.cadence > 0 ? Fmt.num(d.cadence, 0) + i18n.tr("/min") : "–" }
                    Reading { width: parent.width / 3; caption: i18n.tr("Time counted"); value: root.duration(d.time) }
                }

                Notice {
                    iconName: "info"
                    text: i18n.tr("Steps are counted only while this app is open in front and the screen is on. Keep the phone in your hand or a front pocket. Nothing is tracked in the background and nothing is saved.")
                }

                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("Counting starts after four regular steps, so bumps and putting the phone down are ignored; those first steps are then added. Distance uses your step length (%1 cm, Settings).").arg(settings.stepCm.toFixed(0))
                }
            }
        }
    }

    Page {
        id: settingsPage
        visible: false
        header: PageHeader { id: sh; title: i18n.tr("Step counter settings") }
        Flickable {
            anchors { top: sh.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            contentHeight: sc.height + units.gu(2)
            clip: true
            Column {
                id: sc
                width: parent.width
                ListItem {
                    height: c1.height + units.gu(3)
                    Column {
                        id: c1
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                        spacing: units.gu(1)
                        Label { text: i18n.tr("Step length: %1 cm").arg(settings.stepCm.toFixed(0)) }
                        Slider {
                            width: parent.width
                            minimumValue: 40
                            maximumValue: 120
                            value: settings.stepCm
                            live: true
                            function formatValue(v) { return v.toFixed(0) }
                            onValueChanged: settings.stepCm = Math.round(value)
                        }
                        Label {
                            width: parent.width
                            wrapMode: Text.Wrap
                            textSize: Label.Small
                            color: theme.palette.normal.backgroundSecondaryText
                            text: i18n.tr("Walk 10 steps, measure the distance and divide by 10. A rough guide is 0.41 × your height.")
                        }
                    }
                }
                ListItem {
                    height: c2.height + units.gu(3)
                    Column {
                        id: c2
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                        spacing: units.gu(1)
                        Label { text: i18n.tr("Goal: %1 steps").arg(settings.goal) }
                        Slider {
                            width: parent.width
                            minimumValue: 100
                            maximumValue: 20000
                            value: settings.goal
                            live: true
                            function formatValue(v) { return (Math.round(v / 100) * 100).toFixed(0) }
                            onValueChanged: settings.goal = Math.max(100, Math.round(value / 100) * 100)
                        }
                    }
                }
                ListItem {
                    height: l3.height + divider.height
                    ListItemLayout {
                        id: l3
                        title.text: i18n.tr("Distance in miles")
                        Switch {
                            SlotsLayout.position: SlotsLayout.Trailing
                            checked: settings.miles
                            onCheckedChanged: settings.miles = checked
                        }
                    }
                }
            }
        }
    }
}
