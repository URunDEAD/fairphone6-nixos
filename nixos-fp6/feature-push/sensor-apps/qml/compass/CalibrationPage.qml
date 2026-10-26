// Figure-8 calibration: hard/soft-iron offsets, stored by the backend.
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import "../common"

Page {
    id: page
    property var feed
    readonly property var d: feed ? feed.data : ({})
    readonly property real coverage: d.coverage !== undefined ? d.coverage : 0
    property string result: ""

    header: PageHeader {
        id: ph
        title: i18n.tr("Calibrate compass")
    }

    Component.onCompleted: feed.command("calStart")
    Component.onDestruction: if (feed && feed.data.calibrating) feed.command("calCancel")

    Column {
        anchors { top: ph.bottom; left: parent.left; right: parent.right; margins: units.gu(2) }
        spacing: units.gu(2)

        Label {
            width: parent.width
            wrapMode: Text.Wrap
            text: i18n.tr("Hold the phone away from metal and magnets. Slowly draw a large figure 8 in the air while rotating the phone, so every side faces up, down and sideways at least once.")
        }

        Canvas {
            id: eight
            width: parent.width
            height: units.gu(16)
            property real phase: 0
            NumberAnimation on phase { from: 0; to: 2 * Math.PI; duration: 3000; loops: Animation.Infinite; running: Qt.application.state === Qt.ApplicationActive }
            onPhaseChanged: requestPaint()
            onPaint: {
                var ctx = getContext("2d");
                ctx.reset();
                var cx = width / 2, cy = height / 2, ax = Math.min(width / 2, units.gu(18)) - units.gu(2), ay = height / 2 - units.gu(2);
                ctx.strokeStyle = theme.palette.normal.base;
                ctx.lineWidth = units.dp(3);
                ctx.beginPath();
                for (var i = 0; i <= 100; i++) {
                    var t = i / 100 * 2 * Math.PI;
                    var x = cx + ax * Math.sin(t), y = cy + ay * Math.sin(t) * Math.cos(t) * 2;
                    if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
                }
                ctx.stroke();
                ctx.fillStyle = theme.palette.normal.activity;
                var px = cx + ax * Math.sin(phase), py = cy + ay * Math.sin(phase) * Math.cos(phase) * 2;
                ctx.beginPath(); ctx.arc(px, py, units.gu(1.2), 0, 2 * Math.PI); ctx.fill();
            }
        }

        Label {
            text: i18n.tr("Directions covered: %1 %").arg(Math.round(page.coverage * 100))
        }
        ProgressBar {
            width: parent.width
            minimumValue: 0
            maximumValue: 1
            value: page.coverage
        }
        Label {
            textSize: Label.Small
            color: theme.palette.normal.backgroundSecondaryText
            text: i18n.tr("%1 samples").arg(d.samples || 0)
        }

        Label {
            width: parent.width
            wrapMode: Text.Wrap
            visible: page.result !== ""
            text: page.result
            color: theme.palette.normal.negative
        }

        Row {
            anchors.right: parent.right
            spacing: units.gu(1)
            Button {
                text: i18n.tr("Cancel")
                onClicked: { feed.command("calCancel"); pageStack.pop() }
            }
            Button {
                text: i18n.tr("Done")
                color: theme.palette.normal.positive
                enabled: page.coverage >= 0.6
                onClicked: {
                    var r = feed.command("calFinish");
                    if (r.ok) {
                        var q = r.spread < 0.03 ? i18n.tr("good") : (r.spread < 0.08 ? i18n.tr("fair") : i18n.tr("poor – try again away from metal"));
                        PopupUtils.open(doneDialog, page, { quality: q, frameNote: r.frameConfident ? "" : i18n.tr("Turn the phone through more orientations next time to confirm the sensor axes.") });
                    } else {
                        page.result = r.reason || i18n.tr("Calibration failed.");
                    }
                }
            }
        }
    }

    Component {
        id: doneDialog
        Dialog {
            id: dlg
            property string quality
            property string frameNote
            title: i18n.tr("Calibration saved")
            text: i18n.tr("Fit quality: %1.").arg(quality) + (frameNote ? "\n" + frameNote : "")
            Button {
                text: i18n.tr("OK")
                color: theme.palette.normal.positive
                onClicked: { PopupUtils.close(dlg); pageStack.pop() }
            }
        }
    }
}
