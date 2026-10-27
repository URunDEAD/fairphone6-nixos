/*
 * FP6 quick settings: tile catalogue and backends.
 * Toggles go through the indicators' exported GActions (the same actions their
 * menus use) or through GSettings keys of the FP6 modules. Nothing here polls.
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15
import GSettings 1.0
import QMenuModel 1.0
import "QsLayout.js" as QsLayout

Item {
    id: backend
    visible: false

    // set by the panel: identifier -> {busName, actionsObjectPath, menuObjectPath} or null
    property var findIndicator: function(identifier) { return null; }
    // the control centre is on screen (menu scans run only then)
    property bool shown: false
    // bumped by the panel when the indicator list may have changed
    property int indicatorsRevision: 0
    signal screenshotRequested()

    // ---- settings of the control centre itself
    readonly property alias settings: qsSettings
    GSettings {
        id: qsSettings
        schema.id: "com.fairphone6.quick-settings"
    }
    readonly property var defaultTiles: ["wifi", "bluetooth", "mobile-data", "flashlight", "dnd",
        "rotation-lock", "airplane", "location", "battery-saver", "night-light", "hotspot",
        "screen-record", "dark-theme", "silent", "screenshot", "battery", "sound", "time-date",
        "messages", "system", "privacy", "usb-drive", "keyboard"]
    readonly property var tiles: qsSettings.tiles !== undefined ? QsLayout.toList(qsSettings.tiles) : defaultTiles
    readonly property var wideTiles: qsSettings.wideTiles !== undefined ? QsLayout.toList(qsSettings.wideTiles) : ["wifi", "bluetooth"]
    readonly property int columns: Math.max(4, Math.min(6, qsSettings.columns || 4))
    readonly property int collapsedRows: Math.max(1, Math.min(3, qsSettings.collapsedRows || 2))
    readonly property var sliders: qsSettings.sliders !== undefined ? QsLayout.toList(qsSettings.sliders) : ["brightness", "volume", "media"]
    readonly property var allSliders: ["brightness", "volume", "media"]
    readonly property bool openControlCentre: (qsSettings.pullDownOpens || "control-centre") === "control-centre"
    readonly property bool showNotifications: qsSettings.showNotifications !== false

    function setTiles(list, wide) {
        qsSettings.tiles = list;
        if (wide !== undefined) qsSettings.wideTiles = wide;
    }
    function resetLayout() {
        var keys = ["tiles", "wide-tiles", "columns", "collapsed-rows", "sliders"];
        for (var i = 0; i < keys.length; i++) qsSettings.schema.reset(keys[i]);
    }

    // ---- other modules' settings
    GSettings { id: displayComfort; schema.id: "com.fairphone6.display-comfort" }
    GSettings { id: notificationSettings; schema.id: "com.fairphone6.notifications" }

    // ---- indicator action groups (each is one D-Bus signal subscription)
    function on(a) { return !!(a && a.valid && a.state); }
    function valid(a) { return !!(a && a.valid); }
    // stateful boolean GAction without parameter: activate() toggles it;
    // with a boolean parameter ('b'): activate(new state).
    function flip(a, withParam) {
        if (!valid(a)) return;
        if (withParam) a.activate(!a.state); else a.activate();
    }

    QDBusActionGroup {
        id: net
        busType: DBus.SessionBus
        busName: "com.lomiri.indicator.network"
        objectPath: "/com/lomiri/indicator/network"
        property variant wifi: action("wifi.enable")
        property variant airplane: action("airplane.enabled")
        property variant mobileData: action("mobiledata.enabled")
        property variant hotspot: action("hotspot.enable")
        property variant carrier: action("modem.1::status-label")
        property variant scanQr: action("wifi.scan-qr")
        Component.onCompleted: start()
    }
    QDBusActionGroup {
        id: bt
        busType: DBus.SessionBus
        busName: "org.ayatana.indicator.bluetooth"
        objectPath: "/org/ayatana/indicator/bluetooth"
        property variant enabled: action("bluetooth-enabled")
        property variant supported: action("bluetooth-supported")
        Component.onCompleted: start()
    }
    QDBusActionGroup {
        id: loc
        busType: DBus.SessionBus
        busName: "com.lomiri.indicator.location"
        objectPath: "/com/lomiri/indicator/location"
        property variant detection: action("location-detection-enabled")
        property variant gps: action("gps-detection-enabled")
        Component.onCompleted: start()
    }
    QDBusActionGroup {
        id: display
        busType: DBus.SessionBus
        busName: "org.ayatana.indicator.display"
        objectPath: "/org/ayatana/indicator/display"
        property variant rotationLock: action("rotation-lock")
        Component.onCompleted: start()
    }
    QDBusActionGroup {
        id: power
        busType: DBus.SessionBus
        busName: "org.ayatana.indicator.power"
        objectPath: "/org/ayatana/indicator/power"
        property variant flashlight: action("flashlight")
        property variant keepScreenOn: action("keep-screen-on")
        property variant brightness: action("brightness")
        property variant autoBrightness: action("auto-brightness")
        Component.onCompleted: start()
    }
    QDBusActionGroup {
        id: sound
        busType: DBus.SessionBus
        busName: "org.ayatana.indicator.sound"
        objectPath: "/org/ayatana/indicator/sound"
        property variant volume: action("volume")
        property variant silent: action("silent-mode")
        Component.onCompleted: start()
    }
    QDBusActionGroup {
        id: dndGroup
        busType: DBus.SessionBus
        busName: "com.lomiri.indicator.dnd"
        objectPath: "/com/lomiri/indicator/dnd"
        property variant dnd: action("dnd")
        property variant bedtime: action("bedtime")
        Component.onCompleted: start()
    }
    QDBusActionGroup {
        id: rec
        busType: DBus.SessionBus
        busName: "com.lomiri.indicator.screenrecord"
        objectPath: "/com/lomiri/indicator/screenrecord"
        property variant recording: action("recording")
        Component.onCompleted: start()
    }
    QDBusActionGroup {
        id: battery
        busType: DBus.SessionBus
        busName: "com.fairphone6.indicator.battery"
        objectPath: "/com/fairphone6/indicator/battery"
        property variant saver: action("saver")
        Component.onCompleted: start()
    }

    // sliders
    readonly property alias brightnessAction: power.brightness
    readonly property alias autoBrightnessAction: power.autoBrightness
    readonly property alias volumeAction: sound.volume
    function toggleAutoBrightness() { flip(power.autoBrightness, true); }

    // ---- detail labels read from the indicator menus
    QsMenuScan {
        id: wifiName
        scanning: backend.shown
        info: backend.indicatorsRevision >= 0 ? backend.findIndicator("indicator-network") : null
        typePattern: "accesspoint"
    }
    QsMenuScan {
        id: btDevice
        scanning: backend.shown && backend.on(bt.enabled)
        info: backend.indicatorsRevision >= 0 ? backend.findIndicator("ayatana-indicator-bluetooth") : null
        actionPattern: "device-[0-9]+-connected$"
    }

    // live icon / label of an indicator (its status-bar item), for the page tiles
    function liveIcon(info) {
        var it = info && info.item;
        return it && it.icons && it.icons.length ? String(it.icons[0]) : "";
    }
    function liveLabel(info) {
        var it = info && info.item;
        return it ? (it.rightLabel || it.leftLabel || "") : "";
    }
    function pageInfo(identifier) { return indicatorsRevision >= 0 ? findIndicator(identifier) : null; }

    function onOff(b) { return b ? i18n.tr("On") : i18n.tr("Off"); }

    // ---- the catalogue (order = order of the tray in the editor)
    readonly property var catalogue: [wifi, bluetooth, mobileData, flashlight, dnd, rotationLock,
        airplane, location, batterySaver, nightLight, hotspot, screenRecord, darkTheme, silent,
        screenshot, batteryPage, soundPage, timeDatePage, messagesPage, systemPage, privacyPage,
        usbDrivePage, keyboardPage, bedtime, stayAwake, greyscale, scanQr]
    function tile(id) {
        for (var i = 0; i < catalogue.length; i++) if (catalogue[i].tileId === id) return catalogue[i];
        return null;
    }

    QsTileDef {
        id: wifi
        tileId: "wifi"; label: i18n.tr("Wi-Fi")
        iconOn: "network-wifi-symbolic"; iconOff: "wifi-none"
        available: backend.valid(net.wifi)
        active: backend.on(net.wifi)
        subtitle: !active ? i18n.tr("Off") : (wifiName.label || i18n.tr("Not connected"))
        lockPolicy: "unlock"
        indicator: "indicator-network"; settingsUrl: "settings:///system/wifi"
        toggle: function() { backend.flip(net.wifi, false); }
    }
    QsTileDef {
        id: bluetooth
        tileId: "bluetooth"; label: i18n.tr("Bluetooth")
        iconOn: "bluetooth-active"; iconOff: "bluetooth-disabled"
        available: backend.valid(bt.enabled) && (!backend.valid(bt.supported) || backend.on(bt.supported))
        active: backend.on(bt.enabled)
        subtitle: !active ? i18n.tr("Off") : (btDevice.label || i18n.tr("On"))
        lockPolicy: "unlock"
        indicator: "ayatana-indicator-bluetooth"; settingsUrl: "settings:///system/bluetooth"
        toggle: function() { backend.flip(bt.enabled, true); }
    }
    QsTileDef {
        id: mobileData
        tileId: "mobile-data"; label: i18n.tr("Mobile data")
        iconOn: "network-cellular-lte"; iconOff: "network-cellular-none"
        available: backend.valid(net.mobileData)
        active: backend.on(net.mobileData)
        subtitle: active && backend.valid(net.carrier) && net.carrier.state ? String(net.carrier.state) : backend.onOff(active)
        lockPolicy: "unlock"
        indicator: "indicator-network"; settingsUrl: "settings:///system/cellular"
        toggle: function() { backend.flip(net.mobileData, false); }
    }
    QsTileDef {
        id: flashlight
        tileId: "flashlight"; label: i18n.tr("Flashlight")
        iconOn: "torch-on"; iconOff: "torch-off"
        available: backend.valid(power.flashlight)
        active: backend.on(power.flashlight)
        canWide: false
        indicator: "ayatana-indicator-power"
        toggle: function() { backend.flip(power.flashlight, true); }
    }
    QsTileDef {
        id: dnd
        tileId: "dnd"; label: i18n.tr("Do Not Disturb")
        iconOn: "night-mode"
        available: backend.valid(dndGroup.dnd)
        active: backend.on(dndGroup.dnd)
        subtitle: {
            if (!active) return i18n.tr("Off");
            var r = notificationSettings.dndReason || "";
            if (r === "bedtime") return i18n.tr("Bedtime");
            if (r.indexOf("schedule") === 0) return i18n.tr("Scheduled");
            return i18n.tr("On");
        }
        indicator: "fp6-notifyd"; settingsUrl: "settings:///system/do-not-disturb"
        toggle: function() { backend.flip(dndGroup.dnd, false); }
    }
    QsTileDef {
        id: rotationLock
        tileId: "rotation-lock"; label: i18n.tr("Rotation lock")
        iconOn: "orientation-lock"; iconOff: "orientation-lock-disabled"
        available: backend.valid(display.rotationLock)
        active: backend.on(display.rotationLock)
        canWide: false
        indicator: "ayatana-indicator-display"
        toggle: function() { backend.flip(display.rotationLock, true); }
    }
    QsTileDef {
        id: airplane
        tileId: "airplane"; label: i18n.tr("Airplane mode")
        iconOn: "airplane-mode"; iconOff: "airplane-mode-disabled"
        available: backend.valid(net.airplane)
        active: backend.on(net.airplane)
        lockPolicy: "unlock"
        indicator: "indicator-network"
        toggle: function() { backend.flip(net.airplane, false); }
    }
    QsTileDef {
        id: location
        tileId: "location"; label: i18n.tr("Location")
        iconOn: "location-active"; iconOff: "location-disabled"
        available: backend.valid(loc.detection)
        active: backend.on(loc.detection)
        subtitle: !active ? i18n.tr("Off") : (backend.on(loc.gps) ? i18n.tr("GPS on") : i18n.tr("On"))
        lockPolicy: "unlock"
        indicator: "fp6-location-indicator"; settingsUrl: "settings:///system/location"
        toggle: function() { backend.flip(loc.detection, false); }
    }
    QsTileDef {
        id: batterySaver
        tileId: "battery-saver"; label: i18n.tr("Battery saver")
        iconOn: "battery-caution-symbolic"; iconOff: "battery-good-symbolic"
        available: backend.valid(battery.saver)
        active: backend.on(battery.saver)
        indicator: "fp6-battery-indicator"; settingsUrl: "settings:///system/battery"
        toggle: function() { backend.flip(battery.saver, false); }
    }
    QsTileDef {
        id: nightLight
        tileId: "night-light"; label: i18n.tr("Night light")
        iconOn: "weather-clear-night-symbolic"
        available: displayComfort.nightLightEnabled !== undefined
        active: !!displayComfort.nightLightEnabled
        subtitle: !active ? i18n.tr("Off")
                  : ((displayComfort.nightLightCurrent || 6500) < 6500 ? i18n.tr("Active") : i18n.tr("Scheduled"))
        settingsUrl: "settings:///system/display-comfort"
        toggle: function() { displayComfort.nightLightEnabled = !displayComfort.nightLightEnabled; }
    }
    QsTileDef {
        id: hotspot
        tileId: "hotspot"; label: i18n.tr("Hotspot")
        iconOn: "hotspot-active"; iconOff: "hotspot-disabled"
        available: backend.valid(net.hotspot)
        active: backend.on(net.hotspot)
        lockPolicy: "unlock"
        settingsUrl: "settings:///system/hotspot"
        toggle: function() { backend.flip(net.hotspot, false); }
    }
    QsTileDef {
        id: screenRecord
        tileId: "screen-record"; label: i18n.tr("Screen recording")
        iconOn: "media-record"
        available: backend.valid(rec.recording)
        active: backend.on(rec.recording)
        // the recorder never runs on the lock screen (screen-capture module)
        lockPolicy: "unlock"
        indicator: "fp6-screenrecord-indicator"
        toggle: function() { backend.flip(rec.recording, false); }
    }
    QsTileDef {
        id: darkTheme
        tileId: "dark-theme"; label: i18n.tr("Dark theme")
        iconOn: "preferences-desktop-wallpaper-symbolic"
        available: qsSettings.darkTheme !== undefined
        active: !!qsSettings.darkTheme
        settingsUrl: "settings:///system/display-comfort"
        toggle: function() { qsSettings.darkTheme = !qsSettings.darkTheme; }
    }
    QsTileDef {
        id: silent
        tileId: "silent"; label: i18n.tr("Silent mode")
        iconOn: "audio-volume-muted"; iconOff: "audio-volume-high"
        available: backend.valid(sound.silent)
        active: backend.on(sound.silent)
        indicator: "ayatana-indicator-sound"
        toggle: function() { backend.flip(sound.silent, true); }
    }
    QsTileDef {
        id: screenshot
        tileId: "screenshot"; label: i18n.tr("Screenshot")
        iconOn: "camera-photo-symbolic"
        toggleable: false; canWide: false
        toggle: function() { backend.screenshotRequested(); }
    }
    QsTileDef {
        id: bedtime
        tileId: "bedtime"; label: i18n.tr("Bedtime")
        iconOn: "alarm-clock"
        available: backend.valid(dndGroup.bedtime)
        active: backend.on(dndGroup.bedtime)
        indicator: "fp6-notifyd"; settingsUrl: "settings:///system/do-not-disturb"
        toggle: function() { backend.flip(dndGroup.bedtime, false); }
    }
    QsTileDef {
        id: stayAwake
        tileId: "stay-awake"; label: i18n.tr("Stay awake")
        iconOn: "display-brightness-max"; iconOff: "display-brightness-min"
        available: backend.valid(power.keepScreenOn)
        active: backend.on(power.keepScreenOn)
        canWide: false
        indicator: "ayatana-indicator-power"
        toggle: function() { backend.flip(power.keepScreenOn, true); }
    }
    QsTileDef {
        id: greyscale
        tileId: "greyscale"; label: i18n.tr("Greyscale")
        iconOn: "preferences-color-symbolic"
        available: displayComfort.greyscaleActive !== undefined
        active: !!displayComfort.greyscaleActive
        canWide: false
        settingsUrl: "settings:///system/display-comfort"
        toggle: function() { displayComfort.greyscaleActive = !displayComfort.greyscaleActive; }
    }
    QsTileDef {
        id: scanQr
        tileId: "scan-qr"; label: i18n.tr("Scan QR code")
        iconOn: "camera-grid"
        toggleable: false; canWide: false
        available: backend.valid(net.scanQr)
        lockPolicy: "unlock"
        toggle: function() { if (backend.valid(net.scanQr)) net.scanQr.activate(); }
    }

    // ---- page tiles: the indicator pages that have no switch tile of their own
    QsTileDef {
        id: batteryPage
        readonly property var info: backend.pageInfo("ayatana-indicator-power")
        tileId: "battery"; label: i18n.tr("Battery")
        iconOn: "battery-full-symbolic"
        pageOnly: true; toggleable: false
        available: !!info
        iconSource: backend.liveIcon(info)
        subtitle: backend.liveLabel(info)
        indicator: "ayatana-indicator-power"
    }
    QsTileDef {
        id: soundPage
        readonly property var info: backend.pageInfo("ayatana-indicator-sound")
        tileId: "sound"; label: i18n.tr("Sound")
        iconOn: "audio-volume-high"
        pageOnly: true; toggleable: false
        available: !!info
        iconSource: backend.liveIcon(info)
        subtitle: backend.liveLabel(info)
        indicator: "ayatana-indicator-sound"
    }
    QsTileDef {
        id: timeDatePage
        readonly property var info: backend.pageInfo("ayatana-indicator-datetime")
        tileId: "time-date"; label: i18n.tr("Time and date")
        iconOn: "preferences-system-time-symbolic"
        pageOnly: true; toggleable: false
        available: !!info
        
        subtitle: backend.liveLabel(info)
        indicator: "ayatana-indicator-datetime"
    }
    QsTileDef {
        id: messagesPage
        readonly property var info: backend.pageInfo("ayatana-indicator-messages")
        tileId: "messages"; label: i18n.tr("Messages")
        iconOn: "notification"
        pageOnly: true; toggleable: false
        available: !!info
        iconSource: backend.liveIcon(info)
        subtitle: backend.liveLabel(info)
        indicator: "ayatana-indicator-messages"
    }
    QsTileDef {
        id: systemPage
        readonly property var info: backend.pageInfo("ayatana-indicator-session")
        tileId: "system"; label: i18n.tr("System")
        iconOn: "system-shutdown"
        pageOnly: true; toggleable: false
        available: !!info
        
        subtitle: backend.liveLabel(info)
        indicator: "ayatana-indicator-session"
    }
    QsTileDef {
        id: privacyPage
        readonly property var info: backend.pageInfo("fp6-privacy-indicator")
        tileId: "privacy"; label: i18n.tr("Privacy")
        iconOn: "preferences-system-privacy-symbolic"
        pageOnly: true; toggleable: false
        available: !!info
        iconSource: backend.liveIcon(info)
        subtitle: backend.liveLabel(info)
        indicator: "fp6-privacy-indicator"
    }
    QsTileDef {
        id: usbDrivePage
        readonly property var info: backend.pageInfo("fp6-usb-drive-indicator")
        tileId: "usb-drive"; label: i18n.tr("USB drive")
        iconOn: "save-to"
        pageOnly: true; toggleable: false
        available: !!info
        
        subtitle: backend.liveLabel(info)
        indicator: "fp6-usb-drive-indicator"
    }
    QsTileDef {
        id: keyboardPage
        readonly property var info: backend.pageInfo("ayatana-indicator-keyboard")
        tileId: "keyboard"; label: i18n.tr("Keyboard")
        iconOn: "preferences-desktop-keyboard-shortcuts-symbolic"
        pageOnly: true; toggleable: false
        available: !!info
        
        subtitle: backend.liveLabel(info)
        indicator: "ayatana-indicator-keyboard"
    }
}
