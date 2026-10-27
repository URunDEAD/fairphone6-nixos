// Preview harness: the pulled-down FP6 panel with the control centre, at the
// FP6 size (1116x2484, GRID_UNIT_PX=24). FP6_QS_SHOT selects the scene.
import QtQuick 2.15
import QtQuick.Window 2.2
import Lomiri.Components 1.3
import QMenuModel 1.0
import Lomiri.Indicators 0.1 as Indicators
import "Panel/QuickSettings"
import "Shot.js" as Shot

Item {
    id: root
    width: 1116
    height: 2484
    readonly property string shot: Shot.shot
    readonly property string out: Shot.out
    readonly property bool light: shot.indexOf("light") >= 0
    readonly property bool locked: shot.indexOf("locked") >= 0
    readonly property real cornerRadius: 110

    function inset(top) {
        var r = cornerRadius; if (top >= r) return 0;
        var dy = r - top; return Math.ceil(r - Math.sqrt(r * r - dy * dy));
    }

    // wallpaper-ish background + the panel colour at 0.85 like PanelMenu
    Rectangle { anchors.fill: parent; gradient: Gradient { GradientStop { position: 0; color: "#2C3E50" } GradientStop { position: 1; color: "#7B4397" } } }
    Rectangle { anchors.fill: parent; color: root.light ? "#FFFFFF" : "#000000"; opacity: 0.85 }

    // the expanded indicator row (7 gu), corner-inset
    Rectangle {
        id: bar
        width: parent.width; height: units.gu(7)
        color: root.light ? "#FFFFFF" : "#000000"
        Row {
            anchors { right: parent.right; rightMargin: root.inset(units.gu(1)); verticalCenter: parent.verticalCenter }
            layoutDirection: Qt.RightToLeft
            spacing: units.gu(2.2)
            Repeater {
                model: [["", "13:24"], ["battery-080-charging", "77%"], ["audio-volume-medium", ""], ["night-mode", ""],
                        ["wifi-high-secure", ""], ["network-cellular-lte", ""], ["bluetooth-active", ""], ["location", ""],
                        ["orientation-lock", ""], ["notification", ""]]
                Column {
                    spacing: units.gu(0.5)
                    Item { width: Math.max(ic.width, lb.width); height: units.gu(2.5)
                        Icon { id: ic; visible: modelData[0] !== ""; name: modelData[0]; width: visible ? units.gu(2.5) : 0; height: units.gu(2.5); color: root.light ? "#111" : "#AEA79F"; anchors.centerIn: parent }
                        Label { id: lb; text: modelData[1]; visible: text !== ""; color: root.light ? "#111" : "#AEA79F"; anchors.centerIn: parent } }
                }
            }
        }
    }

    Loader {
        id: loader
        anchors { top: bar.bottom; left: parent.left; right: parent.right; bottom: handle.top }
        active: false
        sourceComponent: ControlCentre {
            backend: root.backend
            lightMode: root.light
            locked: root.locked
            notificationsModel: shade
            hideNotificationContent: root.locked
            lockedRestricted: ["indicator-network", "ayatana-indicator-bluetooth", "fp6-privacy-indicator"]
            bottomInset: units.gu(2)
            indicatorExists: function(i) { return true; }
            onOpenIndicator: console.log("openIndicator " + identifier)
            onOpenUrl: console.log("openUrl " + url)
            onUnlockRequested: console.log("unlockRequested")
        }
    }
    Rectangle { id: handle; anchors.bottom: parent.bottom; width: parent.width; height: units.gu(2); color: "transparent"
        Rectangle { anchors.centerIn: parent; width: units.gu(4); height: units.dp(3); radius: height; color: "#888" } }

    ListModel { id: shade }
    property var backend: null
    Component { id: backendComp; QsBackend { findIndicator: function(i) { return { busName: i === "indicator-network" ? "com.lomiri.indicator.network" : i === "ayatana-indicator-sound" ? "org.ayatana.indicator.sound" : "org.ayatana.indicator.bluetooth", actionsObjectPath: "/x", menuObjectPath: "/x/phone" }; } shown: true } }

    Component.onCompleted: {
        theme.name = light ? "Lomiri.Components.Themes.Ambiance" : "Lomiri.Components.Themes.SuruDark";
        var n = "com.lomiri.indicator.network/", p = "org.ayatana.indicator.power/";
        FakeActions.states = {
            "com.lomiri.indicator.network/wifi.enable": true,
            "com.lomiri.indicator.network/airplane.enabled": false,
            "com.lomiri.indicator.network/mobiledata.enabled": true,
            "com.lomiri.indicator.network/hotspot.enable": false,
            "com.lomiri.indicator.network/modem.1::status-label": "Carrier",
            "com.lomiri.indicator.network/wifi.scan-qr": null,
            "org.ayatana.indicator.bluetooth/bluetooth-enabled": true,
            "org.ayatana.indicator.bluetooth/bluetooth-supported": true,
            "com.lomiri.indicator.location/location-detection-enabled": true,
            "com.lomiri.indicator.location/gps-detection-enabled": true,
            "org.ayatana.indicator.display/rotation-lock": true,
            "org.ayatana.indicator.power/flashlight": false,
            "org.ayatana.indicator.power/keep-screen-on": false,
            "org.ayatana.indicator.power/brightness": 0.62,
            "org.ayatana.indicator.power/auto-brightness": true,
            "org.ayatana.indicator.sound/volume": 0.35,
            "org.ayatana.indicator.sound/silent-mode": false,
            "com.lomiri.indicator.dnd/dnd": true,
            "com.lomiri.indicator.dnd/bedtime": false,
            "com.lomiri.indicator.screenrecord/recording": false,
            "com.fairphone6.indicator.battery/saver": false
        };
        Indicators.FakeMenus.rows = {
            "com.lomiri.indicator.network": [
                { type: "lomiri.widgets.systemsettings.tablet.wifisection", action: "indicator.wifi.enable", isToggled: true, label: "Wi-Fi" },
                { type: "lomiri.widgets.systemsettings.tablet.accesspoint", action: "indicator.accesspoint.1", isToggled: false, label: "Network-A" },
                { type: "lomiri.widgets.systemsettings.tablet.accesspoint", action: "indicator.accesspoint.2", isToggled: true, label: "Home-Network" } ],
            "org.ayatana.indicator.bluetooth": [
                { type: "org.ayatana.indicator.switch", action: "indicator.phone-device-1-connected", isToggled: true, label: "Headphones" } ],
            "org.ayatana.indicator.sound": [
                { type: "org.ayatana.indicator.media-player", action: "indicator.player", isToggled: false, label: "Music Player" },
                { type: "org.ayatana.indicator.playback-item", action: "indicator.play", isToggled: false, label: "" } ]
        };
        var now = Date.now();
        var items = [
            { app: "Messages", appId: "messaging-app", icon: "image://theme/message", summary: "Alex", body: "See you at 7? I'll bring the board games.", time: now - 120000 },
            { app: "Messages", appId: "messaging-app", icon: "image://theme/message", summary: "Sam", body: "Thanks!", time: now - 900000 },
            { app: "Firefox", appId: "firefox", icon: "image://theme/browser-app", summary: "Download complete", body: "report.pdf (2.1 MB)", time: now - 1800000 },
            { app: "System update", appId: "", icon: "image://theme/preferences-system-updates-symbolic", summary: "An update is ready", body: "Restart to finish installing.", time: now - 3600000 }
        ];
        for (var i = 0; i < items.length; i++) shade.append(items[i]);
        backend = backendComp.createObject(root);
        if (shot.indexOf("w6") >= 0) backend.settings.columns = 6;
        if (shot.indexOf("w5") >= 0) backend.settings.columns = 5;
        loader.active = true;
        settle.start();
    }
    Timer {
        id: settle
        interval: 700
        onTriggered: {
            var cc = loader.item;
            if (shot.indexOf("expanded") >= 0) { cc.tilesExpanded = true; }
            if (shot.indexOf("edit") >= 0 || shot.indexOf("dragtest") >= 0) cc.editing = true;
            if (shot.indexOf("dragtest") >= 0) { dragTest.start(); return; }
            if (shot.indexOf("toggletest") >= 0) { toggleTest.run(); }
            shoot.start();
        }
    }
    // Functional check of the tiles: which GAction is activated, lock rules.
    QtObject {
        id: toggleTest
        property int unlocks: 0
        property var urls: []
        property var pages: []
        function run() {
            var cc = loader.item, ok = true;
            cc.unlockRequested.connect(function() { unlocks++; });
            cc.openIndicator.connect(function(i) { pages.push(i); });
            cc.openUrl.connect(function(u) { urls.push(u); });
            function check(name, cond) { console.log((cond ? "PASS " : "FAIL ") + name); ok = ok && cond; }
            FakeActions.log = [];
            ["wifi", "flashlight", "dnd", "rotation-lock", "airplane", "silent"].forEach(function(id) { cc.tap(backend.tile(id)); });
            console.log("log: " + FakeActions.log.join(" | "));
            if (root.locked) {
                check("locked: flashlight/dnd/rotation/silent only", FakeActions.log.length === 4 && unlocks === 2);
                cc.detail(backend.tile("wifi"));
                cc.detail(backend.tile("dnd"));
                check("locked detail: wifi asks unlock, dnd page opens", unlocks === 3 && pages.length === 1 && pages[0] === "fp6-notifyd");
                cc.tap(backend.tile("privacy"));
                check("locked: privacy page asks unlock", unlocks === 4);
                check("locked: wifi still shows its network (Android)", backend.tile("wifi").subtitle === "Home-Network");
            } else {
                check("wifi toggled off", backend.tile("wifi").active === false);
                check("flashlight activate(true)", FakeActions.log.indexOf("activate org.ayatana.indicator.power/flashlight(true)") >= 0);
                check("dnd activate()", FakeActions.log.indexOf("activate com.lomiri.indicator.dnd/dnd") >= 0);
                check("rotation activate(false)", FakeActions.log.indexOf("activate org.ayatana.indicator.display/rotation-lock(false)") >= 0);
                cc.detail(backend.tile("wifi"));
                cc.detail(backend.tile("night-light"));
                check("detail: wifi page, night light settings", pages[0] === "indicator-network" && urls[0] === "settings:///system/display-comfort");
                check("subtitle wifi off", backend.tile("wifi").subtitle === "Off");
                cc.tap(backend.tile("battery"));
                cc.tap(backend.tile("system"));
                check("page tiles open their indicator on tap: " + pages, pages[1] === "ayatana-indicator-power" && pages[2] === "ayatana-indicator-session");
            }
            console.log(ok ? "TOGGLETEST OK" : "TOGGLETEST FAILED");
        }
    }

    // Functional check of the editor: drag, drop into the tray, add, size, sliders.
    Timer {
        id: dragTest
        interval: 300
        onTriggered: {
            var e = loader.item.editorItem, ok = true;
            function check(name, cond) { console.log((cond ? "PASS " : "FAIL ") + name); ok = ok && cond; }
            // flashlight: hold it and drop it at the very start of the grid
            var fl = e.activeLay.pos["flashlight"];
            e.beginDrag("flashlight", Qt.point(fl.x + 10, fl.y + 10));
            e.moveDrag(Qt.point(units.gu(2) + 5, units.gu(6) + 5));
            e.endDrag();
            check("drag to front: " + backend.tiles.slice(0, 3), backend.tiles[0] === "flashlight");
            // hotspot: drag down into the tray
            e.beginDrag("hotspot", Qt.point(0, 0));
            e.moveDrag(Qt.point(units.gu(10), (e.activeLay.rows + 1) * e.cellHeight + units.gu(12)));
            e.endDrag();
            check("drag into tray removes", backend.tiles.indexOf("hotspot") < 0 && e.trayIds.indexOf("hotspot") >= 0);
            e.add("bedtime");
            check("add from tray", backend.tiles[backend.tiles.length - 1] === "bedtime");
            e.toggleWide("mobile-data");
            check("wide toggle", backend.wideTiles.indexOf("mobile-data") >= 0);
            e.toggleWide("flashlight");
            check("flashlight cannot be wide", backend.wideTiles.indexOf("flashlight") < 0);
            e.moveSlider("volume", -1);
            check("slider order " + backend.sliders, backend.sliders[0] === "volume");
            e.setSlider("media", false);
            check("slider hidden", backend.sliders.indexOf("media") < 0);
            backend.settings.columns = 6;
            check("columns 6", backend.columns === 6);
            backend.resetLayout();
            check("reset", backend.tiles.length === 23 && backend.columns === 4 && backend.sliders.length === 3);
            console.log(ok ? "DRAGTEST OK" : "DRAGTEST FAILED");
            shoot.start();
        }
    }
    Timer {
        id: shoot
        interval: 900
        onTriggered: root.grabToImage(function(r) { r.saveToFile(root.out); console.log("saved " + root.out); Qt.quit(); })
    }
}
