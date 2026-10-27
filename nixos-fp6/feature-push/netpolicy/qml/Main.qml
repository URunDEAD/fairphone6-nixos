import QtQuick 2.12
import QtQuick.Window 2.12
import Lomiri.Components 1.3

Window {
    id: window
    width: units.gu(45); height: units.gu(90)
    visible: true
    title: "Network policy"
    color: "#f7f7f7"
    property bool preview: previewFile.length > 0
    Rectangle { anchors.fill: parent; color: "#f7f7f7" }
    MainView {
        id: root
        anchors.fill: parent
        applicationName: "org.fairphone.netpolicy"
        backgroundColor: "#f7f7f7"
        property var settings: ({})
        property var status: ({})
        property var apps: []
        property bool ready: false
        property bool editing: false
        property string feedback: ""
        function refresh() {
            if (window.preview || editing) return
            backend.refresh()
            if (backend.status.length) {
                status = JSON.parse(backend.status)
                settings = status.settings
                apps = status.snapshot.apps
                ready = true
            }
        }
        function save() {
            if (window.preview || !ready) return
            if (backend.save(JSON.stringify(settings))) {
                feedback = "Settings saved"
                editing = false
                refresh()
            } else feedback = backend.error
        }
        function change(key, value) {
            settings[key] = value
            save()
        }
        function allow(app, enabled) {
            var list = settings.allow_list.slice()
            var pos = list.indexOf(app)
            if (enabled && pos < 0) list.push(app)
            if (!enabled && pos >= 0) list.splice(pos, 1)
            settings.allow_list = list
            save()
        }
        Component.onCompleted: refresh()
        Timer { interval: 4000; running: !window.preview; repeat: true; onTriggered: root.refresh() }
        PageStack {
            id: stack
            anchors.fill: parent
            Component.onCompleted: {
                push(settingsPage)
                if (window.preview && previewPage === "usage") push(usagePage)
                if (window.preview && previewPage === "thresholds") push(thresholdPage)
            }
        }
        Component {
            id: settingsPage
            Page {
                id: settingsView
                title: "Network policy"
                header: PageHeader { title: "Network policy" }
                Flickable {
                    anchors { left: parent.left; right: parent.right; top: settingsView.header.bottom; bottom: parent.bottom }
                    contentHeight: content.height + units.gu(4)
                    clip: true
                    Column {
                        id: content
                        width: parent.width - units.gu(4)
                        anchors.horizontalCenter: parent.horizontalCenter
                        spacing: units.gu(2)
                        Item { width: 1; height: units.gu(0.5) }
                        Label { text: "YOUR CONNECTION, YOUR CHOICE"; color: "#666666"; textSize: Label.Small }
                        Label {
                            width: parent.width
                            text: window.preview ? "Offline preview · no network activity" : root.status.message || "Connecting to policy service…"
                            wrapMode: Text.WordWrap; color: "#444444"
                        }
                        Rectangle {
                            width: parent.width; height: units.gu(14); radius: units.gu(1)
                            color: "#e5f3ed"
                            Column {
                                anchors { fill: parent; margins: units.gu(2) }
                                spacing: units.gu(1)
                                Label { text: "Data saver"; textSize: Label.Large; color: "#185b43" }
                                Label { width: parent.width - units.gu(8); text: "Keep the app in front connected. Pause background data on mobile and metered Wi-Fi."; wrapMode: Text.WordWrap; color: "#244c3d" }
                            }
                            Switch {
                                anchors { right: parent.right; top: parent.top; margins: units.gu(2) }
                                checked: root.ready ? root.settings.data_saver : true
                                enabled: root.ready
                                onClicked: root.change("data_saver", checked)
                            }
                        }
                        Button {
                            width: parent.width; text: "App usage & background access  ›"
                            onClicked: stack.push(usagePage)
                        }
                        Label { width: parent.width; text: "Usage counts matched socket traffic on mobile and metered links. Foreground apps remain allowed at the background limit."; wrapMode: Text.WordWrap; textSize: Label.Small; color: "#666666" }
                        Rectangle { width: parent.width; height: 1; color: "#dddddd" }
                        Row {
                            width: parent.width; spacing: units.gu(2)
                            Label { width: parent.width - units.gu(10); text: "Smart Wi-Fi switching"; textSize: Label.Large }
                            Switch { checked: root.ready && root.settings.smart_switching; enabled: root.ready; onClicked: root.change("smart_switching", checked) }
                        }
                        Label { width: parent.width; text: "Prefer mobile when connected Wi-Fi stops working. Return only after a stable recovery."; wrapMode: Text.WordWrap; color: "#555555" }
                        Row {
                            width: parent.width
                            Label { width: parent.width - units.gu(8); text: "My mobile plan is capped" }
                            Switch { checked: root.ready ? root.settings.capped_plan : true; enabled: root.ready; onClicked: root.change("capped_plan", checked) }
                        }
                        Row {
                            width: parent.width
                            Label { width: parent.width - units.gu(8); text: "Allow fallback on a capped plan"; wrapMode: Text.WordWrap }
                            Switch { checked: root.ready && root.settings.allow_capped; enabled: root.ready; onClicked: root.change("allow_capped", checked) }
                        }
                        Label { width: parent.width; text: "Automatic fallback is off by default. Quality checks use your NetworkManager endpoint; no analytics are sent."; wrapMode: Text.WordWrap; textSize: Label.Small; color: "#666666" }
                        Button { width: parent.width; text: "Thresholds & connectivity checks  ›"; onClicked: stack.push(thresholdPage) }
                        Label { width: parent.width; text: "Turning Wi-Fi on near saved networks is outside this app’s scope."; wrapMode: Text.WordWrap; textSize: Label.Small; color: "#777777" }
                        Label { width: parent.width; text: root.feedback || (window.preview ? "" : backend.error); wrapMode: Text.WordWrap; color: "#a13d16" }
                    }
                }
            }
        }
        Component {
            id: usagePage
            Page {
                id: usageView
                header: PageHeader { title: "App usage & access" }
                Flickable {
                    anchors { left: parent.left; right: parent.right; top: usageView.header.bottom; bottom: parent.bottom }
                    clip: true; contentHeight: usageContent.height + units.gu(4)
                    Column {
                        id: usageContent
                        width: parent.width - units.gu(4); anchors.horizontalCenter: parent.horizontalCenter; spacing: units.gu(2)
                        Item { width: 1; height: units.gu(1) }
                        Label { text: root.status.snapshot ? "Month: " + root.status.snapshot.usage.month + " (UTC)" : "Monthly usage"; textSize: Label.Large }
                        Label { width: parent.width; text: "Allow background data for selected apps. The app in front is always allowed. Limits apply per app."; wrapMode: Text.WordWrap; color: "#555555" }
                        Label { visible: root.apps.length === 0; width: parent.width; text: "No configured apps yet. App scopes are supplied by the system configuration."; wrapMode: Text.WordWrap; color: "#777777" }
                        Repeater {
                            model: root.apps
                            delegate: Rectangle {
                                width: usageContent.width; height: units.gu(10); color: "white"; radius: units.gu(1)
                                Column {
                                    anchors { left: parent.left; top: parent.top; margins: units.gu(1.5) }
                                    width: parent.width - units.gu(10); spacing: units.gu(0.6)
                                    Label { width: parent.width; text: modelData; elide: Text.ElideRight }
                                    Label { text: ((root.status.snapshot.usage.totals[modelData] || 0) / 1000000).toFixed(1) + " MB"; textSize: Label.Small; color: "#666666" }
                                    Label { visible: root.ready && root.settings.limit_bytes > 0 && (root.status.snapshot.usage.totals[modelData] || 0) >= root.settings.limit_bytes; text: "Background limit reached"; color: "#a13d16"; textSize: Label.Small }
                                }
                                Switch { anchors { right: parent.right; verticalCenter: parent.verticalCenter; rightMargin: units.gu(1.5) } checked: root.ready && root.settings.allow_list.indexOf(modelData) >= 0; enabled: root.ready; onClicked: root.allow(modelData, checked) }
                            }
                        }
                    }
                }
            }
        }
        Component {
            id: thresholdPage
            Page {
                id: thresholdView
                header: PageHeader { title: "Thresholds & checks" }
                Flickable {
                    anchors { left: parent.left; right: parent.right; top: thresholdView.header.bottom; bottom: parent.bottom }
                    contentHeight: thresholdContent.height + units.gu(4); clip: true
                    Column {
                        id: thresholdContent
                        width: parent.width - units.gu(4); anchors.horizontalCenter: parent.horizontalCenter; spacing: units.gu(1.5)
                        Item { width: 1; height: units.gu(1) }
                        Label { width: parent.width; text: "Changes apply together when you tap Save. Zero disables a usage warning or limit."; wrapMode: Text.WordWrap; color: "#555555" }
                        Label { text: "Warning per app (MB / month)" }
                        TextField { id: warning; width: parent.width; inputMethodHints: Qt.ImhDigitsOnly; text: root.ready ? root.settings.warning_bytes / 1000000 : "500"; onActiveFocusChanged: if (activeFocus) root.editing = true }
                        Label { text: "Background limit per app (MB / month)" }
                        TextField { id: limit; width: parent.width; inputMethodHints: Qt.ImhDigitsOnly; text: root.ready ? root.settings.limit_bytes / 1000000 : "1000"; onActiveFocusChanged: if (activeFocus) root.editing = true }
                        Label { text: "Consecutive bad / good samples" }
                        Row { width: parent.width; spacing: units.gu(1)
                            TextField { id: bad; width: (parent.width - units.gu(1)) / 2; text: root.ready ? root.settings.thresholds.bad_samples : "3"; inputMethodHints: Qt.ImhDigitsOnly; onActiveFocusChanged: if (activeFocus) root.editing = true }
                            TextField { id: good; width: bad.width; text: root.ready ? root.settings.thresholds.good_samples : "5"; inputMethodHints: Qt.ImhDigitsOnly; onActiveFocusChanged: if (activeFocus) root.editing = true }
                        }
                        Label { text: "Cooldown (seconds)" }
                        TextField { id: cooldown; width: parent.width; text: root.ready ? root.settings.thresholds.cooldown_secs : "120"; inputMethodHints: Qt.ImhDigitsOnly; onActiveFocusChanged: if (activeFocus) root.editing = true }
                        Label { text: "Packet loss (%) / latency (ms) / weak signal (%)" }
                        Row { width: parent.width; spacing: units.gu(1)
                            TextField { id: loss; width: (parent.width - units.gu(2)) / 3; text: root.ready ? root.settings.thresholds.loss_percent : "40"; inputMethodHints: Qt.ImhDigitsOnly; onActiveFocusChanged: if (activeFocus) root.editing = true }
                            TextField { id: latency; width: loss.width; text: root.ready ? root.settings.thresholds.latency_ms : "800"; inputMethodHints: Qt.ImhDigitsOnly; onActiveFocusChanged: if (activeFocus) root.editing = true }
                            TextField { id: signal; width: loss.width; text: root.ready ? root.settings.thresholds.weak_signal : "20"; inputMethodHints: Qt.ImhDigitsOnly; onActiveFocusChanged: if (activeFocus) root.editing = true }
                        }
                        Label { text: "Connectivity URL (blank = NetworkManager)" }
                        TextField { id: endpoint; width: parent.width; text: root.ready ? root.settings.endpoint : ""; placeholderText: "Use the system connectivity endpoint"; inputMethodHints: Qt.ImhUrlCharactersOnly; onActiveFocusChanged: if (activeFocus) root.editing = true }
                        Label { width: parent.width; text: "The endpoint must return HTTP 204 or NetworkManager’s online response. Probes run only while smart switching is on."; wrapMode: Text.WordWrap; textSize: Label.Small; color: "#666666" }
                        Button {
                            width: parent.width; text: "Save thresholds"; color: "#21845b"; enabled: root.ready
                            onClicked: {
                                var values = [warning.text, limit.text, bad.text, good.text, cooldown.text, loss.text, latency.text, signal.text]
                                for (var i = 0; i < values.length; i++) {
                                    if (!/^[0-9]+$/.test(values[i])) { root.feedback = "Use whole, non-negative numbers"; return }
                                }
                                root.settings.warning_bytes = Number(warning.text) * 1000000
                                root.settings.limit_bytes = Number(limit.text) * 1000000
                                root.settings.thresholds = {bad_samples: Number(bad.text), good_samples: Number(good.text), cooldown_secs: Number(cooldown.text), loss_percent: Number(loss.text), latency_ms: Number(latency.text), weak_signal: Number(signal.text)}
                                root.settings.endpoint = endpoint.text
                                root.save()
                            }
                        }
                        Label { width: parent.width; text: root.feedback; wrapMode: Text.WordWrap; color: "#a13d16" }
                    }
                }
                Component.onDestruction: { root.editing = false; root.refresh() }
            }
        }
    }
    Timer {
        interval: 1000; running: window.preview; repeat: false
        onTriggered: window.contentItem.grabToImage(function(result) {
            if (!result.saveToFile(previewFile)) console.error("Preview save failed")
            Qt.quit()
        })
    }
}
