/*
 * FP6 charging feedback (nixos-fp6/feature-push/charging-feedback): the
 * charging state published by fp6-charging-feedback (session agent, action
 * group com.fairphone6.ChargingFeedback /com/fairphone6/ChargingFeedback,
 * stateful action "state" = JSON) and the texts shown for it. Used by the
 * charging animation (Fp6ChargingFeedback.qml) and the lock-screen line
 * (Greeter/Fp6ChargingStatus.qml).
 *
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15
import Lomiri.Components 1.3
import QMenuModel 1.0

Item {
    id: m
    visible: false

    QDBusActionGroup {
        id: agent
        busType: DBus.SessionBus
        busName: "com.fairphone6.ChargingFeedback"
        objectPath: "/com/fairphone6/ChargingFeedback"
        property variant stateAction: action("state")
        property variant wakeAction: action("wake")
        Component.onCompleted: start()
    }

    readonly property bool available: !!(agent.stateAction && agent.stateAction.valid)
    readonly property var st: {
        if (!available) return ({});
        try { return JSON.parse(String(agent.stateAction.state)) || ({}); } catch (e) { return ({}); }
    }
    // A real reading has arrived (seq is always present then).
    readonly property bool ready: available && st.seq !== undefined

    readonly property bool plugged: st.plugged === true
    readonly property bool wireless: st.source === "wireless"
    // "charging", "full", "paused" (charge limit / adaptive), "notcharging", "discharging"
    readonly property string status: st.status || "discharging"
    readonly property int level: st.level !== undefined ? st.level : -1
    // "fast", "normal", "slow"
    readonly property string speed: st.speed || ""
    readonly property int ttf: st.ttf !== undefined ? st.ttf : -1
    readonly property int target: st.target || 100
    readonly property bool limited: st.limited === true
    readonly property string mode: st.mode || "normal"   // "hold" = adaptive charging holds
    readonly property int holdUntil: st.holdUntil || 0
    readonly property int chargeMw: st.chargeMw || 0
    // A computer is connected: the USB modes notification ("Charging via USB") shows.
    readonly property bool usbHost: st.usbHost === true
    readonly property int seq: st.seq || 0
    readonly property string event: st.event || ""

    readonly property bool charging: plugged && status === "charging"

    // Ring colours [start, end]: green; blue while limited/paused; cyan-green fast; amber-green slow.
    function paletteFor(st, sp, lim) {
        if (st === "paused" || (st === "charging" && lim)) return ["#3aa8ff", "#8adcff"];
        if (st === "notcharging") return ["#9e9e9e", "#d0d0d0"];
        if (st === "charging" && sp === "fast") return ["#24c8ff", "#5ff08c"];
        if (st === "charging" && sp === "slow") return ["#f2c230", "#b5e05a"];
        return ["#34c759", "#a6ef6b"];
    }
    readonly property var palette: paletteFor(status, speed, limited)

    function wake(seconds) {
        if (agent.wakeAction && agent.wakeAction.valid) agent.wakeAction.activate(seconds);
    }

    // ---- texts ----------------------------------------------------------------
    function duration(s) {
        var min = Math.max(1, Math.round(s / 60));
        var h = Math.floor(min / 60);
        var mm = min % 60;
        // Translators: hours and minutes, e.g. "1 h 10 min"
        if (h > 0 && mm > 0) return i18n.tr("%1 h %2 min").arg(h).arg(mm);
        if (h > 0) return i18n.tr("%1 h").arg(h);
        return i18n.tr("%1 min").arg(mm);
    }
    function clock(t) {
        return Qt.formatTime(new Date(t * 1000), Qt.locale().timeFormat(Locale.ShortFormat));
    }

    // "Fast charging", "Charging", "Charging slowly", "Charged", ...
    readonly property string title: {
        switch (status) {
        case "charging":
            if (wireless) return speed === "fast" ? i18n.tr("Fast charging wirelessly") : i18n.tr("Charging wirelessly");
            if (speed === "fast") return i18n.tr("Fast charging");
            if (speed === "slow") return i18n.tr("Charging slowly");
            return i18n.tr("Charging");
        case "full": return i18n.tr("Charged");
        case "paused": return mode === "hold" ? i18n.tr("Adaptive charging") : i18n.tr("Charging paused");
        case "notcharging": return i18n.tr("Connected, not charging");
        }
        return "";
    }

    // Second line of the animation.
    readonly property string detail: {
        switch (status) {
        case "charging":
            if (limited)
                return ttf > 0 ? i18n.tr("Limited to %1 % · %2 left").arg(target).arg(duration(ttf))
                               : i18n.tr("Limited to %1 %").arg(target);
            return ttf > 0 ? i18n.tr("Full in %1").arg(duration(ttf)) : "";
        case "paused":
            if (mode === "hold")
                return holdUntil > 0 ? i18n.tr("Holding at %1 %, full by %2").arg(target).arg(clock(holdUntil))
                                     : i18n.tr("Holding at %1 %").arg(target);
            return i18n.tr("Limited to %1 % to protect the battery").arg(target);
        case "notcharging":
            return i18n.tr("Check the charger and the cable");
        }
        return "";
    }

    // Lock screen, under the clock: "Charging · 45 % · Full in 1 h 10 min"
    readonly property string lockLine: {
        var pct = level >= 0 ? i18n.tr("%1 %").arg(level) : "";
        function join(a) { return a.filter(function(x) { return x !== ""; }).join(" · "); }
        switch (status) {
        case "charging":
            if (limited)
                return join([title, pct, ttf > 0 ? i18n.tr("%1 % in %2").arg(target).arg(duration(ttf)) : ""]);
            return join([title, pct, ttf > 0 ? i18n.tr("Full in %1").arg(duration(ttf)) : ""]);
        case "full": return join([title, pct]);
        case "paused":
            if (mode === "hold") return join([i18n.tr("Charging paused at %1 %").arg(level >= 0 ? level : target)]);
            return i18n.tr("Charging paused at %1 % to protect the battery").arg(level >= 0 ? Math.min(level, target) : target);
        case "notcharging": return join([title, pct]);
        }
        return "";
    }
    // Smaller second lock-screen line (charge limit / adaptive charging).
    readonly property string lockNote: {
        if (status === "charging" && limited) return i18n.tr("Limited to %1 % to protect the battery").arg(target);
        if (status === "paused" && mode === "hold")
            return holdUntil > 0 ? i18n.tr("Adaptive charging · full by %1").arg(clock(holdUntil)) : i18n.tr("Adaptive charging");
        return "";
    }
}
