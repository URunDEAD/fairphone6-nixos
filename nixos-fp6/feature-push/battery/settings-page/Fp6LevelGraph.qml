/*
 * Charge level over time: points [[t, level, plugged], ...] (fp6-batteryd
 * history.json). Plugged-in stretches are drawn in green, the rest in the
 * theme's foreground colour; vertical marks every 6 h (day) or day (week).
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import Lomiri.Components 1.3

Canvas {
    id: g
    property var points: []
    property int span: 86400
    property real now: Date.now() / 1000
    height: units.gu(20)

    onPointsChanged: requestPaint()
    onWidthChanged: requestPaint()

    onPaint: {
        var ctx = getContext("2d")
        ctx.reset()
        var left = units.gu(4), bottom = units.gu(3), top = units.gu(1)
        var w = width - left - units.gu(1), h = height - bottom - top
        var fg = theme.palette.normal.backgroundText
        var font = FontUtils.sizeToPixels("x-small")
        ctx.font = font + "px Ubuntu"
        ctx.fillStyle = fg
        ctx.strokeStyle = fg
        ctx.lineWidth = 1
        // y grid 0/50/100 %
        for (var p = 0; p <= 100; p += 50) {
            var y = top + h - h * p / 100
            ctx.globalAlpha = 0.3
            ctx.beginPath(); ctx.moveTo(left, y); ctx.lineTo(left + w, y); ctx.stroke()
            ctx.globalAlpha = 1
            ctx.fillText(p + "%", 0, y + font / 3)
        }
        // x marks
        var step = span > 2 * 86400 ? 86400 : 6 * 3600
        var d = new Date(now * 1000)
        var start = now - span
        var t0 = span > 2 * 86400
            ? new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime() / 1000
            : Math.floor(now / 3600) * 3600 - (d.getHours() % 6) * 3600
        for (var t = t0; t > start; t -= step) {
            var x = left + w * (t - start) / span
            ctx.globalAlpha = 0.3
            ctx.beginPath(); ctx.moveTo(x, top); ctx.lineTo(x, top + h); ctx.stroke()
            ctx.globalAlpha = 1
            var lab = span > 2 * 86400 ? Qt.formatDate(new Date(t * 1000), "ddd") : Qt.formatTime(new Date(t * 1000), "hh:mm")
            ctx.fillText(lab, x - font, top + h + font + units.gu(0.5))
        }
        if (!points || points.length < 2) {
            ctx.fillText(i18n.tr("Collecting data…"), left + w / 3, top + h / 2)
            return
        }
        ctx.lineWidth = units.dp(2)
        ctx.lineJoin = "round"
        for (var i = 1; i < points.length; i++) {
            var a = points[i - 1], b = points[i]
            if (b[0] - a[0] > 3 * 3600) continue   // gap (service not running)
            ctx.strokeStyle = a[2] ? LomiriColors.green : theme.palette.normal.activity
            ctx.beginPath()
            ctx.moveTo(left + w * (a[0] - start) / span, top + h - h * a[1] / 100)
            ctx.lineTo(left + w * (b[0] - start) / span, top + h - h * b[1] / 100)
            ctx.stroke()
        }
    }
}
