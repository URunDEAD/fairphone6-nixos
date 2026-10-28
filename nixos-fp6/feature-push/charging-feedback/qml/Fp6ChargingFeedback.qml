/*
 * FP6 charging feedback (nixos-fp6/feature-push/charging-feedback): what the
 * shell does when a charger is connected (Android/iOS style).
 *
 *  * Locked, or the display was off: a full-screen animation over the lock
 *    screen: a level ring that fills to the battery level with the
 *    percentage counting up, a ripple, "Fast charging" / "Charging" /
 *    "Charging slowly" and "Full in 1 h 10 min" / "Limited to 80 %" (~3 s).
 *    With the display off it asks fp6-charging-feedback to wake it (powerd
 *    Peek: off again after a few seconds without input; refused in a pocket
 *    or during a call) and starts once the display is on.
 *  * Unlocked: a small banner under the status bar instead (~2.8 s). Not
 *    when a computer is connected: the USB modes notification ("Charging
 *    via USB · Tap for options") is the pop-up then.
 *  * A short sound (plug / unplug; not in silent mode, Do Not Disturb or a
 *    call) and a haptic tick (not during Do Not Disturb).
 *
 * Settings: com.fairphone6.charging-feedback (animation, wake-screen, sound,
 * vibrate; preview-request from Settings > Battery > Charging feedback).
 * Never takes input (enabled: false); nothing animates while idle.
 *
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15
import GSettings 1.0
import Lomiri.Components 1.3
import Powerd 0.1
import QMenuModel 1.0

Item {
    id: fb
    objectName: "fp6ChargingFeedback"

    // Set by the shell.
    property bool locked: false
    property bool inCall: false
    property real panelHeight: units.gu(3)

    // "idle", "waiting" (display wake requested), "full", "compact"
    property string phase: "idle"
    property bool preview: false
    readonly property bool active: phase === "full" || phase === "compact"
    property real elapsed: 0
    readonly property int fullTotal: 3200
    readonly property int compactTotal: 2800
    readonly property int total: phase === "compact" ? compactTotal : fullTotal

    visible: active
    enabled: false

    Fp6ChargingModel { id: m }
    readonly property alias model: m

    GSettings {
        id: settings
        schema.id: "com.fairphone6.charging-feedback"
        onChanged: if (key === "previewRequest" || key === "preview-request") fb.showPreview()
    }
    GSettings {
        id: dnd
        schema.id: "com.fairphone6.notifications"
    }
    QDBusActionGroup {
        id: soundIndicator
        busType: DBus.SessionBus
        busName: "org.ayatana.indicator.sound"
        objectPath: "/org/ayatana/indicator/sound"
        property variant silentMode: action("silent-mode")
        Component.onCompleted: start()
    }

    readonly property bool dndActive: dnd.schema.isValid && (dnd.dndActive === true || dnd.bedtimeActive === true)
    readonly property bool dndNoWake: dndActive && (dnd.bedtimeActive === true || dnd.dndNoWake !== false)
    readonly property bool silentMode: !!(soundIndicator.silentMode && soundIndicator.silentMode.valid
                                          && soundIndicator.silentMode.state === true)
    readonly property bool wantAnimation: settings.animation !== false
    readonly property bool wantWake: settings.wakeScreen !== false
    readonly property bool wantSound: settings.sound !== false
    readonly property bool wantVibrate: settings.vibrate !== false

    NotificationAudio { id: plugSound; source: "@fp6ChargingPlugSound@" }
    NotificationAudio { id: unplugSound; source: "@fp6ChargingUnplugSound@" }

    // ---- events ---------------------------------------------------------------
    property int lastSeq: -1
    function check() {
        // From the parsed state itself: the derived properties (ready, seq,
        // event) update one after the other.
        if (!m.available || m.st.seq === undefined) return;
        var seq = m.st.seq || 0, ev = m.st.event || "";
        if (lastSeq < 0 || seq < lastSeq) {   // first reading, or the agent restarted
            lastSeq = seq;
            return;
        }
        if (seq === lastSeq) return;
        lastSeq = seq;
        if (ev === "plug") plugged();
        else if (ev === "unplug") unplugged();
    }
    Connections {
        target: m
        function onStChanged() { fb.check(); }
    }

    function feedback(plug) {
        if (wantVibrate && !dndActive)
            Haptics.play({ duration: plug ? 25 : 15 });
        if (wantSound && !silentMode && !dndActive && !inCall) {
            var s = plug ? plugSound : unplugSound;
            s.stop();
            s.play();
        }
    }

    function plugged() {
        feedback(true);
        preview = false;
        if (!wantAnimation) return;
        if (Powerd.status !== Powerd.On) {
            if (!wantWake || dndNoWake || inCall) return;
            phase = "waiting";
            m.wake(7);
            wakeTimeout.restart();
            return;
        }
        start(locked && !inCall ? "full" : "compact");
    }

    function unplugged() {
        feedback(false);
        wakeTimeout.stop();
        if (phase === "waiting") phase = "idle";
        else if (active && !preview) leave();
    }

    function showPreview() {
        if (Powerd.status !== Powerd.On) return;
        preview = true;
        feedback(true);
        start("full");
    }

    function start(kind) {
        // A computer: its USB notification is the pop-up while unlocked.
        if (kind === "compact" && m.st.usbHost === true && !preview) { phase = "idle"; return; }
        anim.stop();
        phase = kind;
        elapsed = 0;
        anim.from = 0;
        anim.to = total;
        anim.duration = total;
        anim.start();
    }

    // Fade out now (unplugged, display off, a computer turned out to be connected).
    function leave() {
        var at = total - fadeOut;
        if (!active || elapsed >= at) return;
        anim.stop();
        elapsed = at;
        anim.from = at;
        anim.to = total;
        anim.duration = fadeOut;
        anim.start();
    }

    NumberAnimation {
        id: anim
        target: fb
        property: "elapsed"
        easing.type: Easing.Linear
        onFinished: { fb.phase = "idle"; fb.preview = false; }
    }

    Timer {
        id: wakeTimeout
        interval: 1500
        onTriggered: if (fb.phase === "waiting") fb.phase = "idle"   // refused (pocket, call)
    }
    Connections {
        target: Powerd
        function onStatusChanged() {
            if (Powerd.status === Powerd.On && fb.phase === "waiting") {
                wakeTimeout.stop();
                fb.start("full");
            } else if (Powerd.status !== Powerd.On && fb.active) {
                anim.stop();
                fb.phase = "idle";
            }
        }
    }
    Connections {
        target: m
        function onUsbHostChanged() { if (m.usbHost && fb.phase === "compact" && !fb.preview) fb.leave(); }
    }

    // ---- what is shown ----------------------------------------------------------
    readonly property bool fakePlug: preview && !m.plugged
    readonly property string vStatus: fakePlug ? "charging" : m.status
    readonly property string vTitle: fakePlug ? i18n.tr("Charging") : m.title
    readonly property string vDetail: fakePlug ? i18n.tr("Preview") : m.detail
    readonly property int vLevel: m.level >= 0 ? m.level : 50
    readonly property string vSpeed: fakePlug ? "normal" : m.speed
    readonly property var palette: m.paletteFor(vStatus, vSpeed, m.limited && !fakePlug)

    // ---- timeline (ms) ---------------------------------------------------------
    readonly property int fadeIn: 300
    readonly property int fadeOut: 400
    function clamp01(x) { return Math.max(0, Math.min(1, x)); }
    function outCubic(x) { x = clamp01(x); return 1 - Math.pow(1 - x, 3); }
    function outBack(x) { x = clamp01(x); var c1 = 1.4, c3 = c1 + 1; return 1 + c3 * Math.pow(x - 1, 3) + c1 * Math.pow(x - 1, 2); }
    readonly property real presence: active ? clamp01(elapsed / fadeIn) * clamp01((total - elapsed) / fadeOut) : 0
    readonly property real fillT: outCubic((elapsed - 150) / 1100)
    readonly property real shownLevel: vLevel * fillT

    // ---- full screen (locked / display was off) ---------------------------------
    Item {
        id: full
        anchors.fill: parent
        visible: fb.phase === "full"

        Rectangle {
            anchors.fill: parent
            color: "black"
            opacity: 0.86 * fb.presence
        }

        Item {
            id: stage
            width: Math.min(parent.width, parent.height) * 0.52
            height: width
            anchors.horizontalCenter: parent.horizontalCenter
            y: parent.height * 0.40 - height / 2
            opacity: fb.presence
            scale: 0.86 + 0.14 * fb.outBack(fb.elapsed / 420)

            // ripple once at the start
            Rectangle {
                anchors.centerIn: parent
                readonly property real t: fb.clamp01((fb.elapsed - 200) / 1100)
                width: parent.width * (0.86 + 0.6 * fb.outCubic(t))
                height: width
                radius: width / 2
                color: "transparent"
                border.width: units.dp(2)
                border.color: fb.palette[0]
                opacity: 0.5 * (1 - t) * (t > 0 ? 1 : 0)
            }

            Fp6ChargingRing {
                anchors.fill: parent
                progress: fb.shownLevel / 100
                colorA: fb.palette[0]
                colorB: fb.palette[1]
                thickness: 0.055
                glow: 0.75
                sweep: fb.vSpeed === "fast" && fb.vStatus === "charging" ? fb.clamp01((fb.elapsed - 1000) / 1500) : -1
                bolt: false
            }

            Column {
                anchors.centerIn: parent
                spacing: units.gu(0.5)
                Fp6ChargingBolt {
                    anchors.horizontalCenter: parent.horizontalCenter
                    height: units.gu(4.2)
                    width: height * 0.62
                    color: fb.palette[1]
                    visible: fb.vStatus === "charging"
                }
                Row {
                    anchors.horizontalCenter: parent.horizontalCenter
                    Label {
                        id: pctLabel
                        objectName: "fp6ChargingPercent"
                        text: Math.round(fb.shownLevel)
                        color: "white"
                        font.pixelSize: units.gu(9)
                        font.weight: Font.Light
                    }
                    Label {
                        anchors.baseline: pctLabel.baseline
                        text: "%"
                        color: "white"
                        opacity: 0.8
                        font.pixelSize: units.gu(3.6)
                        font.weight: Font.Light
                    }
                }
            }
        }

        Column {
            anchors { top: stage.bottom; topMargin: units.gu(4); left: parent.left; right: parent.right
                      leftMargin: units.gu(3); rightMargin: units.gu(3) }
            spacing: units.gu(1)
            opacity: fb.presence * fb.clamp01((fb.elapsed - 250) / 400)
            Label {
                objectName: "fp6ChargingTitle"
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                text: fb.vTitle
                color: "white"
                font.pixelSize: units.gu(3.2)
                font.weight: Font.Light
                wrapMode: Text.WordWrap
            }
            Label {
                objectName: "fp6ChargingDetail"
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                visible: text !== ""
                text: fb.vDetail
                color: "white"
                opacity: 0.72
                font.pixelSize: units.gu(2.1)
                wrapMode: Text.WordWrap
            }
        }
    }

    // ---- compact banner (unlocked) ----------------------------------------------
    Rectangle {
        id: banner
        visible: fb.phase === "compact"
        readonly property real slide: fb.outCubic(fb.elapsed / 280) * fb.clamp01((fb.total - fb.elapsed) / fb.fadeOut)
        anchors.horizontalCenter: parent.horizontalCenter
        y: fb.panelHeight + units.gu(1) - (1 - slide) * (height + units.gu(2))
        opacity: slide
        width: Math.min(parent.width - units.gu(4), bannerRow.implicitWidth + units.gu(4))
        height: units.gu(7.5)
        radius: height / 2
        color: Qt.rgba(0.08, 0.08, 0.09, 0.94)
        border.width: units.dp(1)
        border.color: Qt.rgba(1, 1, 1, 0.08)

        Row {
            id: bannerRow
            anchors { left: parent.left; leftMargin: units.gu(1.25); verticalCenter: parent.verticalCenter }
            spacing: units.gu(1.5)
            Fp6ChargingRing {
                width: units.gu(5.5)
                height: width
                anchors.verticalCenter: parent.verticalCenter
                progress: fb.shownLevel / 100
                colorA: fb.palette[0]
                colorB: fb.palette[1]
                thickness: 0.09
                glow: 0.4
                boltScale: 0.42
                boltColor: fb.palette[1]
            }
            Column {
                anchors.verticalCenter: parent.verticalCenter
                width: Math.min(implicitWidth, fb.width - units.gu(15))
                Label {
                    width: Math.min(implicitWidth, fb.width - units.gu(15))
                    elide: Text.ElideRight
                    text: i18n.tr("%1 %").arg(Math.round(fb.shownLevel)) + "  ·  " + fb.vTitle
                    color: "white"
                    font.pixelSize: units.gu(2.2)
                }
                Label {
                    width: Math.min(implicitWidth, fb.width - units.gu(15))
                    elide: Text.ElideRight
                    visible: text !== ""
                    text: fb.vDetail
                    color: "white"
                    opacity: 0.7
                    font.pixelSize: units.gu(1.7)
                }
            }
        }
    }
}
