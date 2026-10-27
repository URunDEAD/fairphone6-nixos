/*
 * Settings > VPN > Reconnect at startup (FP6 persistence module).
 * NetworkManager never reconnects a VPN by itself after a restart;
 * fp6-vpn-restore (user session agent) does, per VPN:
 *   restore (default)  reconnect if it was on when the phone went off;
 *   always             connect at startup, reconnect when it drops;
 *   never              stay off after a restart.
 * List from com.fairphone6.vpn connections ("uuid\tname\tstate", written by
 * the agent), choices in startup-modes ("uuid\tmode").
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.4
import GSettings 1.0
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItems

Column {
    id: root
    objectName: "fp6VpnStartup"

    readonly property var modes: ["restore", "always", "never"]
    readonly property var modeNames: [i18n.tr("Restore last state"), i18n.tr("Always on"), i18n.tr("Off")]

    GSettings {
        id: vpnSettings
        schema.id: "com.fairphone6.vpn"
    }

    readonly property var connections: {
        var out = [];
        var list = vpnSettings.connections || [];
        var chosen = {};
        var m = vpnSettings.startupModes || [];
        for (var j = 0; j < m.length; j++) {
            var q = String(m[j]).split("\t");
            if (q.length >= 2) chosen[q[0]] = q[1];
        }
        for (var i = 0; i < list.length; i++) {
            var p = String(list[i]).split("\t");
            if (p.length >= 3) out.push({ uuid: p[0], name: p[1], state: p[2], mode: chosen[p[0]] || "restore" });
        }
        return out;
    }

    function setMode(uuid, mode) {
        var list = (vpnSettings.startupModes || []).filter(function (l) { return String(l).split("\t")[0] !== uuid; });
        // "restore" is the default: keep the list short.
        if (mode !== "restore") list.push(uuid + "\t" + mode);
        vpnSettings.startupModes = list;
    }

    function stateText(s) {
        return s === "on" ? i18n.tr("Connected")
             : s === "connecting" ? i18n.tr("Connecting…")
             : s === "waiting" ? i18n.tr("Reconnects when a network is available")
             : i18n.tr("Off");
    }

    visible: connections.length > 0

    SettingsItemTitle {
        text: i18n.tr("Reconnect at startup")
    }

    Repeater {
        objectName: "fp6VpnStartupList"
        model: root.connections
        delegate: ListItems.ItemSelector {
            objectName: "fp6VpnStartup_" + modelData.name
            text: modelData.name + " · " + root.stateText(modelData.state)
            model: root.modeNames
            // ItemSelector resets selectedIndex when its model is set: bind afterwards.
            Component.onCompleted: selectedIndex = Qt.binding(function () { return Math.max(0, root.modes.indexOf(modelData.mode)); })
            onDelegateClicked: root.setMode(modelData.uuid, root.modes[index])
        }
    }

    ListItems.Caption {
        text: i18n.tr("Restore last state: a VPN that was on when the phone was switched off or restarted connects again by itself. Always on: connects at every start and reconnects whenever it drops; switching it off keeps it off until you switch it on or restart. Off: stays off after a restart.")
    }
}
