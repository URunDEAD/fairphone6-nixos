/*
 * Model for System Settings > System update (FP6). Backend: fp6-updated
 * (root, system bus, D-Bus activated) through UpdateService.call().
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import Fp6.Update 1.0

QtObject {
    id: model

    property var status: null
    property bool ok: false
    property string error: ""
    readonly property var job: status ? status.job : null
    readonly property bool busy: !!(status && status.busy)
    readonly property var running: status ? status.running : null
    readonly property var available: status ? status.available : null
    readonly property var pending: status ? status.pending : null
    readonly property var settings: status ? status.settings : null
    readonly property var blocked: status ? status.blocked : null

    signal jobFinished(var job)

    property UpdateService service: UpdateService {
        onChanged: model.refresh()
    }

    function call(method, args, cb) {
        service.call(method, JSON.stringify(args || {}), function(err, res) {
            var v = null
            if (!err) {
                try { v = JSON.parse(res) } catch (e) { err = String(e) }
            }
            if (!err && v && v.job !== undefined) model._update(v)
            if (cb) cb(err, v)
        })
    }

    // Every status reply goes through here. A job counts as finished when
    // it was seen running and is not any more, OR when a newer job id shows
    // up already finished (a refused install fails within milliseconds,
    // between two polls).
    property var _lastJob: null
    function _update(v) {
        status = v
        ok = true
        error = ""
        var j = v.job
        if (j && j.id > 0 && j.state !== "running") {
            var seenRunning = _lastJob && _lastJob.id === j.id && _lastJob.state === "running"
            var newer = !_lastJob || j.id > _lastJob.id
            if (seenRunning || (newer && _lastJob !== null))
                jobFinished(j)
        }
        if (j) _lastJob = j
    }

    function refresh() {
        call("status", {}, function(err, v) {
            if (err) { ok = false; error = err }
        })
    }

    // Decimal units, like the stock Storage page.
    function size(bytes) {
        var b = Number(bytes) || 0
        if (b >= 1e9) return (b / 1e9).toFixed(b >= 1e10 ? 0 : 1) + " GB"
        if (b >= 1e6) return Math.round(b / 1e6) + " MB"
        if (b >= 1e3) return Math.round(b / 1e3) + " kB"
        return b + " B"
    }

    function when(t) {
        if (!t) return ""
        return Qt.formatDateTime(new Date(t * 1000), "dd.MM.yyyy hh:mm")
    }

    property Timer timer: Timer {
        interval: model.busy ? 1000 : 5000
        repeat: true
        running: Qt.application.state === Qt.ApplicationActive
        triggeredOnStart: true
        onTriggered: model.refresh()
    }
}
