/*
 * FP6 call recording: dialer-side client of the fp6-callrec user service.
 *
 * Settings: GSettings com.fairphone6.callrecording (gsettings-qt exposes the
 * keys camelCased: autoContacts, noticeAccepted, debugRecordAll, ...).
 * Live state and commands: org.gtk.Actions on the session bus
 * (com.fairphone6.CallRecorder, /com/fairphone6/CallRecorder) through
 * QMenuModel. When the service is not running every action is invalid and
 * `available` is false; callers disable their controls.
 *
 * Installed next to dialer-app.qml (src/qml), so LiveCallPage and DialerPage
 * get it through their existing `import "../"`.
 */
import QtQuick 2.4
import GSettings 1.0
import QMenuModel 1.0

QtObject {
    id: recorder

    readonly property var modes: ["off", "manual", "auto-all", "auto-contacts"]
    readonly property var formats: ["flac", "wav", "opus"]

    property GSettings settings: GSettings {
        schema.id: "com.fairphone6.callrecording"
    }

    property QDBusActionGroup group: QDBusActionGroup {
        busType: DBus.SessionBus
        busName: "com.fairphone6.CallRecorder"
        objectPath: "/com/fairphone6/CallRecorder"
        Component.onCompleted: start()
    }

    property var recordingAction: group.action("recording")
    property var statusAction: group.action("status")
    property var recordingsAction: group.action("recordings")
    property var playingAction: group.action("playing")
    property var deleteAction: group.action("delete")
    property var playAction: group.action("play")
    property var stopPlaybackAction: group.action("stop-playback")
    property var refreshAction: group.action("refresh")

    // settings (unknown values, e.g. a missing schema, read as "off")
    readonly property string mode: modes.indexOf(settings.mode) >= 0 ? settings.mode : "off"
    readonly property string format: formats.indexOf(settings.format) >= 0 ? settings.format : "flac"
    readonly property bool noticeAccepted: settings.noticeAccepted === true
    readonly property bool debugRecordAll: settings.debugRecordAll === true
    readonly property bool announce: settings.announce === true
    readonly property var autoContacts: settings.autoContacts ? settings.autoContacts : []
    // the call screen shows the Record button
    readonly property bool uiEnabled: mode !== "off" || debugRecordAll

    // service state
    readonly property bool available: recordingAction !== null && recordingAction.valid === true
    readonly property bool recording: available && recordingAction.state === true
    readonly property var status: available && statusAction ? parse(statusAction.state, {}) : ({})
    readonly property var recordings: recordingsAction && recordingsAction.valid ? parse(recordingsAction.state, []) : []
    readonly property var playing: playingAction && playingAction.valid ? parse(playingAction.state, {}) : ({})
    readonly property real startedAt: status.startedAt ? status.startedAt : 0
    readonly property string path: status.path ? status.path : ""
    readonly property string error: status.error ? status.error : ""

    function parse(state, fallback) {
        if (state === undefined || state === null || state === "")
            return fallback;
        try {
            var v = JSON.parse(state);
            return v === null ? fallback : v;
        } catch (e) {
            console.warn("CallRecorder: bad state", state);
            return fallback;
        }
    }

    function setMode(m) { settings.mode = m; }
    function acceptNotice() { settings.noticeAccepted = true; }
    function toggle() { if (available) recordingAction.activate(); }
    function play(id, which) {
        if (!playAction || !playAction.valid) return;
        playAction.activate(which && which !== "both" ? id + "|" + which : id);
    }
    function stopPlayback() { if (stopPlaybackAction && stopPlaybackAction.valid) stopPlaybackAction.activate(); }
    function remove(id) { if (deleteAction && deleteAction.valid) deleteAction.activate(id); }
    function refresh() { if (refreshAction && refreshAction.valid) refreshAction.activate(); }

    function addNumber(number) {
        number = (number || "").trim();
        if (number === "") return false;
        var list = [];
        for (var i = 0; i < autoContacts.length; i++) {
            if (autoContacts[i] === number) return false;
            list.push(autoContacts[i]);
        }
        list.push(number);
        settings.autoContacts = list;
        return true;
    }
    function removeNumber(number) {
        var list = [];
        for (var i = 0; i < autoContacts.length; i++)
            if (autoContacts[i] !== number) list.push(autoContacts[i]);
        settings.autoContacts = list;
    }

    // "m:ss" / "h:mm:ss"
    function formatDuration(seconds) {
        var s = Math.max(0, Math.floor(seconds));
        var h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60);
        s = s % 60;
        var ss = (s < 10 ? "0" : "") + s;
        if (h > 0) return h + ":" + (m < 10 ? "0" : "") + m + ":" + ss;
        return m + ":" + ss;
    }
    // "mm:ss" (or "h:mm:ss") for the in-call REC label
    function formatElapsed(ms) {
        var t = formatDuration(ms / 1000);
        return t.length === 4 ? "0" + t : t;
    }
    function pathLabel(p) {
        if (p === "volte") return i18n.tr("VoLTE");
        if (p === "cs") return i18n.tr("CS");
        if (p === "fallback") return i18n.tr("Fallback");
        return p;
    }
    function formatLabel(f) {
        if (f === "flac") return "FLAC";
        if (f === "wav") return "WAV";
        if (f === "opus") return "Opus";
        return f;
    }
}
