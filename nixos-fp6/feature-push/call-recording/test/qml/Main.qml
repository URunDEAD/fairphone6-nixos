// Preview harness for the call-recording dialer UI at the FP6 size
// (1116x2412, GRID_UNIT_PX=24). Shot.js (written by preview.sh) selects the
// scene and output file. The dialer's patched src/qml tree is at ./qml.
// Fake data only (+999 numbers, made-up names).
import QtQuick 2.15
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import Lomiri.Telephony 0.1
import QMenuModel 1.0
import "Shot.js" as Shot

MainView {
    id: mainView
    width: Shot.width
    height: Shot.height
    applicationName: "lomiri-dialer-app"
    theme.name: Shot.dark ? "Lomiri.Components.Themes.SuruDark" : "Lomiri.Components.Themes.Ambiance"

    // what LiveCall.qml and the pages expect from dialer-app.qml
    property bool greeterMode: false
    property string delayedDialNumber: ""
    property color backgroundColor: theme.palette.normal.background
    property bool multiplePhoneAccounts: false
    property bool telepathyReady: true
    property bool applicationActive: true
    property var account: null
    function switchToKeypadView() {}
    function showNotification(title, text) { console.log("notification", title, text) }

    property var settings: ({ contactSearchWithDialPad: false })

    QtObject {
        id: greeter
        property bool greeterActive: false
        function showGreeter() {}
    }

    QtObject {
        id: fakeCall
        property bool active: true
        property bool held: false
        property bool muted: false
        property bool voicemail: false
        property bool isConference: false
        property bool incoming: false
        property string phoneNumber: "+999 555 0101"
        property int elapsedTime: 95
        property string activeAudioOutput: "earpiece"
        property var audioOutputs: [{ id: "earpiece" }, { id: "speaker" }]
        property var account: ({ displayName: "SIM 1" })
        signal callEnded()
        signal callHoldingFailed()
        function endCall() {}
        function sendDTMF(c) {}
    }

    QtObject {
        id: callManager
        property var foregroundCall: fakeCall
        property var backgroundCall: null
        property var calls: [fakeCall]
        property bool hasCalls: true
        property bool callIndicatorVisible: false
        signal conferenceRequestFailed()
        function mergeCalls(a, b) {}
    }

    QtObject {
        id: telepathyHelper
        property bool flightMode: false
        property var phoneAccounts: ({ displayed: [], all: [] })
        property var voiceAccounts: ({ displayed: [], all: [] })
        property bool dialpadSoundsEnabled: true
    }

    PageStack {
        id: pageStackNormalMode
        anchors.fill: parent
    }

    function fakeRecordings() {
        return JSON.stringify([
            { id: "2026-10-27/143012-out-p999555010", start: "2026-10-27T14:30:12", duration: 83.4, direction: "out",
              contact: "Test Contact A", number: "+999 555 0101", path: "volte", format: "flac",
              uplink: "/home/user/Recordings/Calls/2026-10-27/143012-out.uplink.flac",
              downlink: "/home/user/Recordings/Calls/2026-10-27/143012-out.downlink.flac",
              sidecar: "/home/user/Recordings/Calls/2026-10-27/143012-out.json", bytes: 2411520 },
            { id: "2026-10-26/091544-in-p999555020", start: "2026-10-26T09:15:44", duration: 312.0, direction: "in",
              contact: "", number: "+999 555 0202", path: "cs", format: "opus",
              uplink: "/home/user/Recordings/Calls/2026-10-26/091544-in.uplink.opus",
              downlink: "/home/user/Recordings/Calls/2026-10-26/091544-in.downlink.opus",
              sidecar: "/home/user/Recordings/Calls/2026-10-26/091544-in.json", bytes: 640000 },
            { id: "2026-10-25/180201-in-p999555030", start: "2026-10-25T18:02:01", duration: 41.9, direction: "in",
              contact: "Test Contact B", number: "+999 555 0303", path: "fallback", format: "wav",
              uplink: "/home/user/Recordings/Calls/2026-10-25/180201-in.uplink.wav",
              downlink: "/home/user/Recordings/Calls/2026-10-25/180201-in.downlink.wav",
              sidecar: "/home/user/Recordings/Calls/2026-10-25/180201-in.json", bytes: 8044000 }
        ])
    }

    function setup() {
        var s = Shot.shot
        FakeContacts.names = { "+999 555 0101": "Test Contact A", "+999 555 0303": "Test Contact B" }
        var recording = s.indexOf("recording") >= 0 || s === "incall-fallback"
        if (s.indexOf("noservice") < 0) {
            FakeActions.states = {
                "recording": recording,
                "status": JSON.stringify({ inCall: s.indexOf("incall") === 0, recording: recording,
                                           startedAt: recording ? Date.now() - 80800 : 0,
                                           path: recording ? (s === "incall-fallback" ? "fallback" : "volte") : "",
                                           mode: "manual", auto: false, error: "", announced: false }),
                "recordings": fakeRecordings(),
                "playing": s === "recordings" ? JSON.stringify({ id: "2026-10-26/091544-in-p999555020", which: "downlink", startedAt: Date.now() - 5000 }) : "{}",
                "delete": "", "play": "", "stop-playback": "", "refresh": ""
            }
        }
    }

    Component.onCompleted: {
        setup()
        var s = Shot.shot
        var page
        if (s.indexOf("incall") === 0) {
            page = pageStackNormalMode.push(Qt.resolvedUrl("qml/LiveCallPage/LiveCall.qml"))
        } else if (s.indexOf("settings") === 0 || s === "notice") {
            page = pageStackNormalMode.push(Qt.resolvedUrl("qml/SettingsPage/CallRecordingSettings.qml"))
        } else if (s.indexOf("recordings") === 0) {
            page = pageStackNormalMode.push(Qt.resolvedUrl("qml/SettingsPage/RecordingsPage.qml"))
        } else if (s === "phone-settings") {
            page = pageStackNormalMode.push(Qt.resolvedUrl("qml/SettingsPage/SettingsPage.qml"))
        }
        prepare.page = page
        prepare.start()
    }

    // settings values, then the scene's interaction, then the shot
    Timer {
        id: prepare
        property var page
        interval: 300
        onTriggered: {
            var s = Shot.shot
            var rec = findRecorder(page)
            if (rec) {
                var st = rec.settings
                st.mode = s === "settings-off" || s === "notice" ? "off" : (s === "settings-auto-contacts" ? "auto-contacts" : "manual")
                st.noticeAccepted = s !== "notice" && s !== "incall-notice"
                st.format = s === "settings-auto-contacts" ? "opus" : "flac"
                st.announce = s === "settings-auto-contacts"
                st.autoContacts = s === "settings-auto-contacts" ? ["+999 555 0101", "+999 555 0404"] : []
            }
            if (s === "incall-keypad-recording") page.dtmfVisible = true
            interact.start()
        }
    }
    Timer {
        id: interact
        interval: 400
        onTriggered: {
            var s = Shot.shot
            var page = prepare.page
            if (s === "notice") page.chooseMode("manual")
            if (s === "incall-notice") page.toggleRecording()
            if (s === "recordings-expanded") page.expandedId = "2026-10-27/143012-out-p999555010"
            shot.start()
        }
    }
    Timer {
        id: shot
        interval: 1500
        onTriggered: mainView.grabToImage(function(r) {
            r.saveToFile(Shot.out)
            console.log("saved", Shot.out, "log:", JSON.stringify(FakeActions.log))
            Qt.quit()
        })
    }

    function findRecorder(item) {
        if (!item) return null
        if (item.recorder) return item.recorder
        // LiveCall: its CallRecorder has id callRecorder; find through children data
        for (var i = 0; i < item.data.length; i++) {
            var d = item.data[i]
            if (d && d.modes !== undefined && d.settings !== undefined && d.toggle !== undefined) return d
        }
        return null
    }
}
