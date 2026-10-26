// Polar sky plot: zenith in the centre, horizon at the rim, north up.
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    id: plot
    property var sats: []
    property var colorFor: function (c) { return LomiriColors.blue }

    onSatsChanged: canvas.requestPaint()

    Canvas {
        id: canvas
        anchors.fill: parent
        onPaint: {
            var ctx = getContext("2d");
            ctx.reset();
            var cx = width / 2, cy = height / 2, r = Math.min(width, height) / 2 - units.gu(2);
            var dim = theme.palette.normal.backgroundSecondaryText;
            ctx.fillStyle = theme.palette.normal.foreground;
            ctx.beginPath(); ctx.arc(cx, cy, r, 0, 2 * Math.PI); ctx.fill();
            ctx.strokeStyle = theme.palette.normal.base;
            ctx.lineWidth = units.dp(1);
            [30, 60].forEach(function (el) {
                ctx.beginPath(); ctx.arc(cx, cy, r * (90 - el) / 90, 0, 2 * Math.PI); ctx.stroke();
            });
            ctx.beginPath(); ctx.arc(cx, cy, r, 0, 2 * Math.PI); ctx.stroke();
            ctx.beginPath(); ctx.moveTo(cx - r, cy); ctx.lineTo(cx + r, cy); ctx.moveTo(cx, cy - r); ctx.lineTo(cx, cy + r); ctx.stroke();
            ctx.fillStyle = dim;
            ctx.font = Math.round(units.gu(1.6)) + "px sans-serif";
            ctx.textAlign = "center"; ctx.textBaseline = "middle";
            ctx.fillText("N", cx, cy - r - units.gu(1));
            ctx.fillText("S", cx, cy + r + units.gu(1));
            ctx.fillText("E", cx + r + units.gu(1), cy);
            ctx.fillText("W", cx - r - units.gu(1), cy);
            for (var i = 0; i < sats.length; i++) {
                var s = sats[i];
                if (s.el === null || s.az === null || s.el === undefined) continue;
                var rr = r * (90 - Math.max(0, s.el)) / 90, a = (s.az - 90) * Math.PI / 180;
                var x = cx + rr * Math.cos(a), y = cy + rr * Math.sin(a);
                var tracked = s.snr !== null && s.snr > 0;
                ctx.fillStyle = plot.colorFor(s.c);
                ctx.globalAlpha = tracked ? 1 : 0.35;
                ctx.beginPath(); ctx.arc(x, y, units.gu(1.4), 0, 2 * Math.PI); ctx.fill();
                ctx.globalAlpha = 1;
                ctx.fillStyle = "white";
                ctx.font = Math.round(units.gu(1.2)) + "px sans-serif";
                ctx.fillText(String(s.prn), x, y);
            }
        }
    }
}
