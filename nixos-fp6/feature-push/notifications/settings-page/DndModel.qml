/*
 * Do Not Disturb pages: GSettings com.fairphone6.notifications plus the
 * fp6-notifyd status file (history, app names seen).
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import GSettings 1.0

QtObject {
    id: model

    property GSettings settings: GSettings {
        schema.id: "com.fairphone6.notifications"
    }

    property var status: null
    property bool statusOk: false

    readonly property var dayNames: [i18n.tr("Mon"), i18n.tr("Tue"), i18n.tr("Wed"), i18n.tr("Thu"), i18n.tr("Fri"), i18n.tr("Sat"), i18n.tr("Sun")]

    readonly property var schedules: {
        try { return JSON.parse(settings.dndSchedules) } catch (e) { return [] }
    }
    function saveSchedules(list) { settings.dndSchedules = JSON.stringify(list) }

    function setManual(on) {
        settings.dndManualSince = Math.floor(Date.now() / 1000)
        settings.dndManual = on ? "on" : "off"
    }
    function setBedtimeManual(on) {
        settings.bedtimeManualSince = Math.floor(Date.now() / 1000)
        settings.bedtimeManual = on ? "on" : "off"
        settings.dndManual = "none"
    }

    function daysText(days) {
        if (!days || days.length === 0) return i18n.tr("Never")
        if (days.length === 7) return i18n.tr("Every day")
        var s = days.slice().sort().join(",")
        if (s === "1,2,3,4,5") return i18n.tr("Weekdays")
        if (s === "6,7") return i18n.tr("Weekends")
        var out = []
        for (var i = 0; i < days.length; i++) out.push(dayNames[days[i] - 1])
        return out.join(", ")
    }
    function windowText(w) {
        return i18n.tr("%1, %2 – %3").arg(daysText(w.days)).arg(w.start).arg(w.end)
    }

    function reasonText() {
        var r = settings.dndReason
        if (!settings.dndActive) return i18n.tr("Off")
        if (r === "manual") return i18n.tr("On")
        if (r === "bedtime") return i18n.tr("On (bedtime)")
        if (r.indexOf("schedule:") === 0) return i18n.tr("On (%1)").arg(r.substring(9))
        return i18n.tr("On")
    }

    function listWith(list, item, on) {
        var out = []
        for (var i = 0; i < list.length; i++) if (list[i] !== item) out.push(list[i])
        if (on) out.push(item)
        return out
    }

    function hhmm(date) { return Qt.formatTime(date, "hh:mm") }
    function dateFor(text) {
        var p = (text || "00:00").split(":")
        var d = new Date()
        d.setHours(parseInt(p[0]) || 0, parseInt(p[1]) || 0, 0, 0)
        return d
    }

    function refresh() {
        var path = settings.statusFile
        if (!path) { statusOk = false; return }
        var xhr = new XMLHttpRequest
        xhr.open("GET", "file://" + path)
        xhr.onreadystatechange = function() {
            if (xhr.readyState === XMLHttpRequest.DONE) {
                try {
                    status = JSON.parse(xhr.responseText)
                    statusOk = true
                } catch (e) {
                    statusOk = false
                }
            }
        }
        xhr.send()
    }

    property Timer timer: Timer {
        interval: 3000
        repeat: true
        running: Qt.application.state === Qt.ApplicationActive
        triggeredOnStart: true
        onTriggered: model.refresh()
    }
}
