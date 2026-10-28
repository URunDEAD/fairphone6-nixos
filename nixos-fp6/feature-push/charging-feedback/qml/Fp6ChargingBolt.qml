/*
 * FP6 charging feedback: a lightning bolt (painted once, scales with size).
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15

Canvas {
    id: bolt
    property color color: "white"
    onColorChanged: requestPaint()
    onWidthChanged: requestPaint()
    onHeightChanged: requestPaint()
    antialiasing: true
    onPaint: {
        var ctx = getContext("2d");
        ctx.reset();
        var w = width, h = height;
        if (w <= 0 || h <= 0) return;
        // unit bolt in a 62 x 100 box
        var pts = [[38, 0], [4, 56], [28, 56], [22, 100], [58, 40], [34, 40], [44, 0]];
        ctx.fillStyle = bolt.color;
        ctx.beginPath();
        for (var i = 0; i < pts.length; i++) {
            var x = pts[i][0] / 62 * w, y = pts[i][1] / 100 * h;
            if (i === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
        }
        ctx.closePath();
        ctx.fill();
    }
}
