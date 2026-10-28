/*
 * FP6 app permissions: data for the Settings pages, from
 * fp6-permission-agent's session action group com.fairphone6.Permissions
 * ("state" JSON = fp6-permd's state + installed apps; "set", "net", "reset",
 * "refresh"). GPL-3.0-only.
 */
import QtQuick 2.12
import Lomiri.Components 1.3
import QMenuModel 1.0

Item {
    id: m
    visible: false

    QDBusActionGroup {
        id: g
        busType: DBus.SessionBus
        busName: "com.fairphone6.Permissions"
        objectPath: "/com/fairphone6/Permissions"
        property variant stateAction: action("state")
        property variant setAction: action("set")
        property variant netAction: action("net")
        property variant resetAction: action("reset")
        property variant refreshAction: action("refresh")
        Component.onCompleted: start()
    }

    readonly property bool available: !!(g.stateAction && g.stateAction.valid)
    readonly property var st: {
        if (!available) return ({});
        try { return JSON.parse(String(g.stateAction.state)) || ({}); } catch (e) { return ({}); }
    }
    readonly property bool brokerUp: st.broker === true

    // Camera / microphone / location: listed when fp6-permd enforces them
    // (the module's media option).
    readonly property var media: st.media || []
    readonly property var mediaPerms: [
        { id: "camera", title: i18n.tr("Camera"), icon: "camera-app-symbolic",
          caption: i18n.tr("Apps that asked to use the camera") },
        { id: "microphone", title: i18n.tr("Microphone"), icon: "audio-input-microphone-symbolic",
          caption: i18n.tr("Apps that asked to use the microphone") },
        { id: "location", title: i18n.tr("Location"), icon: "location",
          caption: i18n.tr("Apps that asked for this device's location (only while Location is on)") }
    ]
    readonly property var perms: mediaPerms.filter(function(p) { return media.indexOf(p.id) >= 0; }).concat(basePerms)

    // Order and texts of the permission list.
    readonly property var basePerms: [
        { id: "usb", title: i18n.tr("USB devices"), icon: "stock_usb",
          caption: i18n.tr("Apps that asked to use USB devices (raw USB, USB HID, USB serial)") },
        { id: "nfc", title: i18n.tr("NFC"), icon: "nfc",
          caption: i18n.tr("Apps that asked to use NFC") },
        { id: "bluetooth", title: i18n.tr("Bluetooth devices"), icon: "bluetooth-active",
          caption: i18n.tr("Apps that asked to connect to Bluetooth devices directly (audio is not affected)") },
        { id: "contacts", title: i18n.tr("Contacts"), icon: "contact",
          caption: i18n.tr("Apps that asked to access your contacts") },
        { id: "calendar", title: i18n.tr("Calendar"), icon: "calendar",
          caption: i18n.tr("Apps that asked to access your calendar") },
        { id: "messages", title: i18n.tr("Call log & messages"), icon: "message",
          caption: i18n.tr("Apps that asked to access your call log, SMS and MMS") },
        { id: "files", title: i18n.tr("Files & media"), icon: "document-open",
          caption: i18n.tr("Apps that asked to access files outside their own folder (Documents, Pictures, Music, Videos, Downloads, SD card and USB storage)") },
        { id: "sensors", title: i18n.tr("Body & motion sensors"), icon: "sensors",
          caption: i18n.tr("Apps that asked to use motion and body sensors") }
    ]
    readonly property var netClasses: [
        { id: "wifi", title: i18n.tr("Wi-Fi and other networks") },
        { id: "mobile", title: i18n.tr("Mobile data") },
        { id: "vpn", title: i18n.tr("VPN") },
        { id: "local", title: i18n.tr("Devices on the local network") }
    ]

    function permInfo(id) {
        for (var i = 0; i < perms.length; i++) if (perms[i].id === id) return perms[i];
        for (var j = 0; j < mediaPerms.length; j++) if (mediaPerms[j].id === id) return mediaPerms[j];
        if (id === "network") return { id: "network", title: i18n.tr("Network"), icon: "network-wifi-symbolic" };
        if (id === "camera") return { id: id, title: i18n.tr("Camera"), icon: "camera-app-symbolic" };
        if (id === "microphone") return { id: id, title: i18n.tr("Microphone"), icon: "audio-input-microphone-symbolic" };
        if (id === "location") return { id: id, title: i18n.tr("Location"), icon: "location" };
        return { id: id, title: id, icon: "security-alert" };
    }

    function entry(app) { return (st.apps && st.apps[app]) || ({}); }
    function isSystem(app) {
        var s = st.system_apps || [];
        return s.indexOf(app) >= 0;
    }
    function isConfined(app) { return (st.confined || []).indexOf(app) >= 0; }
    function name(app) {
        if (st.names && st.names[app] && st.names[app].name) return st.names[app].name;
        var inst = st.installed || [];
        for (var i = 0; i < inst.length; i++) if (inst[i].id === app) return inst[i].name;
        return app === "system" ? i18n.tr("System") : app;
    }
    function icon(app) {
        if (st.names && st.names[app] && st.names[app].icon) return st.names[app].icon;
        var inst = st.installed || [];
        for (var i = 0; i < inst.length; i++) if (inst[i].id === app) return inst[i].icon;
        return "";
    }
    function perm(app, p) { var e = entry(app); return (e.perms && e.perms[p]) || ({}); }
    function decision(app, p) { return isSystem(app) ? "system" : (perm(app, p).decision || "ask"); }
    // Decision shown in lists: an active "Only this time" grant first.
    function shownDecision(app, p) { return (!isSystem(app) && perm(app, p).once) ? "once" : decision(app, p); }
    function isPreset(app, p) { return !isSystem(app) && perm(app, p).preset === true; }
    function decisionLabel(d) {
        switch (d) {
        case "allow": return i18n.tr("Allowed");
        case "while_using": return i18n.tr("While using");
        case "once": return i18n.tr("This time");
        case "deny": return i18n.tr("Not allowed");
        case "system": return i18n.tr("System");
        default: return i18n.tr("Ask");
        }
    }
    function network(app) {
        var n = entry(app).network || {};
        return { wifi: n.wifi !== false, mobile: n.mobile !== false, vpn: n.vpn !== false, local: n.local !== false };
    }
    function networkSummary(app) {
        if (isSystem(app)) return i18n.tr("System");
        var n = network(app);
        var on = [n.wifi, n.mobile, n.vpn, n.local].filter(function(x) { return x; }).length;
        if (on === 4) return i18n.tr("Allowed");
        if (on === 0) return i18n.tr("No network");
        if (!n.mobile && n.wifi) return i18n.tr("No mobile data");
        if (!n.wifi && n.mobile) return i18n.tr("Mobile data only");
        return i18n.tr("Limited");
    }

    // Every app worth listing: installed launcher apps plus apps the broker
    // knows (asked for something, or have a network rule).
    function allApps() {
        var seen = {}, out = [];
        var inst = st.installed || [];
        for (var i = 0; i < inst.length; i++) { seen[inst[i].id] = true; out.push(inst[i].id); }
        for (var k in (st.apps || {})) if (!seen[k] && k !== "unknown") { seen[k] = true; out.push(k); }
        out.sort(function(a, b) { return name(a).toLowerCase() < name(b).toLowerCase() ? -1 : 1; });
        return out;
    }
    // Apps that asked for a permission (have an entry for it).
    function requested(p) {
        var out = [];
        for (var k in (st.apps || {})) {
            var e = st.apps[k];
            if (e.perms && e.perms[p] && !isSystem(k)) out.push(k);
        }
        out.sort(function(a, b) { return name(a).toLowerCase() < name(b).toLowerCase() ? -1 : 1; });
        return out;
    }
    function grantedCount(p) {
        var r = requested(p), n = 0;
        for (var i = 0; i < r.length; i++) { var d = decision(r[i], p); if (d === "allow" || d === "while_using") n++; }
        return n;
    }
    function history(app) {
        var h = st.history || [];
        if (!app) return h;
        return h.filter(function(e) { return e.app === app; });
    }

    function timeText(t) {
        if (!t) return i18n.tr("Never");
        var now = st.now || Math.floor(Date.now() / 1000);
        var d = Math.max(0, now - t);
        if (d < 60) return i18n.tr("Just now");
        if (d < 3600) return i18n.tr("%1 minute ago", "%1 minutes ago", Math.floor(d / 60)).arg(Math.floor(d / 60));
        if (d < 86400) return i18n.tr("%1 hour ago", "%1 hours ago", Math.floor(d / 3600)).arg(Math.floor(d / 3600));
        return Qt.formatDateTime(new Date(t * 1000), Qt.DefaultLocaleShortDate);
    }
    function kindText(k, detail) {
        switch (k) {
        case "used": return i18n.tr("Used");
        case "denied": return i18n.tr("Refused");
        case "prompted": return i18n.tr("Asked: %1").arg(detail);
        case "changed": return i18n.tr("Changed: %1").arg(detail);
        case "revoked": return i18n.tr("Access taken back");
        default: return k;
        }
    }

    function set(app, p, d) { if (g.setAction && g.setAction.valid) g.setAction.activate(app + "\t" + p + "\t" + d); }
    function setNetwork(app, cls, on) { if (g.netAction && g.netAction.valid) g.netAction.activate(app + "\t" + cls + "\t" + (on ? "1" : "0")); }
    function reset(app) { if (g.resetAction && g.resetAction.valid) g.resetAction.activate(app); }
    function refresh() { if (g.refreshAction && g.refreshAction.valid) g.refreshAction.activate(); }
}
