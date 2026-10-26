/*
 * Security & Privacy > Private Wi-Fi address (FP6 privacy-lock).
 * Scans always use random addresses (NetworkManager). Connections: a stable
 * random address per network by default; per saved network: stable, a new
 * one for each connection, or the device address. Backend: fp6-privacyd
 * (nmcli), list in wifi-networks, choices in wifi-mac-modes.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.4
import SystemSettings 1.0
import SystemSettings.ListItems 1.0 as SettingsListItems
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItems

ItemPage {
    id: root
    objectName: "fp6WifiMacPage"
    title: i18n.tr("Private Wi-Fi address")
    flickable: flick

    property var settings
    readonly property var modes: ["default", "stable", "random", "device"]
    readonly property var modeNames: [i18n.tr("Private, stable for this network"), i18n.tr("Private, stable for this network"), i18n.tr("Private, new for each connection"), i18n.tr("Device address")]

    readonly property var networks: {
        var out = [];
        var list = root.settings ? (root.settings.wifiNetworks || []) : [];
        for (var i = 0; i < list.length; i++) {
            var p = String(list[i]).split("\t");
            if (p.length >= 3) out.push({ uuid: p[0], name: p[1], mode: p[2] });
        }
        return out;
    }

    function setMode(uuid, mode) {
        var list = (root.settings.wifiMacModes || []).filter(function (l) { return String(l).split("\t")[0] !== uuid; });
        list.push(uuid + "\t" + mode);
        root.settings.wifiMacModes = list;
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: column.height + units.gu(4)
        Column {
            id: column
            anchors { left: parent.left; right: parent.right }

            SettingsListItems.Standard {
                text: i18n.tr("Use private addresses")
                Switch {
                    objectName: "fp6WifiMacSwitch"
                    property bool serverChecked: root.settings && root.settings.wifiMacRandomization !== false
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: root.settings.wifiMacRandomization = checked
                }
            }
            ListItems.Caption {
                text: i18n.tr("Searching for networks always uses random addresses. A private address keeps networks from following this phone between places; some networks (MAC filters, fixed DHCP addresses) need the device address. Changes apply the next time the phone connects to that network.")
            }

            SettingsItemTitle {
                text: i18n.tr("Saved networks")
                visible: root.networks.length > 0
            }

            Repeater {
                model: root.settings && root.settings.wifiMacRandomization !== false ? root.networks : []
                delegate: ListItems.ItemSelector {
                    text: modelData.name
                    model: [root.modeNames[1], root.modeNames[2], root.modeNames[3]]
                    selectedIndex: modelData.mode === "random" ? 1 : modelData.mode === "device" ? 2 : 0
                    onDelegateClicked: root.setMode(modelData.uuid, ["stable", "random", "device"][index])
                }
            }
            ListItems.Caption {
                visible: root.networks.length > 0
                text: i18n.tr("Networks saved before private addresses were turned on keep the device address until you change them here.")
            }
        }
    }
}
