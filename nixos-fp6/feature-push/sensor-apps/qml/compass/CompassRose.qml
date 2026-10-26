// Compass card: rotates so that north on the card points to north.
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    id: rose
    property real heading: 0          // degrees the phone points to
    property real declination: 0      // for the magnetic-north mark
    property bool showMagnetic: false
    property bool upright: false

    Canvas {
        id: card
        anchors.fill: parent
        rotation: -rose.heading
        antialiasing: true
        onPaint: {
            var ctx = getContext("2d");
            ctx.reset();
            var w = width, r = w / 2 - units.gu(0.5), cx = w / 2, cy = w / 2;
            var fg = theme.palette.normal.backgroundText;
            var dim = theme.palette.normal.backgroundSecondaryText;
            ctx.fillStyle = theme.palette.normal.foreground;
            ctx.beginPath(); ctx.arc(cx, cy, r, 0, 2 * Math.PI); ctx.fill();
            ctx.lineCap = "round";
            for (var a = 0; a < 360; a += 5) {
                var big = a % 30 === 0, mid = a % 10 === 0;
                var len = big ? units.gu(2) : (mid ? units.gu(1.3) : units.gu(0.7));
                var t = (a - 90) * Math.PI / 180;
                ctx.strokeStyle = big ? fg : dim;
                ctx.lineWidth = big ? units.dp(2) : units.dp(1);
                ctx.beginPath();
                ctx.moveTo(cx + Math.cos(t) * r, cy + Math.sin(t) * r);
                ctx.lineTo(cx + Math.cos(t) * (r - len), cy + Math.sin(t) * (r - len));
                ctx.stroke();
            }
            var names = ["N", "E", "S", "W"];
            ctx.textAlign = "center";
            ctx.textBaseline = "middle";
            for (var i = 0; i < 12; i++) {
                var deg = i * 30;
                var tt = (deg - 90) * Math.PI / 180;
                var cardinal = deg % 90 === 0;
                var txt = cardinal ? names[deg / 90] : String(deg);
                ctx.font = (cardinal ? "bold " + Math.round(units.gu(2.6)) : Math.round(units.gu(1.6))) + "px sans-serif";
                ctx.fillStyle = deg === 0 ? LomiriColors.red : (cardinal ? fg : dim);
                var rr = r - units.gu(cardinal ? 4.2 : 3.6);
                ctx.save();
                ctx.translate(cx + Math.cos(tt) * rr, cy + Math.sin(tt) * rr);
                ctx.rotate(deg * Math.PI / 180);
                ctx.fillText(txt, 0, 0);
                ctx.restore();
            }
            // north needle
            ctx.fillStyle = LomiriColors.red;
            ctx.beginPath();
            ctx.moveTo(cx, cy - r * 0.55);
            ctx.lineTo(cx - units.gu(1.2), cy);
            ctx.lineTo(cx + units.gu(1.2), cy);
            ctx.closePath(); ctx.fill();
            ctx.fillStyle = dim;
            ctx.beginPath();
            ctx.moveTo(cx, cy + r * 0.55);
            ctx.lineTo(cx - units.gu(1.2), cy);
            ctx.lineTo(cx + units.gu(1.2), cy);
            ctx.closePath(); ctx.fill();
            ctx.fillStyle = theme.palette.normal.background;
            ctx.beginPath(); ctx.arc(cx, cy, units.gu(0.6), 0, 2 * Math.PI); ctx.fill();
        }
        Connections {
            target: theme
            function onNameChanged() { card.requestPaint() }
        }
    }

    // magnetic north marker on the rim (when the card shows true north)
    Item {
        anchors.fill: parent
        visible: rose.showMagnetic
        rotation: -rose.heading - rose.declination
        Rectangle {
            width: units.gu(1.2); height: units.gu(1.2); radius: width / 2
            color: LomiriColors.blue
            anchors.horizontalCenter: parent.horizontalCenter
            y: units.gu(2.6)
        }
    }

    // lubber line: the direction the phone points
    Canvas {
        anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.top; bottomMargin: -units.gu(1.4) }
        width: units.gu(3); height: units.gu(2)
        onPaint: {
            var ctx = getContext("2d");
            ctx.reset();
            ctx.fillStyle = theme.palette.normal.activity;
            ctx.beginPath();
            ctx.moveTo(0, 0); ctx.lineTo(width, 0); ctx.lineTo(width / 2, height);
            ctx.closePath(); ctx.fill();
        }
    }
}
