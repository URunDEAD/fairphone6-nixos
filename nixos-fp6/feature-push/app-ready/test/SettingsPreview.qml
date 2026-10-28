// Previews + checks of Settings > Memory & performance (offscreen,
// 46.5x103.5 gu at 24 px/gu = 1116x2484): the real plugin pages with a
// fixture status.json. Scene in Shot.js; PASS/FAIL lines on stdout.
//   main main-bottom main-empty advanced advanced-bottom never-picker clear
import QtQuick 2.12
import Lomiri.Components 1.3
import GSettings 1.0
import "Shot.js" as Shot

MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(103.5)
    applicationName: "lomiri-system-settings"
    Rectangle { anchors.fill: parent; color: theme.palette.normal.background; z: -1 }

    GSettings { id: gs; schema.id: "com.fairphone6.app-ready" }

    function check(name, ok, detail) { console.log((ok ? "PASS " : "FAIL ") + Shot.shot + ": " + name + (ok ? "" : " (" + detail + ")")); }

    function find(item, name) {
        if (!item) return null;
        if (item.objectName === name) return item;
        var kids = item.children || [];
        for (var i = 0; i < kids.length; i++) { var r = find(kids[i], name); if (r) return r; }
        return null;
    }
    function findText(item, sub) {
        if (!item) return null;
        if (item.text !== undefined && String(item.text).indexOf(sub) >= 0 && item.visible) return item;
        var kids = item.children || [];
        for (var i = 0; i < kids.length; i++) { var r = findText(kids[i], sub); if (r) return r; }
        return null;
    }

    AdaptivePageLayout {
        id: pageStack
        anchors.fill: parent
        primaryPageSource: Qt.resolvedUrl("PageComponent.qml")
    }

    Component.onCompleted: {
        gs.installedApps = ["morph-browser\tMorph\tmorph-browser", "lomiri-messaging-app\tMessages\tmessaging-app",
                            "lomiri-camera-app\tCamera\tcamera-app", "lomiri-gallery-app\tGallery\tgallery-app",
                            "lomiri-system-settings\tSystem Settings\tpreferences-system", "lomiri-clock-app\tClock\tclock-app"];
        gs.keepReady = ["morph-browser"];
        gs.preload = ["morph-browser"];
        if (Shot.shot !== "main-empty") gs.statusFile = Shot.status;
        if (Shot.shot === "advanced-bottom") gs.neverKeep = ["lomiri-clock-app"];
    }

    Timer {
        interval: 1500; running: true
        onTriggered: {
            var page = pageStack.primaryPage;
            mv.check("page title", page.title === "Memory & performance", page.title);
            if (Shot.shot.indexOf("main") === 0) {
                if (Shot.shot === "main-empty") {
                    mv.check("no status: hint shown", !!mv.findText(page, "not running"), "missing");
                    later.start(); return;
                }
                var bar = mv.find(page, "ramBar");
                mv.check("RAM bar shown", bar && bar.visible && bar.parts.length === 4, "hidden");
                var legend = mv.find(page, "ramLegend");
                mv.check("legend apps/cache/free", legend && legend.text.indexOf("Apps 1.5 GB") === 0 && legend.text.indexOf("cache 3.0 GB") > 0, legend ? legend.text : "-");
                var sel = mv.find(page, "profileSelector");
                mv.check("profile default Aggressive", sel && sel.selectedIndex === 2, sel ? sel.selectedIndex : "-");
                mv.check("predicted Morph listed", !!mv.find(page, "predicted_morph-browser"), "missing");
                var msg = mv.find(page, "predicted_lomiri-messaging-app");
                mv.check("predicted state text", msg && msg.subText.indexOf("22 % likely next") === 0 && msg.subText.indexOf("files cached") > 0, msg ? msg.subText : "-");
                var hit = mv.find(page, "hitRate");
                mv.check("hit rate", hit && hit.text.indexOf("83 % of 23") >= 0, hit ? hit.text : "-");
                if (Shot.shot === "main-bottom") page.flickable.contentY = page.flickable.contentHeight - page.flickable.height;
                if (Shot.shot === "main") {
                    sel.delegateClicked(1);
                    changed.start(); return;
                }
                later.start(); return;
            }
            // advanced / never-picker / clear
            // the way the user gets there
            mv.find(page, "advancedItem").clicked();
            advOpened.start();
        }
    }
    Timer {
        id: changed; interval: 400
        onTriggered: {
            mv.check("profile selection writes the key", gs.profile === "balanced", gs.profile);
            var t = mv.find(pageStack.primaryPage, "profileText");
            mv.check("profile text follows", t && t.text.indexOf("3 apps") > 0, t ? t.text : "-");
            gs.profile = "aggressive";
            later.start();
        }
    }
    property var adv: null
    Timer {
        id: advOpened; interval: 1000
        onTriggered: {
            var a = mv.find(pageStack, "memoryAdvancedPage");
            mv.adv = a;
            mv.check("advanced page opened", !!a, "no page");
            if (!a) { later.start(); return; }
            var ready = mv.find(a, "readyAppsSelector");
            mv.check("ready apps: profile default (6)", ready && ready.selectedIndex === 0 && ready.model[0].indexOf("(6)") > 0, ready ? ready.model[0] : "-");
            var zs = mv.find(a, "zramSizeSelector");
            mv.check("zram size: profile default 75 %", zs && zs.selectedIndex === 0 && zs.model[0].indexOf("75 %") > 0, zs ? zs.model[0] : "-");
            if (Shot.shot === "advanced") {
                ready.delegateClicked(4);  // 3
                zs.delegateClicked(2);     // 50 %
                advChanged.start(); return;
            }
            if (Shot.shot === "advanced-bottom") {
                mv.check("never-keep app listed", !!mv.find(a, "never_lomiri-clock-app"), "missing");
                a.flickable.contentY = a.flickable.contentHeight - a.flickable.height;
                later.start(); return;
            }
            if (Shot.shot === "never-picker") {
                mv.find(a, "addNeverItem").clicked();
                pickerOpened.start(); return;
            }
            if (Shot.shot === "clear") {
                mv.find(a, "clearHistoryButton").clicked();
                dialogOpened.start(); return;
            }
            later.start();
        }
    }
    Timer {
        id: advChanged; interval: 400
        onTriggered: {
            mv.check("ready apps written", gs.readyApps === 3, gs.readyApps);
            mv.check("zram percent written", gs.zramPercent === 50, gs.zramPercent);
            gs.readyApps = -1; gs.zramPercent = 0;
            later.start();
        }
    }
    Timer {
        id: pickerOpened; interval: 800
        onTriggered: {
            var p = mv.find(pageStack, "appLaunchPickerPage");
            mv.check("never picker opened", p && p.title === "Never keep ready", p ? p.title : "-");
            var row = mv.find(p, "pick_morph-browser");
            mv.check("kept app offered", !!row, "missing");
            row.clicked();
            picked.start();
        }
    }
    Timer {
        id: picked; interval: 400
        onTriggered: {
            mv.check("never-keep written", gs.neverKeep.indexOf("morph-browser") >= 0, JSON.stringify(gs.neverKeep));
            mv.check("removed from keep-ready", gs.keepReady.indexOf("morph-browser") < 0, JSON.stringify(gs.keepReady));
            later.start();
        }
    }
    Timer {
        id: dialogOpened; interval: 800
        onTriggered: {
            var before = gs.clearHistoryRequest;
            // the dialog lives in the PopupUtils overlay of the MainView
            var btn = mv.find(mv, "clearConfirm");
            mv.check("confirm dialog shown", !!btn, "missing");
            if (btn) btn.clicked();
            mv.clearBefore = before;
            cleared.start();
        }
    }
    property int clearBefore: 0
    Timer {
        id: cleared; interval: 600
        onTriggered: {
            mv.check("clear request sent", gs.clearHistoryRequest === mv.clearBefore + 1, gs.clearHistoryRequest);
            mv.check("cleared note shown", !!mv.find(mv.adv, "clearedLabel") && mv.find(mv.adv, "clearedLabel").visible, "hidden");
            later.start();
        }
    }
    Timer { id: later; interval: 700; onTriggered: mv.grabToImage(function(r) { r.saveToFile(Shot.out); Qt.quit(); }) }
}
