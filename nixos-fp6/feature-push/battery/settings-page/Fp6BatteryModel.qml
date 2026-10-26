/*
 * Shared data for the FP6 additions to System Settings > Battery.
 * Backend: fp6-batteryd (system service + session agent). Settings in
 * GSettings com.fairphone6.battery; computed state in
 * /run/fp6-battery/status.json (every 60 s plugged in, 300 s on battery)
 * and history.json; live readings straight from power_supply sysfs while
 * a page is visible.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import GSettings 1.0

QtObject {
    id: model

    property GSettings settings: GSettings { schema.id: "com.fairphone6.battery" }

    property var status: null
    property bool statusOk: false
    property var history: null
    property bool live: true
    property bool wantHistory: false

    // Live readings (sysfs, µA/µV)
    property real liveCurrent: 0
    property real liveVoltage: 0
    property real usbCurrent: 0
    property real usbVoltage: 0
    property bool usbOnline: false
    property int liveLevel: -1

    readonly property string bat: "/sys/class/power_supply/qcom-battmgr-bat/"
    readonly property string usb: "/sys/class/power_supply/qcom-battmgr-usb/"

    readonly property var charging: status ? status.charging : null
    readonly property var limit: status ? status.limit : null
    readonly property var saver: status ? status.saver : null
    readonly property var health: status ? status.health : null

    readonly property bool plugged: usbOnline || (charging ? charging.plugged : false)
    // Battery charge power in W (current > 0 = into the battery).
    readonly property real chargeWatts: liveCurrent > 0 ? liveCurrent * liveVoltage / 1e12 : 0
    readonly property real inputWatts: usbOnline ? Math.max(0, usbCurrent) * Math.max(0, usbVoltage) / 1e12 : 0

    function read(path, cb) {
        var xhr = new XMLHttpRequest
        xhr.open("GET", "file://" + path)
        xhr.onreadystatechange = function() {
            if (xhr.readyState === XMLHttpRequest.DONE)
                cb(xhr.responseText)
        }
        xhr.send()
    }

    function refreshLive() {
        read(bat + "current_now", function(t) { var v = parseInt(t); if (!isNaN(v)) liveCurrent = v })
        read(bat + "voltage_now", function(t) { var v = parseInt(t); if (!isNaN(v)) liveVoltage = v })
        read(bat + "capacity", function(t) { var v = parseInt(t); if (!isNaN(v)) liveLevel = v })
        read(usb + "online", function(t) { usbOnline = parseInt(t) === 1 })
        read(usb + "current_now", function(t) { var v = parseInt(t); if (!isNaN(v)) usbCurrent = v })
        read(usb + "voltage_now", function(t) { var v = parseInt(t); if (!isNaN(v)) usbVoltage = v })
    }

    function refresh() {
        read(settings.statusFile || "/run/fp6-battery/status.json", function(t) {
            try {
                status = JSON.parse(t)
                statusOk = (Date.now() / 1000 - status.time) < 900
            } catch (e) {
                statusOk = false
            }
        })
        if (wantHistory)
            read(settings.historyFile || "/run/fp6-battery/history.json", function(t) {
                try { history = JSON.parse(t) } catch (e) { }
            })
    }

    function duration(s) {
        var m = Math.round(s / 60)
        if (m < 60) return i18n.tr("%1 min").arg(m)
        var h = Math.floor(m / 60)
        return m % 60 ? i18n.tr("%1 h %2 min").arg(h).arg(m % 60) : i18n.tr("%1 h").arg(h)
    }

    function clock(t) {
        return Qt.formatTime(new Date(t * 1000), "hh:mm")
    }

    function watts(w) {
        return w >= 10 ? Math.round(w) + " W" : w.toFixed(1) + " W"
    }

    // One line for the charging state (Battery page, indicator has its own).
    function chargingText() {
        if (!plugged) return ""
        var c = charging
        var who = c && c.charger ? c.charger : i18n.tr("USB")
        if (chargeWatts > 0.05)
            return i18n.tr("%1 into the battery (%2, %3 from the charger)")
                .arg(watts(chargeWatts)).arg(who).arg(watts(inputWatts))
        return i18n.tr("Not charging (%1, %2 from the charger runs the phone)").arg(who).arg(watts(inputWatts))
    }

    function fullText() {
        if (!plugged || !status) return ""
        var l = limit, c = charging
        if (l && l.mode === "hold")
            return i18n.tr("Adaptive charging: holding at %1 %, 100 % by %2 (alarm %3)")
                .arg(c.target).arg(clock(l.holdUntil + 0)).arg(clock(l.modeAlarm))
        var lvl = liveLevel >= 0 ? liveLevel : status.level
        if (c && c.target < 100 && lvl >= c.target)
            return i18n.tr("Charge limit %1 % reached").arg(c.target)
        if (c && c.timeToFull > 0)
            return i18n.tr("%1 to %2 % (about %3)").arg(duration(c.timeToFull)).arg(c.target)
                .arg(clock(Date.now() / 1000 + c.timeToFull))
        if (lvl >= 100) return i18n.tr("Fully charged")
        return ""
    }

    property Timer slow: Timer {
        interval: 10000; repeat: true; triggeredOnStart: true
        running: model.live && Qt.application.state === Qt.ApplicationActive
        onTriggered: model.refresh()
    }
    property Timer fast: Timer {
        interval: 3000; repeat: true; triggeredOnStart: true
        running: model.live && Qt.application.state === Qt.ApplicationActive
        onTriggered: model.refreshLive()
    }
}
