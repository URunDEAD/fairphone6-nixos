/*
 * FP6 edge lighting (nixos-fp6/feature-push/edge-flash). The FP6 has no
 * notification LED: a notification that arrives while the display is off
 * lights the screen edges for a moment instead of turning the display on.
 *
 * The display stays logically off the whole time (Powerd.status Off, lock
 * state and touch untouched). fp6-lomiri-powerd only raises the panel
 * brightness to a low level ("flash" action on /com/fairphone6/EdgeFlash,
 * system bus) while this item draws an opaque black frame with a glow along
 * the rounded edges. On the OLED panel black pixels emit nothing, so only
 * the edges light. Sequence: draw black + glow at strength 0, then ask for
 * the backlight; animate the glow while "flashing" is true; fade out, then
 * "stop" (powerd blanks) and hide. Animation is stepped at ~30 Hz (Timer),
 * so the shell renders ~30 frames per second, not 120, while it runs.
 *
 * Settings: com.fairphone6.edge-flash. Do Not Disturb decides first
 * (Notification.qml asks only for bubbles that DND would not silence).
 *
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15
import GSettings 1.0
import Powerd 0.1
import QMenuModel 1.0

Item {
    id: edge
    objectName: "fp6EdgeFlash"

    // Physical corner radius of the panel (quick-settings' estimate).
    property real cornerRadius: @fp6EdgeCornerRadiusPx@
    // "idle", "arming" (black frame drawn, backlight requested), "lit",
    // "fading", "preview" (display on: Settings' Preview button).
    property string phase: "idle"
    property color tint: "#3eb4f0"
    property real elapsed: 0          // ms since lit (wall clock)
    property real litAt: 0
    readonly property int duration: Math.max(1000, Math.min(6000, settings.durationMs || 2600))
    readonly property bool active: phase !== "idle"
    // Notifications that lit up since the display was last on.
    property int unseen: 0
    property int reminders: 0
    property color lastTint: tint

    visible: active
    enabled: false    // never takes input (touch is in LP while off)

    GSettings {
        id: settings
        schema.id: "com.fairphone6.edge-flash"
        onChanged: {
            if (key === "previewRequest" || key === "preview-request") edge.preview();
        }
    }
    GSettings {
        id: battery
        schema.id: "com.fairphone6.battery"
    }
    readonly property bool saverActive: battery.schema.isValid ? battery.saverActive === true : false

    QDBusActionGroup {
        id: powerd
        busType: DBus.SystemBus
        busName: "com.lomiri.Repowerd"
        objectPath: "/com/fairphone6/EdgeFlash"
        property variant flashAction: action("flash")
        property variant stopAction: action("stop")
        property variant flashingAction: action("flashing")
        Component.onCompleted: start()
    }
    readonly property bool available: !!(powerd.flashAction && powerd.flashAction.valid)
    readonly property bool lit: !!(powerd.flashingAction && powerd.flashingAction.valid && powerd.flashingAction.state === true)

    // ---- policy -------------------------------------------------------------
    function isMessage(h) {
        if (h["x-fp6-dnd"] !== undefined) return true;   // telephony: calls, SMS
        var apps = settings.messageApps || [];
        var app = String(h["x-fp6-app-name"] || "");
        var entry = String(h["desktop-entry"] || "").replace(/\.desktop$/, "");
        return (app !== "" && apps.indexOf(app) >= 0) || (entry !== "" && apps.indexOf(entry) >= 0);
    }

    // Called by Notification.qml for a bubble that DND lets through. true:
    // the edges light up and the display must not be woken. plain: not a
    // placeholder, confirmation (OSD) or snap decision (calls, alarms);
    // critical: urgency Critical (those wake as before).
    function offer(hints, plain, iconSource, critical) {
        if (settings.enabled !== true || !available) return false;
        if (Powerd.status !== Powerd.Off) return false;
        if (!plain || critical) return false;
        if (saverActive && settings.inBatterySaver !== true) return false;
        var h = hints || {};
        if (settings.onlyCallsMessages === true && !isMessage(h)) return false;
        var key = String(h["x-fp6-app-name"] || h["desktop-entry"] || iconSource || "");
        var colour = settings.colorMode === "fixed" ? fixedTint() : hashTint(key);
        unseen += 1;
        reminders = 0;
        flash(colour);
        if (settings.colorMode !== "fixed" && iconSource && String(iconSource) !== "")
            sampler.sample(String(iconSource));
        return true;
    }

    function fixedTint() {
        var c = String(settings.color || "");
        return /^#[0-9a-fA-F]{6}$/.test(c) ? c : "#3eb4f0";
    }

    // Stable per-app colour when the icon gives none (or until it is sampled).
    readonly property var tintPalette: ["#3eb4f0", "#4cd07d", "#f5a623", "#e8505b", "#a46cf0", "#2fc7c0", "#f07ac0", "#8fd14f"]
    function hashTint(key) {
        var h = 0;
        for (var i = 0; i < key.length; i++) h = (h * 31 + key.charCodeAt(i)) & 0x7fffffff;
        return tintPalette[h % tintPalette.length];
    }

    // ---- sequence -------------------------------------------------------------
    function flash(colour) {
        tint = colour;
        lastTint = colour;
        if (phase === "lit" || phase === "fading") {
            // Another one while lit: run the glow again, keep the backlight.
            elapsed = 0;
            litAt = Date.now();
            phase = "lit";
            request();
            return;
        }
        if (phase === "arming") return;
        elapsed = 0;
        phase = "arming";
        // Let the black frame reach the panel before the backlight comes up.
        armTimer.restart();
    }

    function request() {
        // A margin past the animation: powerd's own timer is only the safety net.
        powerd.flashAction.activate((duration + 600) + ":" + Math.max(1, Math.min(64, settings.brightness || 16)));
    }

    function finish() {
        armTimer.stop();
        stepTimer.stop();
        watchdog.stop();
        if (phase === "preview") { phase = "idle"; return; }
        // Hide once powerd reports dark (onLitChanged), or after a short wait.
        hideTimer.interval = 400;
        hideTimer.restart();
        if (lit && powerd.stopAction && powerd.stopAction.valid) powerd.stopAction.activate();
    }

    function abort() {
        armTimer.stop();
        stepTimer.stop();
        watchdog.stop();
        hideTimer.stop();
        phase = "idle";
    }

    function preview() {
        if (Powerd.status !== Powerd.On || phase !== "idle") return;
        tint = settings.colorMode === "fixed" ? fixedTint() : hashTint("preview");
        elapsed = 0;
        litAt = Date.now();
        phase = "preview";
        stepTimer.restart();
    }

    Timer {
        id: armTimer
        interval: 60
        onTriggered: {
            if (edge.phase !== "arming") return;
            edge.request();
            watchdog.restart();
        }
    }
    // powerd refused (display on, call, pocket, face down) or is missing.
    Timer {
        id: watchdog
        interval: 1000
        onTriggered: if (edge.phase === "arming") edge.abort()
    }
    Timer {
        id: hideTimer
        interval: 400
        onTriggered: edge.phase = "idle"
    }
    onLitChanged: {
        if (lit && phase === "arming") {
            watchdog.stop();
            elapsed = 0;
            litAt = Date.now();
            phase = "lit";
            stepTimer.restart();
        } else if (!lit && phase !== "idle" && phase !== "preview" && phase !== "arming") {
            // Dark again (stopped, timed out, or a real wake): hide after a frame.
            stepTimer.stop();
            hideTimer.interval = 50;
            hideTimer.restart();
        }
    }
    Timer {
        id: stepTimer
        interval: 33
        repeat: true
        onTriggered: {
            edge.elapsed = Date.now() - edge.litAt;
            if (edge.elapsed >= edge.duration) {
                edge.elapsed = edge.duration;
                edge.finish();
            } else if (edge.phase === "lit" && edge.elapsed >= edge.duration - edge.fadeOut) {
                edge.phase = "fading";
            }
        }
    }

    readonly property int fadeIn: 350
    readonly property int fadeOut: 500
    readonly property real strength: {
        if (phase === "idle" || phase === "arming") return 0;
        var t = elapsed;
        var up = Math.min(1, t / fadeIn);
        var down = Math.min(1, Math.max(0, (duration - t) / fadeOut));
        // A soft double pulse while held, like a breathing LED.
        var pulse = 0.85 + 0.15 * Math.cos(2 * Math.PI * t / 1300);
        return up * down * pulse;
    }

    // A real wake (power key, double tap, call) while the edges glow: get
    // out of the way at once. Display on also means "seen".
    Connections {
        target: Powerd
        function onStatusChanged() {
            if (Powerd.status === Powerd.On) {
                if (edge.phase !== "preview") edge.abort();
                edge.unseen = 0;
                edge.reminders = 0;
            }
        }
    }

    // Optional reminder for notifications that lit up but were not seen.
    Timer {
        id: reminder
        interval: Math.max(1, settings.reminderMinutes || 0) * 60000
        repeat: true
        running: settings.enabled === true && (settings.reminderMinutes || 0) > 0 && edge.unseen > 0
                 && Powerd.status === Powerd.Off && edge.reminders < Math.max(1, settings.reminderCount || 6)
        onTriggered: {
            if (edge.phase !== "idle" || !edge.available) return;
            if (edge.saverActive && settings.inBatterySaver !== true) return;
            if (dnd.schema.isValid && dnd.dndActive === true) return;   // DND began meanwhile
            edge.reminders += 1;
            edge.flash(edge.lastTint);
        }
    }
    GSettings {
        id: dnd
        schema.id: "com.fairphone6.notifications"
    }

    // ---- icon colour --------------------------------------------------------
    // The notification icon (often image://theme/...) drawn at 12x12; the
    // average of its saturated, opaque pixels. Runs once per notification.
    Canvas {
        id: sampler
        x: -width - 8
        width: 12
        height: 12
        renderTarget: Canvas.Image
        renderStrategy: Canvas.Immediate
        property string source: ""
        function sample(url) {
            if (source !== "") unloadImage(source);
            source = url;
            loadImage(url);
            if (isImageLoaded(url)) requestPaint();
        }
        onImageLoaded: requestPaint()
        onPaint: {
            if (source === "" || !isImageLoaded(source)) return;
            var ctx = getContext("2d");
            ctx.clearRect(0, 0, width, height);
            ctx.drawImage(source, 0, 0, width, height);
            var d = ctx.getImageData(0, 0, width, height).data;
            var r = 0, g = 0, b = 0, w = 0;
            for (var i = 0; i < d.length; i += 4) {
                var R = d[i] / 255, G = d[i + 1] / 255, B = d[i + 2] / 255, A = d[i + 3] / 255;
                var mx = Math.max(R, G, B), mn = Math.min(R, G, B);
                var sat = mx > 0 ? (mx - mn) / mx : 0;
                if (A < 0.5 || mx < 0.2 || sat < 0.25) continue;
                var weight = sat * A;
                r += R * weight; g += G * weight; b += B * weight; w += weight;
            }
            if (w > 0.5) {
                var c = Qt.rgba(r / w, g / w, b / w, 1);
                // Full value: the glow's brightness comes from the panel level.
                var m = Math.max(c.r, c.g, c.b);
                c = Qt.rgba(c.r / m, c.g / m, c.b / m, 1);
                edge.lastTint = c;
                if (edge.active && settings.colorMode !== "fixed") edge.tint = c;
            }
        }
    }

    Behavior on tint { ColorAnimation { duration: 250 } }

    // ---- the glow: one opaque full-screen pass (black + rounded-edge glow)
    ShaderEffect {
        id: glow
        anchors.fill: parent
        visible: edge.active
        blending: false
        property size size: Qt.size(width, height)
        property real radius: Math.min(edge.cornerRadius, Math.min(width, height) / 2)
        property real spread: Math.max(8, Math.min(width, height) * 0.035)
        property color tint: edge.tint
        property real strength: edge.strength
        // the bright patches go once round the edge per flash
        property real sweep: edge.elapsed / Math.max(1, edge.duration)
        fragmentShader: "
            #ifdef GL_ES
            precision highp float;
            #endif
            varying vec2 qt_TexCoord0;
            uniform float qt_Opacity;
            uniform vec2 size;
            uniform float radius;
            uniform float spread;
            uniform vec4 tint;
            uniform float strength;
            uniform float sweep;
            float roundedBox(vec2 p, vec2 b, float r) {
                vec2 q = abs(p) - b + vec2(r);
                return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - r;
            }
            void main() {
                vec2 p = qt_TexCoord0 * size - 0.5 * size;
                float sd = roundedBox(p, 0.5 * size, radius);
                // Outside the rounded panel: nothing (those pixels do not exist).
                float inside = step(sd, 0.0);
                float d = max(0.0, -sd);
                float halo = exp(-d / spread);
                float rim = exp(-d / (spread * 0.16));
                // Two soft bright patches running round the edge.
                float a = atan(p.y, p.x) / 6.2831853;
                float comet = pow(0.5 + 0.5 * cos(12.5663706 * (a - sweep)), 6.0);
                float k = inside * strength * (halo * (0.35 + 0.65 * comet) + 0.5 * rim);
                gl_FragColor = vec4(tint.rgb * min(k, 1.0), 1.0) * qt_Opacity;
            }"
    }
}
