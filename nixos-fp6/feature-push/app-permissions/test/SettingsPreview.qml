// Offscreen previews + behaviour checks of Security & Privacy > App
// permissions (the patched stock AppAccess.qml and the FP6 pages), with a
// simulated fp6-permission-agent (stubs/QMenuModel).
import QtQuick 2.12
import Lomiri.Components 1.3
import QMenuModel 1.0
import "fp6-app-permissions"
import "Shot.js" as Shot
import "Sample.js" as Sample

MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(103.5)
    applicationName: "lomiri-system-settings"
    property var pluginOptions: ({})
    property var pluginManager: null

    Rectangle { anchors.fill: parent; color: theme.palette.normal.background; z: -1 }
    Fp6PermissionsModel { id: pm }

    PageStack {
        id: pageStack
        function addPageToNextColumn(src, page, props) { push(page, props); }
    }

    function check(ok, what) { console.log((ok ? "PASS " : "FAIL ") + Shot.shot + ": " + what); }
    function find(item, name) {
        if (!item) return null;
        if (item.objectName === name) return item;
        var kids = item.children || [];
        for (var i = 0; i < kids.length; i++) { var r = find(kids[i], name); if (r) return r; }
        if (item.contentItem && item.contentItem !== item) return find(item.contentItem, name);
        return null;
    }

    Component.onCompleted: {
        var media = Shot.shot.indexOf("-media") > 0;
        FakeBus.state.state = JSON.stringify(media ? Sample.mediaState : Sample.state);
        switch (Shot.shot) {
        case "settings-main": pageStack.push(Qt.resolvedUrl("security-privacy/AppAccess.qml")); break;
        case "settings-main-media": pageStack.push(Qt.resolvedUrl("security-privacy/AppAccess.qml")); break;
        case "settings-perm-media": pageStack.push(Qt.resolvedUrl("fp6-app-permissions/Fp6PermissionAppsPage.qml"), { model: pm, perm: "microphone" }); break;
        case "settings-app-media": pageStack.push(Qt.resolvedUrl("fp6-app-permissions/Fp6AppPermissionsPage.qml"), { model: pm, app: "lomiri-camera-app" }); break;
        case "settings-decision-media": pageStack.push(Qt.resolvedUrl("fp6-app-permissions/Fp6DecisionPage.qml"), { model: pm, app: "lomiri-camera-app", perm: "camera" }); break;
        case "settings-perm": pageStack.push(Qt.resolvedUrl("fp6-app-permissions/Fp6PermissionAppsPage.qml"), { model: pm, perm: "usb" }); break;
        case "settings-app": pageStack.push(Qt.resolvedUrl("fp6-app-permissions/Fp6AppPermissionsPage.qml"), { model: pm, app: "morph-browser" }); break;
        case "settings-app-nonet": pageStack.push(Qt.resolvedUrl("fp6-app-permissions/Fp6AppPermissionsPage.qml"), { model: pm, app: "org.example.Serial" }); break;
        case "settings-app-system": pageStack.push(Qt.resolvedUrl("fp6-app-permissions/Fp6AppPermissionsPage.qml"), { model: pm, app: "lomiri-system-settings" }); break;
        case "settings-network": pageStack.push(Qt.resolvedUrl("fp6-app-permissions/Fp6NetworkAppsPage.qml"), { model: pm }); break;
        case "settings-history": pageStack.push(Qt.resolvedUrl("fp6-app-permissions/Fp6PermissionHistoryPage.qml"), { model: pm }); break;
        case "settings-decision": pageStack.push(Qt.resolvedUrl("fp6-app-permissions/Fp6DecisionPage.qml"), { model: pm, app: "morph-browser", perm: "usb" }); break;
        }
    }

    Timer {
        interval: 1800; running: true
        onTriggered: {
            var p = pageStack.currentPage;
            check(!!p, "page loaded");
            switch (Shot.shot) {
            case "settings-main":
                var sec = find(p, "fp6PermissionsSection");
                check(!!sec, "FP6 section inside the stock App permissions page");
                var usb = find(p, "fp6Perm_usb");
                check(usb && usb.value === "2/2", "USB row counts asked/allowed apps (" + (usb ? usb.value : "-") + ")");
                var bt = find(p, "fp6Perm_bluetooth");
                check(bt && bt.value === "0/1", "Bluetooth row: asked but not allowed (" + (bt ? bt.value : "-") + ")");
                var net = find(p, "fp6PermNetwork");
                check(net && net.value === "3 apps limited", "network row (" + (net ? net.value : "-") + ")");
                check(!find(p, "fp6Perm_camera"), "no camera row without the media option");
                break;
            case "settings-main-media":
                var cam = find(p, "fp6Perm_camera");
                check(cam && cam.value === "1/1", "camera row: the Camera app's preset counts (" + (cam ? cam.value : "-") + ")");
                var mic = find(p, "fp6Perm_microphone");
                check(mic && mic.value === "1/2", "microphone row: Camera allowed, Recorder asks (" + (mic ? mic.value : "-") + ")");
                check(!!find(p, "fp6Perm_location"), "location row");
                break;
            case "settings-perm-media":
                check(!!find(p, "fp6PermApp_org.example.Recorder") && !!find(p, "fp6PermApp_lomiri-camera-app"), "microphone apps listed");
                check(find(p, "fp6PermApp_org.example.Recorder").layout.subtitle.text.indexOf("This time") === 0, "only-this-time grant shown");
                break;
            case "settings-app-media":
                check(find(p, "fp6AppPerm_camera").value === "Allowed", "camera preset shown as allowed");
                check(find(p, "fp6AppPerm_location").value === "Ask", "location asks");
                break;
            case "settings-decision-media":
                check(find(p, "fp6Decision_allow") !== null, "options listed");
                break;
            case "settings-perm":
                check(!!find(p, "fp6PermApp_morph-browser") && !!find(p, "fp6PermApp_org.example.Serial"), "both USB apps listed");
                check(!find(p, "fp6PermApp_lomiri-system-settings"), "system app not listed");
                break;
            case "settings-app":
                var mob = find(p, "fp6Net_mobile");
                check(!!mob, "network rows shown");
                check(find(p, "fp6AppPerm_usb").value === "While using", "USB decision shown");
                check(find(p, "fp6AppPerm_files").value === "Not allowed", "files decision shown");
                break;
            case "settings-app-nonet":
                check(!find(p, "fp6Net_wifi").visible, "per-class rows hidden when the app has no network at all");
                check(find(p, "fp6NetAllSwitch").checked === false, "master switch off");
                break;
            case "settings-app-system":
                check(!find(p, "fp6NetAll").enabled, "system component: network not switchable");
                break;
            case "settings-network":
                check(!!find(p, "fp6App_lomiri-calculator-app"), "installed apps listed");
                break;
            case "settings-history":
                check(p.flickable.count === Sample.state.history.length, "all events listed");
                break;
            case "settings-decision":
                check(find(p, "fp6Decision_while_using") !== null, "options listed");
                break;
            }
            shot.start();
        }
    }
    Timer {
        id: shot; interval: 500
        onTriggered: mv.grabToImage(function(r) {
            r.saveToFile(Shot.out + "/" + Shot.shot + ".png");
            var p = pageStack.currentPage;
            if (Shot.shot === "settings-decision") {
                find(p, "fp6Decision_deny").clicked();
                check(FakeBus.last("set") === "set morph-browser\tusb\tdeny", "decision sent to the agent");
            }
            if (Shot.shot === "settings-app") {
                find(p, "fp6AppReset").clicked();
                check(FakeBus.last("reset") === "reset morph-browser", "reset sent");
            }
            Qt.quit();
        })
    }
}
