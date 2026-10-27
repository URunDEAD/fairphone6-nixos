// Wi-Fi QR scanner: viewfinder with a scan frame; a Wi-Fi code opens
// "Join <network>?", Join creates and activates the connection through
// NetworkManager (WifiJoin), then the app closes. The camera runs only while
// the app is in front.
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import Fp6.WifiQrScan 1.0

MainView {
    id: root
    objectName: "fp6WifiQrScan"
    applicationName: appId
    width: units.gu(45)
    height: units.gu(80)

    readonly property bool appActive: Qt.application.state === Qt.ApplicationActive
    readonly property bool demo: fp6Preview.indexOf("demo") === 0
    property var sheet: null
    readonly property bool scanning: (appActive || demo) && sheet === null
                                     && join.state !== "connecting" && join.state !== "connected"
    property string lastText: ""
    property double lastTime: 0
    property string toast: ""

    Component.onCompleted: theme.name = "Lomiri.Components.Themes.SuruDark"

    WifiJoin {
        id: join
        onStateChanged: if (join.state === "connected") closeTimer.start()
    }

    QrScanFilter {
        id: scanner
        scanning: root.scanning
        onDecoded: root.handle(text)
    }

    function handle(text) {
        var now = Date.now();
        // the same code stays in view: once per 3 s
        if (text === lastText && now - lastTime < 3000)
            return;
        lastText = text;
        lastTime = now;
        var r = join.parse(text);
        if (r.status === "ok")
            sheet = PopupUtils.open(joinSheet, root);
        else if (r.status === "not-wifi")
            showToast(r.reason);
        else
            sheet = PopupUtils.open(problemSheet, root, { reason: r.reason, enterprise: r.enterprise });
    }
    function closeSheet() {
        if (sheet)
            PopupUtils.close(sheet);
        sheet = null;
        lastTime = Date.now();
    }
    function showToast(t) {
        toast = t;
        toastTimer.restart();
    }

    Timer {
        id: toastTimer
        interval: 2500
        onTriggered: root.toast = ""
    }
    Timer {
        id: closeTimer
        interval: 1600
        onTriggered: if (!root.demo) Qt.quit()
    }
    // Frames that cannot be read on the CPU (and the laptop demo): grab the
    // viewfinder item a few times a second instead.
    Timer {
        interval: 350
        repeat: true
        running: root.scanning && viewLoader.item !== null && (root.demo || !scanner.frameAccess)
        onTriggered: viewLoader.item.grab(function (img) { scanner.decodeImage(img); })
    }

    Page {
        anchors.fill: parent
        header: PageHeader {
            id: header
            title: i18n.tr("Scan QR code")
            leadingActionBar.actions: [
                Action {
                    iconName: "back"
                    text: i18n.tr("Back")
                    onTriggered: Qt.quit()
                }
            ]
        }

        Rectangle {
            id: stage
            anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
            color: "black"
            clip: true

            Loader {
                id: viewLoader
                anchors.fill: parent
                source: root.demo ? "DemoView.qml" : "CameraView.qml"
                onLoaded: {
                    item.active = Qt.binding(function () { return root.appActive || root.demo; });
                    item.filter = scanner;
                }
            }

            MouseArea {
                anchors.fill: parent
                onClicked: if (viewLoader.item) viewLoader.item.focusAt(mouse.x, mouse.y)
            }

            // scan frame: dimmed surroundings, corner marks
            Item {
                id: frame
                readonly property real side: Math.min(stage.width, stage.height) * 0.68
                anchors.centerIn: parent
                anchors.verticalCenterOffset: -stage.height * 0.04
                width: side
                height: side
                visible: viewLoader.item !== null && viewLoader.item.error === ""
                readonly property color dim: Qt.rgba(0, 0, 0, 0.45)
                readonly property real arm: side * 0.14
                readonly property real thick: units.dp(4)

                Rectangle { color: frame.dim; x: -stage.width; y: -stage.height; width: stage.width * 3; height: stage.height }
                Rectangle { color: frame.dim; x: -stage.width; y: frame.height; width: stage.width * 3; height: stage.height }
                Rectangle { color: frame.dim; x: -stage.width; y: 0; width: stage.width; height: frame.height }
                Rectangle { color: frame.dim; x: frame.width; y: 0; width: stage.width; height: frame.height }

                Repeater {
                    model: 4
                    Item {
                        x: index % 2 ? frame.width - frame.arm : 0
                        y: index > 1 ? frame.height - frame.arm : 0
                        width: frame.arm
                        height: frame.arm
                        Rectangle { color: "white"; radius: height / 2; width: frame.arm; height: frame.thick
                                    y: index > 1 ? frame.arm - frame.thick : 0 }
                        Rectangle { color: "white"; radius: width / 2; width: frame.thick; height: frame.arm
                                    x: index % 2 ? frame.arm - frame.thick : 0 }
                    }
                }
            }

            Label {
                id: hint
                objectName: "fp6ScanHint"
                anchors { top: frame.bottom; topMargin: units.gu(3); left: parent.left; right: parent.right; leftMargin: units.gu(3); rightMargin: units.gu(3) }
                visible: frame.visible
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                color: "white"
                text: i18n.tr("Point the camera at a Wi-Fi QR code")
            }

            Rectangle {
                id: toastBox
                objectName: "fp6ScanToast"
                anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: units.gu(6) }
                visible: root.toast.length > 0
                width: Math.min(toastLabel.implicitWidth + units.gu(4), stage.width - units.gu(4))
                height: toastLabel.height + units.gu(2)
                radius: height / 2
                color: Qt.rgba(0.15, 0.15, 0.15, 0.92)
                Label {
                    id: toastLabel
                    anchors { centerIn: parent }
                    width: Math.min(implicitWidth, stage.width - units.gu(8))
                    wrapMode: Text.WordWrap
                    horizontalAlignment: Text.AlignHCenter
                    color: "white"
                    text: root.toast
                }
            }

            // camera missing / busy
            Column {
                anchors.centerIn: parent
                width: parent.width - units.gu(6)
                spacing: units.gu(2)
                visible: viewLoader.status === Loader.Error || (viewLoader.item !== null && viewLoader.item.error !== "")
                Icon {
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: units.gu(6)
                    height: width
                    name: "camera-symbolic"
                    color: "white"
                }
                Label {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    color: "white"
                    text: viewLoader.status === Loader.Error ? i18n.tr("The camera stack is not installed.")
                          : viewLoader.item && viewLoader.item.error === "busy" ? i18n.tr("The camera is in use by another app. Close it and come back.")
                          : i18n.tr("The camera is not available: %1").arg(viewLoader.item ? viewLoader.item.error : "")
                }
            }
        }
    }

    Component {
        id: joinSheet
        Dialog {
            id: d
            objectName: "fp6JoinSheet"
            title: join.state === "connected" ? i18n.tr("Connected") : i18n.tr("Join “%1”?").arg(join.ssid)
            text: join.state === "connected" ? join.message : ""

            Component.onCompleted: {
                if (root.demo && (fp6PreviewStep === "connecting" || fp6PreviewStep === "connected"))
                    join.join();
            }

            // One positioned Column: the Dialog sizes itself by childrenRect,
            // which would count the hidden state rows.
            Item {
                width: parent.width
                height: body.height
                Column {
                    id: body
                    width: parent.width
                    spacing: units.gu(2)
                    Row {
                        spacing: units.gu(1.5)
                        visible: join.state !== "connected"
                        Icon {
                            width: units.gu(3)
                            height: width
                            name: join.security === "open" ? "wifi-full" : "wifi-full-secure"
                            color: theme.palette.normal.backgroundText
                        }
                        Column {
                            anchors.verticalCenter: parent.verticalCenter
                            width: body.width - units.gu(4.5)
                            Label {
                                text: join.securityText
                            }
                            Label {
                                visible: join.hidden || join.saved
                                width: parent.width
                                wrapMode: Text.WordWrap
                                textSize: Label.Small
                                color: theme.palette.normal.backgroundSecondaryText
                                text: join.saved ? (join.hidden ? i18n.tr("Hidden network · already saved on this phone")
                                                                : i18n.tr("Already saved on this phone"))
                                                 : i18n.tr("Hidden network")
                            }
                        }
                    }

                    Row {
                        spacing: units.gu(1.5)
                        visible: join.state === "connecting"
                        ActivityIndicator {
                            running: parent.visible
                        }
                        Label {
                            anchors.verticalCenter: parent.verticalCenter
                            width: body.width - units.gu(5)
                            wrapMode: Text.WordWrap
                            text: join.message
                        }
                    }

                    Label {
                        objectName: "fp6JoinError"
                        width: parent.width
                        visible: join.state === "failed"
                        wrapMode: Text.WordWrap
                        color: theme.palette.normal.negative
                        text: join.message
                    }

                    Icon {
                        visible: join.state === "connected"
                        anchors.horizontalCenter: parent.horizontalCenter
                        width: units.gu(5)
                        height: width
                        name: "tick"
                        color: theme.palette.normal.positive
                    }

                    Row {
                        id: buttons
                        width: parent.width
                        spacing: units.gu(1)
                        visible: join.state !== "connected"
                        readonly property real bw: (width - spacing) / 2
                        Button {
                            objectName: "fp6JoinCancel"
                            width: buttons.bw
                            text: i18n.tr("Cancel")
                            enabled: join.state !== "connecting"
                            onClicked: {
                                join.cancel();
                                root.closeSheet();
                            }
                        }
                        Button {
                            objectName: "fp6JoinButton"
                            width: buttons.bw
                            color: theme.palette.normal.positive
                            text: join.state === "failed" ? i18n.tr("Try again") : i18n.tr("Join")
                            enabled: join.state === "ready" || join.state === "failed"
                            onClicked: join.join()
                        }
                    }
                }
            }
        }
    }

    Component {
        id: problemSheet
        Dialog {
            id: p
            objectName: "fp6ProblemSheet"
            property string reason
            property bool enterprise: false
            title: enterprise ? i18n.tr("Can't add this network") : i18n.tr("Can't join from this code")
            text: reason
            Button {
                visible: p.enterprise
                text: i18n.tr("Wi-Fi settings")
                onClicked: {
                    scanApp.openWifiSettings();
                    Qt.quit();
                }
            }
            Button {
                objectName: "fp6ProblemOk"
                text: i18n.tr("OK")
                color: theme.palette.normal.positive
                onClicked: {
                    join.cancel();
                    root.closeSheet();
                }
            }
        }
    }
}
