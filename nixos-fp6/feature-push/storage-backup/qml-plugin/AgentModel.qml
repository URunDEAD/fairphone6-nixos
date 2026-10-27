/*
 * Shared model for System Settings > Storage and > Backup (FP6).
 * Backend: fp6-storage-agent (session bus, D-Bus activated) through
 * StorageAgent.call(); it relays the privileged parts to fp6-storaged.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import Fp6.StorageBackup 1.0

QtObject {
    id: model

    property var status: null
    property bool ok: false
    property string error: ""
    readonly property var job: status ? status.job : null
    readonly property bool busy: !!(job && job.state === "running")
    readonly property var storage: status ? status.storage : null
    readonly property var backup: status ? status.backup : null

    signal jobFinished(var job)

    property StorageAgent agent: StorageAgent {
        onChanged: model.refresh()
    }

    function call(method, args, cb) {
        agent.call(method, JSON.stringify(args || {}), function(err, res) {
            var v = null
            if (!err) {
                try { v = JSON.parse(res) } catch (e) { err = String(e) }
            }
            if (cb) cb(err, v)
        })
    }

    property var _lastJob: null
    function refresh() {
        call("status", {}, function(err, v) {
            if (err) { ok = false; error = err; return }
            status = v
            ok = true
            error = ""
            var j = v.job
            if (j && _lastJob && _lastJob.id === j.id && _lastJob.state === "running" && j.state !== "running")
                jobFinished(j)
            _lastJob = j
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
