// USB Drive: share an ISO/IMG on the phone as a USB CD-ROM or disk.
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import Fp6.UsbDrive 1.0

MainView {
    id: root
    applicationName: "fp6-usb-drive"
    anchorToKeyboard: true
    width: units.gu(45)
    height: units.gu(80)

    UsbDrive { id: backend }

    property var status: ({ hosting: false, available: true })
    property var jobs: []
    property var library: ({ images: [], dir: "", exists: true })
    property var settings: backend.call("settings")
    readonly property bool hosting: status.hosting === true
    readonly property bool busy: jobs.some(function (j) { return j.status === "running" && j.kind === "host" })
    property var seenDone: ({})

    signal jobFinished(var job)

    function refreshLibrary() { library = backend.call("list") }
    function refreshStatus() { status = backend.call("status") }
    function refreshJobs() {
        var j = backend.call("jobs") || [];
        var changed = false;
        for (var i = 0; i < j.length; i++) {
            if (j[i].status !== "running" && !seenDone[j[i].id]) {
                seenDone[j[i].id] = true;
                changed = true;
                jobFinished(j[i]);
            }
        }
        jobs = j;
        if (changed) { refreshLibrary(); refreshStatus(); }
    }
    function start(cmd, args) {
        var r = backend.call(cmd, args || {});
        if (r && r.ok === false) {
            PopupUtils.open(errorDialog, root, { text: r.error });
        }
        refreshJobs();
        return r;
    }
    function backend_cmd(c, a) { var r = backend.call(c, a || {}); refreshJobs(); return r }
    function showError(t) { PopupUtils.open(errorDialog, root, { text: t }) }

    onJobFinished: {
        if (job.kind === "host" && job.status === "failed")
            showError(job.message);
    }

    Timer {
        // Fast while something is moving, slow otherwise; paused in the background.
        interval: (root.hosting || root.jobs.some(function (j) { return j.status === "running" })) ? 1000 : 4000
        running: Qt.application.state === Qt.ApplicationActive
        repeat: true
        triggeredOnStart: true
        onTriggered: { refreshStatus(); refreshJobs(); }
    }
    Connections {
        target: Qt.application
        function onStateChanged() { if (Qt.application.state === Qt.ApplicationActive) root.refreshLibrary() }
    }

    Component.onCompleted: {
        refreshLibrary();
        stack.push(Qt.resolvedUrl("LibraryPage.qml"));
        if (fp6Preview === "host" && library.images.length > 0)
            stack.push(Qt.resolvedUrl("HostPage.qml"), { image: library.images[0] });
        else if (fp6Preview === "downloads")
            stack.push(Qt.resolvedUrl("DownloadsPage.qml"));
        else if (fp6Preview === "settings")
            stack.push(Qt.resolvedUrl("SettingsPage.qml"));
        else if (fp6Preview === "create")
            stack.push(Qt.resolvedUrl("CreatePage.qml"));
    }

    PageStack { id: stack }

    Component {
        id: errorDialog
        Dialog {
            id: d
            title: i18n.tr("USB Drive")
            Button {
                text: i18n.tr("OK")
                onClicked: PopupUtils.close(d)
            }
        }
    }
}
