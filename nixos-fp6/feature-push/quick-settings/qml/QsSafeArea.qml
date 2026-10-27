/*
 * FP6: draws the rounded-corner arcs and the status bar's side insets
 * (com.fairphone6.quick-settings show-safe-area), to calibrate the radius.
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15
import Lomiri.Components 1.3

Item {
    id: area
    property real radius: 0
    property real barHeight: units.gu(3)
    property real inset: 0
    Canvas {
        anchors.fill: parent
        onPaint: {
            var c = getContext("2d"), r = area.radius, w = width, h = height;
            c.reset();
            c.lineWidth = 2;
            c.strokeStyle = "#ff3b30";
            c.beginPath(); c.arc(r, r, r, Math.PI, 1.5 * Math.PI); c.stroke();
            c.beginPath(); c.arc(w - r, r, r, 1.5 * Math.PI, 2 * Math.PI); c.stroke();
            c.beginPath(); c.arc(r, h - r, r, 0.5 * Math.PI, Math.PI); c.stroke();
            c.beginPath(); c.arc(w - r, h - r, r, 0, 0.5 * Math.PI); c.stroke();
            c.strokeStyle = "#34c759";
            c.strokeRect(area.inset, 0, w - 2 * area.inset, area.barHeight);
        }
    }
}
