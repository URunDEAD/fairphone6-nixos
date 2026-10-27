import QtQuick 2.12
import QtQuick.Window 2.12
import Lomiri.Components 1.3

Window {
    id: appWindow
    visible: true
    width: units.gu(50)
    height: units.gu(100)
    title: "Create bug report"
MainView {
    id: root
    anchors.fill: parent
    objectName: "bugreportMain"
    applicationName: "fp6-bugreport"
    backgroundColor: "#fafafa"
    property bool captureMode: previewCapture
    property bool screenshotConsent: false
    property bool privileged: false
    property string windowValue: "15m"
    property var sectionData: [
        {key: "system", title: "System", detail: "FP6 release, kernel version and uptime"},
        {key: "units", title: "Services", detail: "Failed system and user units"},
        {key: "journal", title: "Recent journal", detail: "Bounded logs; sensitive text masked"},
        {key: "kernel", title: "Kernel log", detail: "dmesg, when readable"},
        {key: "coredumps", title: "Crash list", detail: "Crash metadata only; never core files"},
        {key: "power", title: "Battery & temperature", detail: "Charge, current and thermal sensors"},
        {key: "modem", title: "Modem / oFono", detail: "Service and modem state; identifiers masked"},
        {key: "network", title: "Network", detail: "Links and routes; addresses masked, no Wi-Fi names"},
        {key: "audio", title: "Audio", detail: "PipeWire / wpctl status"},
        {key: "display", title: "Display & GPU", detail: "Rendering, compositor and kernel errors"},
        {key: "memory", title: "Memory & app readiness", detail: "zram, pressure, OOM and app-ready units"},
        {key: "storage", title: "Storage", detail: "Filesystem usage; no file contents"}
    ]
    function selectedSections() {
        var selected = [];
        for (var i = 0; i < sections.count; ++i) {
            if (sections.itemAt(i).selected) selected.push(sectionData[i].key);
        }
        return selected.join(",");
    }
    Page {
        id: page
        Rectangle { anchors.fill: parent; color: "#fafafa"; z: -1 }
        header: PageHeader { title: "Create bug report" }
        Flickable {
            id: scroll
            anchors { fill: parent; topMargin: page.header.height; bottomMargin: footer.height }
            contentHeight: content.height + units.gu(4)
            clip: true
            Column {
                id: content
                x: units.gu(2.5)
                width: parent.width - units.gu(5)
                spacing: units.gu(2)
                Item { height: units.gu(0.5); width: 1 }
                Label { text: "DIAGNOSTICS · FAIRPHONE 6"; color: "#772953"; fontSize: "small" }
                Label { text: "A useful report.\nYour choice of details."; fontSize: "x-large"; width: parent.width; wrapMode: Text.Wrap }
                Rectangle {
                    width: parent.width; height: privacyText.height + units.gu(3); radius: units.gu(1); color: "#eee7ed"
                    Label {
                        id: privacyText
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1.5) }
                        text: "Saved only on this phone. Text is redacted, but review before sharing. No uploads, telemetry or core files."
                        wrapMode: Text.Wrap; color: "#5e2750"; fontSize: "small"
                    }
                }
                Label { text: "Journal time window"; fontSize: "medium" }
                Row {
                    spacing: units.gu(1)
                    Repeater {
                        model: ["15m", "1h", "24h"]
                        Button { text: modelData; width: units.gu(12); color: root.windowValue === modelData ? "#772953" : "#e6e6e6"; onClicked: root.windowValue = modelData }
                    }
                }
                SectionRow { title: "Capture now"; description: "For an intermittent glitch. An optional screenshot is taken first."; selected: root.captureMode; onSelectedChanged: root.captureMode = selected }
                SectionRow {
                    visible: root.captureMode; height: visible ? implicitRowHeight : 0
                    property real implicitRowHeight: units.gu(11)
                    title: "Include screenshot"; description: "UNREDACTED: may show messages, names or passwords. Uses the shell screenshot API if available."
                    selected: root.screenshotConsent; onSelectedChanged: root.screenshotConsent = selected
                }
                SectionRow { title: "Restricted system logs"; description: "Optional. Requests administrator authorization through polkit."; selected: false; onSelectedChanged: root.privileged = selected }
                Label { id: sectionHeading; text: "What goes into the report"; fontSize: "large"; width: parent.width; wrapMode: Text.Wrap }
                Label { text: "Turn off any section you do not want to include."; color: "#666666"; fontSize: "small"; width: parent.width; wrapMode: Text.Wrap }
                Column {
                    width: parent.width
                    Repeater {
                        id: sections
                        model: root.sectionData
                        SectionRow { title: modelData.title; description: modelData.detail }
                    }
                }
                Label { text: "Destination\n~/Documents/Bug reports/\nPrivate archive + summary.txt"; width: parent.width; wrapMode: Text.Wrap; fontSize: "small"; color: "#666666" }
            }
        }
        Rectangle {
            id: footer
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
            height: actions.height + units.gu(3)
            color: "#ffffff"
            Rectangle { width: parent.width; height: 1; color: "#dddddd" }
            Column {
                id: actions
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(1.5) }
                spacing: units.gu(1)
                Label { visible: backend.message.length > 0; text: backend.message; width: parent.width; wrapMode: Text.WrapAnywhere; fontSize: "small" }
                Button {
                    width: parent.width
                    text: backend.busy ? "Creating report…" : root.captureMode ? "Capture now" : "Create bug report"
                    color: "#e95420"
                    enabled: !backend.busy
                    onClicked: {
                        if (root.selectedSections().length === 0) {
                            backend.start(root.windowValue, "", root.privileged, false);
                        } else if (root.captureMode && root.screenshotConsent && previewPath.length === 0) {
                            root.Window.window.visibility = Window.Minimized;
                            captureDelay.start();
                        } else {
                            backend.start(root.windowValue, root.selectedSections(), root.privileged, false);
                        }
                    }
                }
                Label { text: "Review the report before sharing it."; fontSize: "small"; color: "#666666"; anchors.horizontalCenter: parent.horizontalCenter }
            }
        }
    }
    Timer { interval: 150; repeat: true; running: backend.busy; onTriggered: {
        backend.poll();
        if (!backend.busy && root.Window.window.visibility === Window.Minimized)
            root.Window.window.visibility = Window.Windowed;
    } }
    Timer {
        id: captureDelay; interval: 500
        onTriggered: backend.start(root.windowValue, root.selectedSections(), root.privileged, true)
    }
    Timer {
        interval: 1500; running: previewPath.length > 0
        onTriggered: {
            if (previewDetails) scroll.contentY = sectionHeading.y - units.gu(1.5);
            previewSave.start();
        }
    }
    Timer {
        id: previewSave; interval: 400
        onTriggered: root.grabToImage(function(result) {
            if (!result.saveToFile(previewPath)) console.error("Preview save failed");
            Qt.quit();
        })
    }
}
}
