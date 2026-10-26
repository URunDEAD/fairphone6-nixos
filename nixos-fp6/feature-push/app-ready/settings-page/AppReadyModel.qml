/*
 * Shared helpers for System Settings > App Launch (FP6).
 * Backend: fp6-app-ready (user service), GSettings com.fairphone6.app-ready,
 * status JSON in $XDG_RUNTIME_DIR/fp6-app-ready/status.json.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import GSettings 1.0

QtObject {
    id: model

    property GSettings settings: GSettings {
        schema.id: "com.fairphone6.app-ready"
    }

    property var status: null
    property bool statusOk: false

    // "id TAB name TAB icon" entries kept by fp6-app-ready.
    readonly property var apps: {
        var list = settings.installedApps || []
        var out = []
        for (var i = 0; i < list.length; i++) {
            var p = String(list[i]).split("\t")
            if (p.length >= 2) out.push({ id: p[0], name: p[1], icon: p.length > 2 ? p[2] : "" })
        }
        return out
    }

    // Apps with any option on, in a stable order.
    readonly property var chosen: {
        var out = []
        var lists = [settings.keepReady || [], settings.prelaunch || [], settings.preload || []]
        for (var l = 0; l < lists.length; l++)
            for (var i = 0; i < lists[l].length; i++)
                if (out.indexOf(lists[l][i]) < 0) out.push(lists[l][i])
        return out
    }

    function app(id) {
        for (var i = 0; i < apps.length; i++)
            if (apps[i].id === id) return apps[i]
        return { id: id, name: id, icon: "" }
    }

    function iconSource(icon) {
        if (!icon) return "image://theme/application-default-icon"
        if (icon.indexOf("/") === 0) return "file://" + icon
        return "image://theme/" + icon
    }

    function appStatus(id) {
        if (!status || !status.apps) return null
        for (var i = 0; i < status.apps.length; i++)
            if (status.apps[i].id === id) return status.apps[i]
        return null
    }

    function has(key, id) {
        var l = settings[key] || []
        return l.indexOf(id) >= 0
    }

    function set(key, id, on) {
        var l = (settings[key] || []).slice()
        var i = l.indexOf(id)
        if (on && i < 0) l.push(id)
        if (!on && i >= 0) l.splice(i, 1)
        settings[key] = l
    }

    function remove(id) {
        set("keepReady", id, false)
        set("prelaunch", id, false)
        set("preload", id, false)
    }

    function mb(bytes) {
        var v = bytes / 1048576
        return v >= 1024 ? (v / 1024).toFixed(1) + " GB" : Math.round(v) + " MB"
    }

    function stateText(s) {
        if (!s || !s.running) return i18n.tr("Not running")
        if (s.focused) return i18n.tr("On screen")
        if (s.frozen) return i18n.tr("Ready (suspended)")
        return i18n.tr("Running in the background")
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
                    statusOk = (Date.now() / 1000 - status.time) < 120
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

    Component.onCompleted: settings.appListRequest = (settings.appListRequest + 1) % 1000000
}
