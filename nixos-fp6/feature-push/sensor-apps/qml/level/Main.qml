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
    readonly property int mode: sections.selectedIndex
    property bool hold: false
    property var held: ({})
    readonly property var v: hold ? held : d

    // zeroed readings (calibrate-to-zero offsets per mode)
    readonly property real bx: (v.flatX || 0) - settings.zeroX
    readonly property real by: (v.flatY || 0) - settings.zeroY
    readonly property real edge: (v.edgeDeviation || 0) - settings.zeroEdge
    readonly property real tol: settings.tolerance

    SensorFeed {
        id: feed
        kind: "level"
        interval: 33
        active: Qt.application.state === Qt.ApplicationActive
    }

    Settings {
        id: settings
        category: "level"
        property real zeroX: 0
        property real zeroY: 0
        property real zeroEdge: 0
        property real tolerance: 0.3
        property int mode: 0
    }

    Component.onCompleted: {
        var m = ["bubble", "edge", "incline", "protractor"].indexOf(fp6Preview);
        sections.selectedIndex = m >= 0 ? m : settings.mode;
    }

    function zero() {
        if (mode === 0) { settings.zeroX = d.flatX || 0; settings.zeroY = d.flatY || 0; }
        else if (mode === 1) settings.zeroEdge = d.edgeDeviation || 0;
    }

    Page {
        anchors.fill: parent
        header: PageHeader {
            id: header
            title: i18n.tr("Level")
            trailingActionBar.actions: [
                Action {
                    iconName: root.hold ? "media-playback-start" : "media-playback-pause"
                    text: root.hold ? i18n.tr("Resume") : i18n.tr("Hold")
                    onTriggered: { root.held = feed.data; root.hold = !root.hold; }
                },
                Action {
                    iconName: "reset"
                    text: i18n.tr("Zero")
                    visible: root.mode < 2
                    onTriggered: root.zero()
                }
            ]
            extension: Sections {
                id: sections
                anchors { left: parent.left; bottom: parent.bottom }
                model: [i18n.tr("Bubble"), i18n.tr("Edge"), i18n.tr("Inclinometer"), i18n.tr("Protractor")]
                onSelectedIndexChanged: settings.mode = selectedIndex
            }
        }

        Item {
            id: body
            anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom; margins: units.gu(2) }

            Notice {
                id: err
                visible: d.error !== undefined
                iconName: "dialog-warning-symbolic"
                accent: theme.palette.normal.negative
                text: d.error || ""
            }

            // ---- 2-axis bubble -------------------------------------------------
            Column {
                visible: root.mode === 0
                anchors.fill: parent
                spacing: units.gu(2)
                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: v.flat === false ? i18n.tr("Lay the phone flat, screen up.") : i18n.tr("Lay the phone on the surface. Zero sets the current tilt as level.")
                }
                Bubble {
                    width: Math.min(parent.width, parent.height - units.gu(16))
                    height: width
                    anchors.horizontalCenter: parent.horizontalCenter
                    dx: v.flat ? root.bx : 0
                    dy: v.flat ? root.by : 0
                    opacity: v.flat === false ? 0.4 : 1
                    tolerance: root.tol
                }
                Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: units.gu(6)
                    Reading { caption: i18n.tr("Left–right"); value: v.flat ? Fmt.signed(root.bx, 1) + "°" : "–"; valueColor: Math.abs(root.bx) <= root.tol ? theme.palette.normal.positive : theme.palette.normal.backgroundText }
                    Reading { caption: i18n.tr("Front–back"); value: v.flat ? Fmt.signed(root.by, 1) + "°" : "–"; valueColor: Math.abs(root.by) <= root.tol ? theme.palette.normal.positive : theme.palette.normal.backgroundText }
                }
            }

            // ---- 1-axis edge (tube) --------------------------------------------
            Column {
                visible: root.mode === 1
                anchors.fill: parent
                spacing: units.gu(3)
                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("Stand the phone on one of its edges (long or short) against the surface.")
                }
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: Fmt.signed(root.edge, 1) + "°"
                    font.pixelSize: units.gu(8)
                    font.weight: Font.Light
                    color: Math.abs(root.edge) <= root.tol ? theme.palette.normal.positive : theme.palette.normal.backgroundText
                    rotation: -(v.edgeQuadrant || 0)
                }
                Tube {
                    width: parent.width
                    height: units.gu(8)
                    deviation: root.edge
                    tolerance: root.tol
                }
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: v.flat ? i18n.tr("The phone is lying flat – use Bubble mode.")
                                 : ((v.edgeQuadrant === 0 || v.edgeQuadrant === 180) ? i18n.tr("standing on its short edge (plumb)") : i18n.tr("lying on its long edge (level)"))
                }
            }

            // ---- inclinometer ----------------------------------------------------
            Column {
                visible: root.mode === 2
                anchors.fill: parent
                spacing: units.gu(2)
                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: v.flat ? i18n.tr("Phone lying on a surface: slope of the surface.") : i18n.tr("Phone standing on an edge: angle of the edge from vertical.")
                }
                Dial {
                    width: Math.min(parent.width, parent.height - units.gu(18))
                    height: width
                    anchors.horizontalCenter: parent.horizontalCenter
                    angle: v.flat ? (v.slope || 0) : (v.roll || 0)
                    label: Fmt.num(Math.abs(v.flat ? (v.slope || 0) : (v.roll || 0)), 1) + "°"
                }
                Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: units.gu(4)
                    Reading { caption: i18n.tr("Slope"); value: Fmt.num(v.slope, 1) + "°" }
                    Reading { caption: i18n.tr("Grade"); value: (v.slope || 0) < 60 ? Fmt.num(v.grade, 1) + " %" : "–" }
                    Reading { caption: i18n.tr("Rotation"); value: Fmt.signed(v.roll, 1) + "°" }
                }
            }

            // ---- protractor (relative angle) ----------------------------------
            Column {
                visible: root.mode === 3
                anchors.fill: parent
                spacing: units.gu(2)
                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("Put the phone's edge on the first surface and tap Set start. Then put it on the second surface: the angle between them is shown.")
                }
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: v.refAngle !== undefined ? Fmt.num(Math.abs(v.flat ? v.refAngle : v.refRoll), 1) + "°" : "–"
                    font.pixelSize: units.gu(10)
                    font.weight: Font.Light
                }
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: v.refAngle !== undefined
                          ? i18n.tr("in-plane %1° · 3D %2°").arg(Fmt.signed(v.refRoll, 1)).arg(Fmt.num(v.refAngle, 1))
                          : i18n.tr("no start position yet")
                }
                Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: units.gu(1)
                    Button {
                        text: i18n.tr("Set start")
                        color: theme.palette.normal.positive
                        onClicked: { root.hold = false; feed.command("setReference") }
                    }
                    Button {
                        text: i18n.tr("Clear")
                        onClicked: feed.command("clearReference")
                    }
                }
            }

            Label {
                anchors { bottom: parent.bottom; horizontalCenter: parent.horizontalCenter }
                textSize: Label.XSmall
                color: theme.palette.normal.backgroundTertiaryText
                text: (root.hold ? i18n.tr("HOLD · ") : "")
                      + (d.gyro ? i18n.tr("accelerometer + gyroscope") : i18n.tr("accelerometer"))
                      + (d.steady === false ? i18n.tr(" · moving") : "")
                      + ((settings.zeroX || settings.zeroY || settings.zeroEdge) ? i18n.tr(" · zero offset set") : "")
            }
        }
    }
}
