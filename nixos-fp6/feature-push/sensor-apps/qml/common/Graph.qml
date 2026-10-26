// Line graph for one or more series (Canvas, theme colours).
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    id: graph
    // [{ values: [..], color: "#..." }]
    property var series: []
    property real fixedMin: NaN
    property real fixedMax: NaN
    property real minSpan: 1
    property int decimals: 1
    property string unit: ""
    property string leftLabel: ""
    property string rightLabel: ""
    implicitHeight: units.gu(16)

    onSeriesChanged: canvas.requestPaint()
    onWidthChanged: canvas.requestPaint()
    onHeightChanged: canvas.requestPaint()
    Connections {
        target: theme
        function onNameChanged() { canvas.requestPaint() }
    }

    Rectangle {
        anchors.fill: parent
        radius: units.gu(1)
        color: theme.palette.normal.foreground
    }

    Canvas {
        id: canvas
        anchors.fill: parent
        anchors.margins: units.gu(1)
        onPaint: {
            var ctx = getContext("2d");
            ctx.reset();
            var w = width, h = height;
            var lo = Infinity, hi = -Infinity, n = 0;
            for (var s = 0; s < graph.series.length; s++) {
                var v = graph.series[s].values || [];
                for (var i = 0; i < v.length; i++) {
                    if (v[i] === null || !isFinite(v[i])) continue;
                    lo = Math.min(lo, v[i]); hi = Math.max(hi, v[i]); n++;
                }
            }
            if (isFinite(graph.fixedMin)) lo = graph.fixedMin;
            if (isFinite(graph.fixedMax)) hi = graph.fixedMax;
            var textColor = theme.palette.normal.backgroundSecondaryText;
            var fontPx = Math.round(units.gu(1.4));
            ctx.font = fontPx + "px sans-serif";
            if (n === 0 && !(isFinite(graph.fixedMin) && isFinite(graph.fixedMax))) {
                ctx.fillStyle = textColor;
                ctx.fillText(i18n.tr("waiting for data…"), w / 2 - units.gu(6), h / 2);
                return;
            }
            if (hi - lo < graph.minSpan) {
                var mid = (hi + lo) / 2;
                lo = mid - graph.minSpan / 2; hi = mid + graph.minSpan / 2;
                if (isFinite(graph.fixedMin) && lo < graph.fixedMin) { lo = graph.fixedMin; hi = lo + graph.minSpan; }
                if (isFinite(graph.fixedMax) && hi > graph.fixedMax) { hi = graph.fixedMax; lo = hi - graph.minSpan; }
            }
            var pad = (hi - lo) * 0.08;
            if (!isFinite(graph.fixedMin)) lo -= pad;
            if (!isFinite(graph.fixedMax)) hi += pad;
            var top = fontPx * 0.2, bottom = h - fontPx * 1.4;
            function yOf(val) { return bottom - (val - lo) / (hi - lo) * (bottom - top); }
            // grid
            ctx.strokeStyle = theme.palette.normal.base;
            ctx.lineWidth = 1;
            ctx.fillStyle = textColor;
            for (var gl = 0; gl <= 3; gl++) {
                var gv = lo + (hi - lo) * gl / 3;
                var gy = yOf(gv);
                ctx.beginPath(); ctx.moveTo(0, gy); ctx.lineTo(w, gy); ctx.stroke();
                ctx.fillText(gv.toFixed(graph.decimals) + (gl === 3 ? " " + graph.unit : ""), 2, gy - 3);
            }
            ctx.fillText(graph.leftLabel, 0, h - 2);
            var rw = ctx.measureText(graph.rightLabel).width;
            ctx.fillText(graph.rightLabel, w - rw, h - 2);
            // lines
            ctx.lineWidth = units.dp(2);
            ctx.lineJoin = "round";
            for (var k = 0; k < graph.series.length; k++) {
                var vals = graph.series[k].values || [];
                if (vals.length < 2) continue;
                ctx.strokeStyle = graph.series[k].color || theme.palette.normal.activity;
                ctx.beginPath();
                var started = false;
                for (var j = 0; j < vals.length; j++) {
                    var x = j / (vals.length - 1) * w;
                    if (vals[j] === null || !isFinite(vals[j])) { started = false; continue; }
                    var y = Math.max(top, Math.min(bottom, yOf(vals[j])));
                    if (!started) { ctx.moveTo(x, y); started = true; } else ctx.lineTo(x, y);
                }
                ctx.stroke();
            }
        }
    }
}
