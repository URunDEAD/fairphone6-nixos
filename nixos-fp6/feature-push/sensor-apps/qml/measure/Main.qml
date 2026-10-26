import QtQuick 2.12
import QtQuick.Window 2.12
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
    property var panel: ({})
    // logical (QML) pixels per millimetre
    readonly property real pxPerMm: (panel.pxPerMm ? panel.pxPerMm / Screen.devicePixelRatio : Screen.pixelDensity) * settings.rulerScale

    SensorFeed {
        id: feed
        kind: "measure"
        interval: 50
        // the angle readout is only needed on the height/distance tab
        active: Qt.application.state === Qt.ApplicationActive && sections.selectedIndex === 1
    }

    Settings {
        id: settings
        category: "measure"
        property real rulerScale: 1.0
        property real eyeHeight: 1.60
        property bool useCamera: false
        property bool inches: false
    }

    Component.onCompleted: {
        panel = feed.command("panel");
        if (fp6Preview === "height") sections.selectedIndex = 1;
        if (fp6Preview === "card") stack.push(cardPage);
        if (fp6Preview !== "card") stack.push(mainPage);
    }

    PageStack { id: stack }

    Page {
        id: mainPage
        visible: false
        header: PageHeader {
            id: header
            title: i18n.tr("Measure")
            trailingActionBar.actions: [
                Action {
                    iconName: "settings"
                    text: i18n.tr("Ruler calibration")
                    onTriggered: stack.push(cardPage)
                }
            ]
            extension: Sections {
                id: sections
                anchors { left: parent.left; bottom: parent.bottom }
                model: [i18n.tr("Ruler"), i18n.tr("Height & distance")]
            }
        }

        // ---- ruler ---------------------------------------------------------------
        Item {
            id: rulerArea
            visible: sections.selectedIndex === 0
            anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            property real markY: units.gu(20)

            Canvas {
                id: ruler
                anchors.fill: parent
                property real ppm: root.pxPerMm
                onPpmChanged: requestPaint()
                onHeightChanged: requestPaint()
                onPaint: {
                    var ctx = getContext("2d");
                    ctx.reset();
                    var fg = theme.palette.normal.backgroundText;
                    ctx.strokeStyle = fg; ctx.fillStyle = fg;
                    ctx.font = Math.round(units.gu(1.8)) + "px sans-serif";
                    ctx.textBaseline = "middle";
                    var mm = 0;
                    // left edge: millimetres
                    for (var y = 0; y <= height; mm++, y = mm * ppm) {
                        var len = mm % 10 === 0 ? units.gu(4) : (mm % 5 === 0 ? units.gu(2.6) : units.gu(1.5));
                        ctx.lineWidth = mm % 10 === 0 ? units.dp(2) : units.dp(1);
                        ctx.beginPath(); ctx.moveTo(0, y + 0.5); ctx.lineTo(len, y + 0.5); ctx.stroke();
                        if (mm % 10 === 0 && mm > 0) ctx.fillText(String(mm / 10), units.gu(4.6), y);
                    }
                    // right edge: 1/16 inch
                    var six = 0, ppi = ppm * 25.4;
                    ctx.textAlign = "right";
                    for (var yi = 0; yi <= height; six++, yi = six * ppi / 16) {
                        var l = six % 16 === 0 ? units.gu(4) : (six % 8 === 0 ? units.gu(3) : (six % 4 === 0 ? units.gu(2.2) : (six % 2 === 0 ? units.gu(1.6) : units.gu(1))));
                        ctx.lineWidth = six % 16 === 0 ? units.dp(2) : units.dp(1);
                        ctx.beginPath(); ctx.moveTo(width, yi + 0.5); ctx.lineTo(width - l, yi + 0.5); ctx.stroke();
                        if (six % 16 === 0 && six > 0) ctx.fillText(String(six / 16), width - units.gu(4.6), yi);
                    }
                    ctx.textAlign = "left";
                    ctx.fillText("cm", units.gu(4.6), units.gu(1.5));
                    ctx.textAlign = "right";
                    ctx.fillText("in", width - units.gu(4.6), units.gu(1.5));
                }
                Connections {
                    target: theme
                    function onNameChanged() { ruler.requestPaint() }
                }
            }

            // draggable measuring line
            Rectangle {
                id: mark
                width: parent.width
                height: units.dp(2)
                y: rulerArea.markY
                color: theme.palette.normal.activity
            }
            Rectangle {
                anchors { horizontalCenter: parent.horizontalCenter; verticalCenter: mark.verticalCenter }
                width: readout.width + units.gu(3); height: readout.height + units.gu(1.5)
                radius: height / 2
                color: theme.palette.normal.activity
                Label {
                    id: readout
                    anchors.centerIn: parent
                    color: theme.palette.normal.activityText
                    text: Fmt.num(rulerArea.markY / root.pxPerMm / 10, 2) + " cm  ·  " + Fmt.num(rulerArea.markY / root.pxPerMm / 25.4, 2) + " in"
                }
            }
            MouseArea {
                anchors.fill: parent
                onPressed: rulerArea.markY = Math.max(0, Math.min(height, mouse.y))
                onPositionChanged: rulerArea.markY = Math.max(0, Math.min(height, mouse.y))
            }
            Label {
                anchors { bottom: parent.bottom; horizontalCenter: parent.horizontalCenter; bottomMargin: units.gu(1) }
                width: parent.width - units.gu(20)
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                textSize: Label.XSmall
                color: theme.palette.normal.backgroundSecondaryText
                text: i18n.tr("0 is the top edge of this area. Drag to measure.") + "\n"
                      + (root.panel.detected ? i18n.tr("Panel: %1 px/mm").arg(Fmt.num(root.panel.pxPerMm, 2)) : i18n.tr("Panel not recognised: using the reported screen density"))
                      + (settings.rulerScale !== 1 ? i18n.tr(", adjusted %1 %").arg(Fmt.signed((settings.rulerScale - 1) * 100, 1)) : "")
            }
        }

        // ---- height & distance ------------------------------------------------
        Flickable {
            visible: sections.selectedIndex === 1
            anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            contentHeight: hd.height + units.gu(4)
            clip: true

            Column {
                id: hd
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                spacing: units.gu(2)

                property real footAngle: NaN
                property real topAngle: NaN
                readonly property real aim: (settings.useCamera ? d.backElevation : d.topElevation) || 0
                readonly property var result: isFinite(footAngle)
                    ? feed.command("sight", settings.eyeHeight + "," + footAngle + "," + (isFinite(topAngle) ? topAngle : ""))
                    : ({})

                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    text: settings.useCamera
                          ? i18n.tr("Hold the phone upright at eye height and look along the back of the phone (camera direction).")
                          : i18n.tr("Hold the phone at eye height and sight along its top edge, like aiming along a ruler.")
                }
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: d.topElevation !== undefined ? Fmt.signed(hd.aim, 1) + "°" : "–"
                    font.pixelSize: units.gu(7)
                    font.weight: Font.Light
                }
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("angle above (+) or below (−) the horizon")
                }
                Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    spacing: units.gu(1)
                    Button {
                        text: i18n.tr("1. Aim at the foot")
                        color: theme.palette.normal.positive
                        onClicked: { hd.footAngle = hd.aim; hd.topAngle = NaN }
                    }
                    Button {
                        text: i18n.tr("2. Aim at the top")
                        enabled: isFinite(hd.footAngle)
                        onClicked: hd.topAngle = hd.aim
                    }
                }
                Column {
                    width: parent.width
                    InfoRow {
                        name: i18n.tr("Distance")
                        value: hd.result.ok ? Fmt.num(hd.result.distance, 1) + " m"
                               : (isFinite(hd.footAngle) ? i18n.tr("aim below the horizon") : "–")
                    }
                    InfoRow {
                        name: i18n.tr("Height")
                        value: hd.result.ok && hd.result.height !== undefined ? Fmt.num(hd.result.height, 1) + " m" : "–"
                    }
                    InfoRow {
                        name: i18n.tr("Angles used")
                        value: (isFinite(hd.footAngle) ? Fmt.signed(hd.footAngle, 1) + "°" : "–") + " / " + (isFinite(hd.topAngle) ? Fmt.signed(hd.topAngle, 1) + "°" : "–")
                    }
                }
                Row {
                    spacing: units.gu(1)
                    Label {
                        anchors.verticalCenter: parent.verticalCenter
                        text: i18n.tr("Eye height (m)")
                    }
                    TextField {
                        width: units.gu(10)
                        text: settings.eyeHeight.toFixed(2)
                        inputMethodHints: Qt.ImhFormattedNumbersOnly
                        onAccepted: {
                            var h = parseFloat(text.replace(",", "."));
                            if (isFinite(h) && h > 0.2 && h < 3) settings.eyeHeight = h;
                        }
                        onActiveFocusChanged: if (!activeFocus) accepted()
                    }
                }
                ListItem {
                    height: l1.height + divider.height
                    divider.visible: false
                    ListItemLayout {
                        id: l1
                        padding.leading: 0
                        title.text: i18n.tr("Aim with the camera direction")
                        subtitle.text: i18n.tr("Off: sight along the top edge")
                        Switch {
                            SlotsLayout.position: SlotsLayout.Trailing
                            checked: settings.useCamera
                            onCheckedChanged: settings.useCamera = checked
                        }
                    }
                }
                Label {
                    width: parent.width
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("Assumes level ground between you and the object. Distance = eye height ÷ tan(angle to the foot); height = eye height + distance × tan(angle to the top). Small angles near the horizon give large errors.")
                }
            }
        }
    }

    // ---- ruler calibration with a bank card -----------------------------------
    Page {
        id: cardPage
        visible: false
        header: PageHeader {
            id: ch
            title: i18n.tr("Ruler calibration")
        }
        Column {
            anchors { top: ch.bottom; left: parent.left; right: parent.right; margins: units.gu(2) }
            spacing: units.gu(2)
            Label {
                width: parent.width
                wrapMode: Text.Wrap
                text: i18n.tr("The ruler uses the panel size (6.31\", 1116 × 2484). To check it, hold a bank card (85.60 × 53.98 mm) against the screen: adjust until the card and the outline match.")
            }
            Rectangle {
                width: 53.98 * root.pxPerMm
                height: 85.60 * root.pxPerMm
                anchors.horizontalCenter: parent.horizontalCenter
                color: "transparent"
                radius: 3.18 * root.pxPerMm
                border.color: theme.palette.normal.activity
                border.width: units.dp(2)
                Label {
                    anchors.centerIn: parent
                    text: "85.60 × 53.98 mm"
                    color: theme.palette.normal.backgroundSecondaryText
                }
            }
            Label { text: i18n.tr("Adjustment: %1 %").arg(Fmt.signed((settings.rulerScale - 1) * 100, 1)) }
            Slider {
                width: parent.width
                minimumValue: 0.95
                maximumValue: 1.05
                value: settings.rulerScale
                live: true
                function formatValue(v) { return Fmt.signed((v - 1) * 100, 1) + " %" }
                onValueChanged: settings.rulerScale = value
            }
            Button {
                text: i18n.tr("Reset")
                onClicked: settings.rulerScale = 1.0
            }
        }
    }
}
