/*
 * System Settings > Quick settings (FP6): the control centre in the
 * pulled-down status bar. Same settings as its edit mode (pencil button in
 * the panel): tiles and their order, small/wide, tiles per row, rows before
 * expanding, sliders, what pulling down opens, and reset.
 * Backend: GSettings com.fairphone6.quick-settings (read live by Lomiri).
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
    objectName: "quickSettingsPage"
    title: i18n.tr("Quick settings")
    flickable: flick

    GSettings { id: gs; schema.id: "com.fairphone6.quick-settings" }

    // id -> [label, icon, can be wide]; keep in sync with QsBackend.qml
    readonly property var catalogue: ({
        "wifi": [i18n.tr("Wi-Fi"), "network-wifi-symbolic", true],
        "bluetooth": [i18n.tr("Bluetooth"), "bluetooth-active", true],
        "mobile-data": [i18n.tr("Mobile data"), "network-cellular-lte", true],
        "flashlight": [i18n.tr("Flashlight"), "torch-on", false],
        "dnd": [i18n.tr("Do Not Disturb"), "night-mode", true],
        "rotation-lock": [i18n.tr("Rotation lock"), "orientation-lock", false],
        "airplane": [i18n.tr("Airplane mode"), "airplane-mode", true],
        "location": [i18n.tr("Location"), "location-active", true],
        "battery-saver": [i18n.tr("Battery saver"), "battery-caution-symbolic", true],
        "night-light": [i18n.tr("Night light"), "weather-clear-night-symbolic", true],
        "hotspot": [i18n.tr("Hotspot"), "hotspot-active", true],
        "screen-record": [i18n.tr("Screen recording"), "media-record", true],
        "dark-theme": [i18n.tr("Dark theme"), "preferences-desktop-wallpaper-symbolic", true],
        "silent": [i18n.tr("Silent mode"), "audio-volume-muted", true],
        "screenshot": [i18n.tr("Screenshot"), "camera-photo-symbolic", false],
        "bedtime": [i18n.tr("Bedtime"), "alarm-clock", true],
        "stay-awake": [i18n.tr("Stay awake"), "display-brightness-max", false],
        "greyscale": [i18n.tr("Greyscale"), "preferences-color-symbolic", false],
        "scan-qr": [i18n.tr("Scan QR code"), "camera-grid", false],
        "battery": [i18n.tr("Battery"), "battery-full-symbolic", true],
        "sound": [i18n.tr("Sound"), "audio-volume-high", true],
        "time-date": [i18n.tr("Time and date"), "preferences-system-time-symbolic", true],
        "messages": [i18n.tr("Messages"), "notification", true],
        "system": [i18n.tr("System"), "system-shutdown", true],
        "privacy": [i18n.tr("Privacy"), "preferences-system-privacy-symbolic", true],
        "usb-drive": [i18n.tr("USB drive"), "save-to", true],
        "keyboard": [i18n.tr("Keyboard"), "preferences-desktop-keyboard-shortcuts-symbolic", true]
    })
    readonly property var order: ["wifi", "bluetooth", "mobile-data", "flashlight", "dnd", "rotation-lock",
        "airplane", "location", "battery-saver", "night-light", "hotspot", "screen-record", "dark-theme",
        "silent", "screenshot", "battery", "sound", "time-date", "messages", "system", "privacy",
        "usb-drive", "keyboard", "bedtime", "stay-awake", "greyscale", "scan-qr"]

    function list(v) { var o = []; if (v) for (var i = 0; i < v.length; i++) o.push(String(v[i])); return o; }
    readonly property var tiles: list(gs.tiles)
    readonly property var wide: list(gs.wideTiles)
    readonly property var sliders: list(gs.sliders)
    readonly property var unused: order.filter(function(id) { return tiles.indexOf(id) < 0; })

    function moveTile(id, delta) {
        var t = tiles.slice(), i = t.indexOf(id);
        if (i < 0 || i + delta < 0 || i + delta >= t.length) return;
        t.splice(i, 1); t.splice(i + delta, 0, id);
        gs.tiles = t;
    }
    function removeTile(id) { gs.tiles = tiles.filter(function(x) { return x !== id; }); }
    function addTile(id) { gs.tiles = tiles.concat([id]); }
    function setWide(id, on) {
        var w = wide.filter(function(x) { return x !== id; });
        if (on) w.push(id);
        gs.wideTiles = w;
    }
    function sliderName(id) {
        return id === "brightness" ? i18n.tr("Brightness") : id === "volume" ? i18n.tr("Volume") : i18n.tr("Media controls");
    }
    function setSlider(id, on) {
        var all = sliders.slice();
        ["brightness", "volume", "media"].forEach(function(s) { if (all.indexOf(s) < 0) all.push(s); });
        gs.sliders = all.filter(function(s) { return s === id ? on : sliders.indexOf(s) >= 0; });
    }
    function moveSlider(id, delta) {
        var s = sliders.slice(), i = s.indexOf(id);
        if (i < 0 || i + delta < 0 || i + delta >= s.length) return;
        s.splice(i, 1); s.splice(i + delta, 0, id);
        gs.sliders = s;
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

            Item { width: 1; height: units.gu(2) }
            Label {
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Pull the status bar down to open quick settings. Tap a tile to switch it, hold it for its details (the indicator menu or the Settings page). The pencil button in the panel edits the tiles by dragging.")
            }

            SettingsItemTitle { text: i18n.tr("Pulling down opens") }
            ListItem.ItemSelector {
                id: pullDown
                objectName: "pullDownSelector"
                expanded: true
                model: [i18n.tr("Quick settings"), i18n.tr("The status icon under the finger")]
                selectedIndex: gs.pullDownOpens === "indicator" ? 1 : 0
                onDelegateClicked: gs.pullDownOpens = index === 1 ? "indicator" : "control-centre"
            }
            SettingsListItems.Standard {
                text: i18n.tr("Show notifications below the tiles")
                Switch {
                    objectName: "showNotificationsSwitch"
                    checked: gs.showNotifications
                    onTriggered: gs.showNotifications = checked
                }
            }

            SettingsItemTitle { text: i18n.tr("Layout") }
            ListItem.ItemSelector {
                objectName: "columnsSelector"
                text: i18n.tr("Tiles per row")
                model: ["4", "5", "6"]
                selectedIndex: Math.max(0, (gs.columns || 4) - 4)
                onDelegateClicked: gs.columns = index + 4
            }
            ListItem.ItemSelector {
                objectName: "rowsSelector"
                text: i18n.tr("Rows shown before expanding")
                model: ["1", "2", "3"]
                selectedIndex: Math.max(0, (gs.collapsedRows || 2) - 1)
                onDelegateClicked: gs.collapsedRows = index + 1
            }

            SettingsItemTitle { text: i18n.tr("Tiles") }
            Repeater {
                model: root.tiles
                delegate: SettingsListItems.Icon {
                    objectName: "tile-" + modelData
                    readonly property var info: root.catalogue[modelData] || [modelData, "", false]
                    text: info[0]
                    iconName: info[1]
                    Row {
                        spacing: units.gu(0.5)
                        AbstractButton {
                            width: units.gu(4); height: units.gu(4)
                            enabled: index > 0
                            opacity: enabled ? 1 : 0.3
                            onClicked: root.moveTile(modelData, -1)
                            Icon { anchors.centerIn: parent; name: "go-up"; width: units.gu(2); height: width }
                        }
                        AbstractButton {
                            width: units.gu(4); height: units.gu(4)
                            enabled: index < root.tiles.length - 1
                            opacity: enabled ? 1 : 0.3
                            onClicked: root.moveTile(modelData, 1)
                            Icon { anchors.centerIn: parent; name: "go-down"; width: units.gu(2); height: width }
                        }
                        AbstractButton {
                            objectName: "wide-" + modelData
                            width: units.gu(5.5); height: units.gu(4)
                            visible: info[2]
                            onClicked: root.setWide(modelData, root.wide.indexOf(modelData) < 0)
                            Label {
                                anchors.centerIn: parent
                                text: root.wide.indexOf(modelData) >= 0 ? i18n.tr("Wide") : i18n.tr("Small")
                                fontSize: "small"
                                color: theme.palette.normal.activity
                            }
                        }
                        AbstractButton {
                            objectName: "remove-" + modelData
                            width: units.gu(4); height: units.gu(4)
                            onClicked: root.removeTile(modelData)
                            Icon { anchors.centerIn: parent; name: "remove"; width: units.gu(2); height: width; color: theme.palette.normal.negative }
                        }
                    }
                }
            }

            SettingsItemTitle { text: i18n.tr("More tiles"); visible: root.unused.length > 0 }
            Repeater {
                model: root.unused
                delegate: SettingsListItems.Icon {
                    objectName: "unused-" + modelData
                    text: (root.catalogue[modelData] || [modelData])[0]
                    iconName: (root.catalogue[modelData] || ["", ""])[1]
                    AbstractButton {
                        objectName: "add-" + modelData
                        width: units.gu(4); height: units.gu(4)
                        onClicked: root.addTile(modelData)
                        Icon { anchors.centerIn: parent; name: "add"; width: units.gu(2); height: width; color: theme.palette.normal.positive }
                    }
                }
            }

            SettingsItemTitle { text: i18n.tr("Sliders") }
            Repeater {
                model: {
                    var all = root.sliders.slice();
                    ["brightness", "volume", "media"].forEach(function(s) { if (all.indexOf(s) < 0) all.push(s); });
                    return all;
                }
                delegate: SettingsListItems.Standard {
                    readonly property int pos: root.sliders.indexOf(modelData)
                    text: root.sliderName(modelData)
                    Row {
                        spacing: units.gu(0.5)
                        AbstractButton {
                            width: units.gu(4); height: units.gu(4)
                            enabled: pos > 0
                            opacity: enabled ? 1 : 0.3
                            onClicked: root.moveSlider(modelData, -1)
                            Icon { anchors.centerIn: parent; name: "go-up"; width: units.gu(2); height: width }
                        }
                        AbstractButton {
                            width: units.gu(4); height: units.gu(4)
                            enabled: pos >= 0 && pos < root.sliders.length - 1
                            opacity: enabled ? 1 : 0.3
                            onClicked: root.moveSlider(modelData, 1)
                            Icon { anchors.centerIn: parent; name: "go-down"; width: units.gu(2); height: width }
                        }
                        Switch {
                            objectName: "slider-" + modelData
                            checked: pos >= 0
                            onTriggered: root.setSlider(modelData, checked)
                        }
                    }
                }
            }

            SettingsItemTitle { text: i18n.tr("Screen corners") }
            SettingsListItems.Standard {
                text: i18n.tr("Show the rounded-corner safe area")
                Switch {
                    objectName: "safeAreaSwitch"
                    checked: gs.showSafeArea
                    onTriggered: gs.showSafeArea = checked
                }
            }

            SettingsListItems.SingleControl {
                Button {
                    objectName: "resetButton"
                    text: i18n.tr("Reset to default")
                    width: parent.width - units.gu(4)
                    onClicked: ["tiles", "wide-tiles", "columns", "collapsed-rows", "sliders", "pull-down-opens",
                                "show-notifications"].forEach(function(k) { gs.schema.reset(k); })
                }
            }
        }
    }
}
