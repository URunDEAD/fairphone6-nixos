// Arc gauge (270 degrees) with a value arc and tick labels; the centre is
// left free for a big reading (put children inside).
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    id: gauge
    property real value: NaN
    property real minimum: 0
    property real maximum: 100
    property int ticks: 6
    property int decimals: 0
    property color color: theme.palette.normal.activity
    property real marker: NaN          // e.g. the maximum so far
    property bool showLabels: true
    implicitWidth: units.gu(36)
    implicitHeight: width

    onValueChanged: canvas.requestPaint()
    onMaximumChanged: canvas.requestPaint()
    onMarkerChanged: canvas.requestPaint()
    onWidthChanged: canvas.requestPaint()
    Connections {
        target: theme
        function onNameChanged() { canvas.requestPaint() }
    }

    Canvas {
        id: canvas
        anchors.fill: parent
        onPaint: {
            var ctx = getContext("2d");
            ctx.reset();
            var w = width, h = height, cx = w / 2, cy = h / 2;
            var lw = units.gu(1.6);
            var r = Math.min(w, h) / 2 - lw;
            var a0 = Math.PI * 0.75, a1 = Math.PI * 2.25;
            function ang(v) {
                var f = (v - gauge.minimum) / (gauge.maximum - gauge.minimum);
                return a0 + Math.max(0, Math.min(1, f)) * (a1 - a0);
            }
            ctx.lineCap = "round";
            ctx.lineWidth = lw;
            ctx.strokeStyle = theme.palette.normal.base;
            ctx.beginPath(); ctx.arc(cx, cy, r, a0, a1, false); ctx.stroke();
            if (isFinite(gauge.value) && gauge.value > gauge.minimum) {
                ctx.strokeStyle = gauge.color;
                ctx.beginPath(); ctx.arc(cx, cy, r, a0, ang(gauge.value), false); ctx.stroke();
            }
            if (isFinite(gauge.marker) && gauge.marker > gauge.minimum) {
                var am = ang(gauge.marker);
                ctx.lineWidth = units.dp(3);
                ctx.lineCap = "butt";
                ctx.strokeStyle = theme.palette.normal.backgroundText;
                ctx.beginPath();
                ctx.moveTo(cx + (r - lw) * Math.cos(am), cy + (r - lw) * Math.sin(am));
                ctx.lineTo(cx + (r + lw * 0.5) * Math.cos(am), cy + (r + lw * 0.5) * Math.sin(am));
                ctx.stroke();
            }
            // tick labels inside the arc
            var fontPx = Math.round(units.gu(1.5));
            ctx.font = fontPx + "px sans-serif";
            ctx.fillStyle = theme.palette.normal.backgroundSecondaryText;
            ctx.textAlign = "center";
            ctx.textBaseline = "middle";
            for (var i = 0; gauge.showLabels && i <= gauge.ticks; i++) {
                var v = gauge.minimum + (gauge.maximum - gauge.minimum) * i / gauge.ticks;
                var a = ang(v);
                var rr = r - lw - fontPx * 1.2;
                ctx.fillText(v.toFixed(gauge.decimals), cx + rr * Math.cos(a), cy + rr * Math.sin(a));
            }
        }
    }
}
