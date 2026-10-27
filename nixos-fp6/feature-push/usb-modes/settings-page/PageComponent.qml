/*
 * System Settings > USB (FP6): what the computer on the USB cable may use
 * (combinable: USB network, file transfer, USB drive, USB debugging), who
 * charges whom (USB-C power role) and what a new connection gets.
 * Backend: fp6-usbmoded (root) via the session agent fp6-usbmodes-agent:
 * this page writes GSettings com.fairphone6.usb-modes (preferences and a
 * one-shot `request` key) and reads /run/fp6-usb-modes/status.json.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import GSettings 1.0
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: root
    objectName: "usbModesPage"
    title: i18n.tr("USB")
    flickable: flick

    property string statusUrl: "file:///run/fp6-usb-modes/status.json"
    property var st: null
    property bool waiting: false
    property string waitingFor: ""

    GSettings { id: gs; schema.id: "com.fairphone6.usb-modes" }

    function list(v) { var o = []; if (v) for (var i = 0; i < v.length; i++) o.push(String(v[i])); return o; }
    readonly property var functions: st ? list(st.functions) : []
    readonly property var keep: st ? list(st.keep) : []
    readonly property var defaults: st ? list(st.defaults) : []
    readonly property var power: st ? st.power : ({})
    readonly property bool connected: !!st && st.cable && (st.peer === "host" || st.peer === "device")
    function has(f) { return functions.indexOf(f) >= 0; }
    function available(f) { return !!st && !!st.available && !!st.available[f]; }

    function reload() {
        var xhr = new XMLHttpRequest;
        xhr.open("GET", statusUrl);
        xhr.onreadystatechange = function() {
            if (xhr.readyState !== XMLHttpRequest.DONE) return;
            try {
                var s = JSON.parse(xhr.responseText);
                if (root.waiting && root.st && JSON.stringify(s.functions) + s.power.role + s.lastError + s.lastChange
                        !== root.waitingFor) root.waiting = false;
                root.st = s;
            } catch (e) { root.st = null; }
        };
        xhr.send();
    }
    Timer { interval: 1000; repeat: true; running: root.visible; triggeredOnStart: true; onTriggered: root.reload() }
    Timer { id: waitTimeout; interval: 30000; onTriggered: root.waiting = false }

    function request(r) {
        if (st) waitingFor = JSON.stringify(st.functions) + st.power.role + st.lastError + st.lastChange;
        waiting = true;
        waitTimeout.restart();
        gs.request = r + "@" + Date.now();
    }
    function setFunction(f, on) {
        var s = functions.filter(function(x) { return x !== f; });
        if (on) s.push(f);
        request("functions:" + s.join(","));
    }
    function chargeOnly() { request("functions:" + keep.filter(function(x) { return x !== "mass_storage"; }).join(",")); }

    function headline() {
        if (!st) return i18n.tr("USB service not running");
        if (power.role === "source" && st.cable) return i18n.tr("Charging the connected device");
        if (st.peer === "host") return i18n.tr("Connected to a computer");
        if (st.peer === "charger") return i18n.tr("Connected to a charger");
        return i18n.tr("Not connected");
    }
    function detail() {
        if (!st || !st.cable) return i18n.tr("Connect a USB cable to choose how the connection is used.");
        var parts = [];
        if (power.role === "sink" && power.online) {
            var w = Math.max(0, power.currentUa) * Math.max(0, power.voltageUv) / 1e12;
            parts.push(i18n.tr("Charging this phone") + (w > 0.05 ? " · " + w.toFixed(1) + " W" : ""));
        }
        if (power.usbType) parts.push(power.usbType === "SDP" ? i18n.tr("computer USB port")
                                      : power.usbType === "CDP" ? i18n.tr("charging USB port")
                                      : power.usbType === "DCP" ? i18n.tr("wall charger")
                                      : power.usbType.indexOf("PD") === 0 ? i18n.tr("USB Power Delivery") : power.usbType);
        if (st.peer === "charger" && !st.udcBound)
            parts.push(i18n.tr("a computer is recognized once USB data is on"));
        return parts.join(" · ");
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        boundsBehavior: (contentHeight > root.height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick

        Column {
            id: content
            anchors.left: parent.left
            anchors.right: parent.right

            SettingsListItems.Standard {
                objectName: "statusItem"
                highlightWhenPressed: false
                layout.title.text: root.headline()
                layout.title.font.weight: Font.Medium
                layout.subtitle.text: root.detail()
                layout.subtitle.maximumLineCount: 3
                layout.subtitle.wrapMode: Text.WordWrap
                Icon {
                    SlotsLayout.position: SlotsLayout.Leading
                    width: units.gu(4); height: width
                    name: root.power.role === "source" ? "battery-full-charging-symbolic" : "stock_usb"
                    color: root.connected ? theme.palette.normal.activity : theme.palette.normal.backgroundSecondaryText
                }
                ActivityIndicator { running: root.waiting || (root.st && root.st.busy); visible: running }
            }

            Label {
                objectName: "errorLabel"
                visible: !!root.st && !!root.st.lastError
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: theme.palette.normal.negative
                text: root.st ? i18n.tr("Last change failed: %1").arg(root.st.lastError) : ""
            }
            Label {
                visible: !!root.st && !!root.st.pendingUnlock
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Waiting for unlock to turn on the last used functions.")
            }

            SettingsItemTitle { text: i18n.tr("Use USB for") }
            SettingsListItems.Standard {
                objectName: "chargeOnlyItem"
                text: i18n.tr("Charging only (no data)")
                layout.subtitle.text: root.keep.indexOf("ncm") >= 0
                    ? i18n.tr("USB network stays on in this build") : i18n.tr("The computer sees no USB device")
                enabled: !!root.st && !root.st.driveExclusive
                onClicked: root.chargeOnly()
                Icon {
                    width: units.gu(2.5); height: width
                    name: "tick"
                    color: theme.palette.normal.positive
                    visible: root.functions.filter(function(f) { return root.keep.indexOf(f) < 0; }).length === 0
                }
            }
            SettingsListItems.Standard {
                objectName: "ncmItem"
                text: i18n.tr("USB network")
                layout.subtitle.text: root.keep.indexOf("ncm") >= 0
                    ? i18n.tr("Always on in this build: developer access (SSH 172.16.42.1)")
                    : i18n.tr("Network to the computer (172.16.42.1; SSH, sharing the connection)")
                layout.subtitle.maximumLineCount: 2
                layout.subtitle.wrapMode: Text.WordWrap
                Switch {
                    objectName: "ncmSwitch"
                    checked: root.has("ncm")
                    enabled: !!root.st && root.keep.indexOf("ncm") < 0 && !root.st.driveExclusive
                    onTriggered: root.setFunction("ncm", checked)
                }
            }
            SettingsListItems.Standard {
                objectName: "mtpItem"
                visible: root.available("mtp")
                text: i18n.tr("File transfer")
                layout.subtitle.text: i18n.tr("MTP: Documents, Downloads, Music, Pictures, Videos")
                Switch {
                    objectName: "mtpSwitch"
                    checked: root.has("mtp")
                    enabled: !!root.st && !root.st.driveExclusive
                    onTriggered: root.setFunction("mtp", checked)
                }
            }
            SettingsListItems.Standard {
                objectName: "driveItem"
                text: i18n.tr("USB drive")
                layout.subtitle.text: root.has("mass_storage")
                    ? (root.st.driveExclusive ? i18n.tr("Sharing an image (exclusive: the other functions are off)") : i18n.tr("Sharing an image"))
                    : i18n.tr("Share an ISO or disk image (opens USB Drive)")
                layout.subtitle.maximumLineCount: 2
                layout.subtitle.wrapMode: Text.WordWrap
                Switch {
                    objectName: "driveSwitch"
                    checked: root.has("mass_storage")
                    enabled: !!root.st
                    onTriggered: root.setFunction("mass_storage", checked)
                }
            }
            SettingsListItems.Standard {
                objectName: "adbItem"
                visible: root.available("adb") || root.has("adb")
                text: i18n.tr("USB debugging (ADB)")
                layout.subtitle.text: i18n.tr("Developer mode; for this connection only")
                Switch {
                    objectName: "adbSwitch"
                    checked: root.has("adb")
                    enabled: !!root.st && !root.st.driveExclusive
                    onTriggered: root.setFunction("adb", checked)
                }
            }

            SettingsItemTitle { text: i18n.tr("Power") }
            SettingsListItems.Standard {
                objectName: "sinkItem"
                text: i18n.tr("Charge this phone")
                enabled: !!root.st
                onClicked: if (root.power.role === "source") root.request("power:sink")
                Icon { width: units.gu(2.5); height: width; name: "tick"; color: theme.palette.normal.positive
                       visible: root.power.role !== "source" }
            }
            SettingsListItems.Standard {
                objectName: "sourceItem"
                text: i18n.tr("Charge the connected device")
                enabled: !!root.power.canSwap || root.power.role === "source"
                layout.subtitle.text: root.power.role === "source" ? i18n.tr("This phone powers the device on the cable")
                    : root.power.canSwap ? i18n.tr("Swap power roles over USB Power Delivery")
                    : i18n.tr("Not available: %1").arg(root.power.swapReason || i18n.tr("nothing connected"))
                layout.subtitle.maximumLineCount: 3
                layout.subtitle.wrapMode: Text.WordWrap
                onClicked: if (root.power.role !== "source") root.request("power:source")
                Icon { width: units.gu(2.5); height: width; name: "tick"; color: theme.palette.normal.positive
                       visible: root.power.role === "source" }
            }
            SettingsListItems.Standard {
                objectName: "noChargeItem"
                text: i18n.tr("Don't charge this phone")
                enabled: !!root.power.canStopCharging
                layout.subtitle.text: i18n.tr("Not supported: %1").arg(root.power.stopChargingReason || "")
                layout.subtitle.maximumLineCount: 3
                layout.subtitle.wrapMode: Text.WordWrap
            }

            SettingsItemTitle { text: i18n.tr("When a computer is connected") }
            ListItem.ItemSelector {
                objectName: "defaultSelector"
                expanded: true
                readonly property var modes: ["system", "charge", "last", "ask"]
                model: [root.defaults.indexOf("ncm") >= 0 ? i18n.tr("USB network (default)") : i18n.tr("Default of this phone (charging only)"),
                        i18n.tr("Charging only"), i18n.tr("Last used"), i18n.tr("Ask every time")]
                selectedIndex: Math.max(0, modes.indexOf(gs.defaultWhenConnected))
                onDelegateClicked: gs.defaultWhenConnected = modes[index]
            }
            Label {
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Data is only turned on while the phone is unlocked. Unplugging returns to this choice.")
            }
            SettingsListItems.Standard {
                objectName: "rememberItem"
                text: i18n.tr("Remember per computer")
                layout.subtitle.text: root.st && root.st.hostRecognizable
                    ? i18n.tr("This computer identifies itself")
                    : i18n.tr("Only for computers that identify themselves over USB Power Delivery")
                layout.subtitle.maximumLineCount: 2
                layout.subtitle.wrapMode: Text.WordWrap
                Switch {
                    objectName: "rememberSwitch"
                    checked: gs.rememberHosts
                    onTriggered: gs.rememberHosts = checked
                }
            }
            SettingsListItems.Standard {
                objectName: "notifyItem"
                text: i18n.tr("Notify when connected")
                Switch {
                    objectName: "notifySwitch"
                    checked: gs.notifyOnConnect
                    onTriggered: gs.notifyOnConnect = checked
                }
            }

            Item { width: 1; height: units.gu(2) }
            Label {
                objectName: "portLabel"
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "x-small"
                color: theme.palette.normal.backgroundSecondaryText
                text: root.st ? i18n.tr("USB-C port: %1, USB Power Delivery %2, data role %3.")
                        .arg(root.power.drp ? i18n.tr("dual-role") : i18n.tr("single-role"))
                        .arg(root.power.pdRevision || "?").arg(root.power.dataRole || "?") : ""
            }
        }
    }
}
