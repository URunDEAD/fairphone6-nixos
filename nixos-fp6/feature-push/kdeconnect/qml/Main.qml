import QtQuick 2.12
import QtQuick.Window 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3

MainView {
    id: root
    objectName: "kdeConnectMain"
    applicationName: "com.fairphone6.KdeConnect"
    width: units.gu(50)
    height: units.gu(100)
    backgroundColor: "#f6f7f4"
    property var state: ({devices: [], name: "Fairphone 6", event: ""})
    property var selected: null
    property string shareUrl: initialShare
    property var pairPopup: null
    property string dismissedRequest: ""
    property color accent: "#187b68"
    property var pluginLabels: ({ping:"Ping",battery:"Battery status",notifications:"Notifications",clipboard:"Clipboard",share:"Files & links",findmyphone:"Find my phone",media:"Media controls",input:"Remote keyboard & pointer"})
    property var pluginNotes: ({ping:"Exchange a friendly hello",battery:"Share charge and charging status",notifications:"Sync new notifications in both directions",clipboard:"Share clipboard text while connected",share:"Receive files and send from the share menu",findmyphone:"Let this device ring your phone",media:"Allow control of phone media players",input:"Allow for 10 minutes; US keyboard layout"})
    function call(method, data, enabled) {
        connectBridge.command(method, selected ? selected.id : "", data || "", enabled || false)
    }
    function update() {
        connectBridge.poll()
        if (!connectBridge.snapshot) return
        state = JSON.parse(connectBridge.snapshot)
        if (selected) {
            var found = null
            for (var i = 0; i < state.devices.length; i++) if (state.devices[i].id === selected.id) found = state.devices[i]
            selected = found
        }
        if (pairPopup && selected && selected.paired) { PopupUtils.close(pairPopup); pairPopup = null }
        if (!previewMode) {
            for (var j = 0; j < state.devices.length; j++) {
                var device = state.devices[j]
                if (device.pending && device.pending.direction === "incoming" && dismissedRequest !== device.id && !pairPopup) {
                    selected = device
                    pairPopup = PopupUtils.open(pairDialog)
                    break
                }
            }
        }
    }
    Component.onCompleted: {
        update()
        if (previewPage === "plugins") selected = state.devices[0]
        if (previewPage === "pairing") selected = state.devices[2]
    }
    Timer { interval: 1000; running: !previewMode; repeat: true; onTriggered: root.update() }
    Timer {
        interval: 350; running: previewMode && previewPage === "pairing"; repeat: false
        onTriggered: root.pairPopup = PopupUtils.open(pairDialog)
    }
    Timer {
        interval: 2200; running: previewMode; repeat: false
        onTriggered: root.Window.window.contentItem.grabToImage(function(result) {
            if (!result.saveToFile(previewPath)) console.error("Preview save failed")
            Qt.quit()
        })
    }
    Rectangle { anchors.fill: parent; color: root.backgroundColor }
    Page {
        id: page
        anchors.fill: parent
        header: PageHeader {
            title: root.selected ? root.selected.name.replace(/[<>]/g, "") : "KDE Connect"
            leadingActionBar.actions: [Action {
                iconName: "back"; text: "Devices"; visible: root.selected !== null
                onTriggered: root.selected = null
            }]
            trailingActionBar.actions: [Action {
                iconName: "reload"; text: "Refresh"
                onTriggered: connectBridge.command("Discover", "", "", false)
            }]
        }
        Flickable {
            anchors { fill: parent; topMargin: page.header.height }
            contentHeight: content.height + units.gu(4)
            clip: true
            Column {
                id: content
                width: parent.width - units.gu(4)
                x: units.gu(2); y: units.gu(2)
                spacing: units.gu(2)
                Rectangle {
                    width: parent.width; height: hero.height + units.gu(4)
                    radius: units.gu(1.6); color: "#e3efea"
                    Column {
                        id: hero
                        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
                        spacing: units.gu(1)
                        Row {
                            spacing: units.gu(1)
                            Rectangle { width: units.gu(1); height: width; radius: width/2; color: root.accent; anchors.verticalCenter: parent.verticalCenter }
                            Label { textFormat: Text.PlainText; text: "YOUR PRIVATE CONNECTION"; color: root.accent; textSize: Label.XSmall; font.weight: Font.DemiBold }
                        }
                        Label { textFormat: Text.PlainText; text: root.selected ? (root.selected.online ? "Connected over Wi-Fi" : "Currently offline") : root.state.name; fontSize: "x-large"; color: "#183e35" }
                        Label { textFormat: Text.PlainText;
                            width: parent.width
                            text: root.selected ? (root.selected.paired ? "You decide what this device can access." : "Pair securely before sharing anything.") : "Your phone and your devices.\nTogether on your Wi-Fi, with you in control."
                            wrapMode: Text.WordWrap; color: "#416056"; textSize: Label.Small
                        }
                    }
                }
                Column {
                    visible: root.selected === null
                    width: parent.width; spacing: units.gu(1.5)
                    Row {
                        width: parent.width
                        Label { textFormat: Text.PlainText; text: root.shareUrl ? "SEND TO A DEVICE" : "YOUR DEVICES"; color: "#64746c"; textSize: Label.Small; font.weight: Font.DemiBold; width: parent.width - countLabel.width }
                        Label { textFormat: Text.PlainText; id: countLabel; text: root.state.devices.length + " devices"; color: "#64746c"; textSize: Label.Small }
                    }
                    Repeater {
                        model: root.state.devices
                        delegate: Rectangle {
                            width: content.width; height: units.gu(12); radius: units.gu(1.2); color: "white"
                            property var device: modelData
                            Rectangle {
                                x: units.gu(1.5); width: units.gu(6); height: width; anchors.verticalCenter: parent.verticalCenter
                                radius: units.gu(1.2); color: device.online ? "#e3efea" : "#eff1ee"
                                Icon { anchors.centerIn: parent; width: units.gu(3.5); height: width; name: "computer-symbolic"; color: device.online ? root.accent : "#849089" }
                            }
                            Column {
                                x: units.gu(9); anchors.verticalCenter: parent.verticalCenter; spacing: units.gu(0.8)
                                Label { textFormat: Text.PlainText; text: device.name; font.weight: Font.DemiBold; color: "#253d34" }
                                Label { textFormat: Text.PlainText; text: device.paired ? (device.online ? "Connected  ·  Paired" : "Offline  ·  Paired") : "Available to pair"; textSize: Label.Small; color: device.online ? root.accent : "#849089" }
                                Label { textFormat: Text.PlainText; visible: !!device.battery; text: device.battery ? device.battery.charge + "% battery" : ""; textSize: Label.XSmall; color: "#64746c" }
                            }
                            Icon { anchors { right: parent.right; rightMargin: units.gu(1.5); verticalCenter: parent.verticalCenter } width: units.gu(2); height: width; name: "go-next"; color: "#849089" }
                            MouseArea { anchors.fill: parent; onClicked: { root.selected = device; if (!device.paired) root.pairPopup = PopupUtils.open(pairDialog) } }
                        }
                    }
                    Label { textFormat: Text.PlainText; visible: root.state.devices.length === 0; width: parent.width; wrapMode: Text.WordWrap; text: "No devices yet. Open KDE Connect or GSConnect on another device on this Wi-Fi, then tap Refresh."; color: "#64746c" }
                }
                Column {
                    visible: root.selected !== null
                    width: parent.width; spacing: units.gu(1.5)
                    Row {
                        spacing: units.gu(1)
                        Button { text: "Ping"; enabled: !!root.selected && root.selected.online && !!root.selected.plugins.ping; onClicked: root.call("ping") }
                        Button { text: "Ring"; enabled: !!root.selected && root.selected.online && !!root.selected.plugins.findmyphone; onClicked: root.call("ring") }
                        Button { text: "Pair"; visible: !!root.selected && !root.selected.paired; onClicked: root.pairPopup = PopupUtils.open(pairDialog) }
                    }
                    Button { visible: !!root.shareUrl; text: "Send shared item"; color: root.accent; enabled: !!root.selected && root.selected.online && !!root.selected.plugins.share; onClicked: root.call("share", root.shareUrl) }
                    Label { textFormat: Text.PlainText; text: "ALLOW FOR THIS DEVICE"; color: "#64746c"; textSize: Label.Small; font.weight: Font.DemiBold }
                    Repeater {
                        model: ["ping","battery","notifications","clipboard","share","findmyphone","media","input"]
                        delegate: Rectangle {
                            width: content.width; height: units.gu(9); radius: units.gu(1); color: "white"
                            Column {
                                anchors { left: parent.left; right: toggle.left; margins: units.gu(1.5); verticalCenter: parent.verticalCenter } spacing: units.gu(0.5)
                                Label { textFormat: Text.PlainText; text: root.pluginLabels[modelData]; color: "#253d34"; font.weight: Font.DemiBold; textSize: Label.Small }
                                Label { textFormat: Text.PlainText; text: root.pluginNotes[modelData]; width: parent.width; wrapMode: Text.WordWrap; color: "#64746c"; textSize: Label.XSmall }
                            }
                            Switch {
                                id: toggle
                                anchors { right: parent.right; rightMargin: units.gu(1.5); verticalCenter: parent.verticalCenter }
                                checked: !!root.selected && !!root.selected.plugins[modelData]
                                enabled: !!root.selected && root.selected.paired && (modelData !== "input" || root.selected.online)
                                onClicked: root.call("SetPlugin", modelData, checked)
                            }
                        }
                    }
                    Button { text: "Unpair device"; color: "#b94c3c"; enabled: !!root.selected && root.selected.paired; onClicked: root.call("Unpair") }
                }
                Rectangle { width: parent.width; height: units.gu(0.15); color: "#dce2dc" }
                Label { textFormat: Text.PlainText; width: parent.width; text: "Wi-Fi only  ·  No cloud  ·  No telemetry"; color: root.accent; textSize: Label.Small; horizontalAlignment: Text.AlignHCenter }
                Label { textFormat: Text.PlainText; width: parent.width; text: connectBridge.error || root.state.event; wrapMode: Text.WordWrap; color: connectBridge.error ? "#b94c3c" : "#64746c"; textSize: Label.Small }
                Button { text: "Stop ringing"; onClicked: connectBridge.command("StopRinging", "", "", false) }
            }
        }
    }
    Loader { active: !previewMode; source: "ShareTarget.qml"; onLoaded: item.shared.connect(function(url) {root.shareUrl = url; root.selected = null}) }
    Component {
        id: pairDialog
        Dialog {
            id: pairing
            title: "Pair with " + (root.selected ? root.selected.name.replace(/[<>]/g, "") : "device")
            text: "Compare the code on both screens before you accept."
            Label { textFormat: Text.PlainText;
                width: parent.width
                text: root.selected && root.selected.version === 7 ? "Compare fingerprints" : (root.selected && root.selected.pending ? root.selected.pending.code : "Ready to request pairing")
                font.family: "monospace"; fontSize: "x-large"; color: root.accent; horizontalAlignment: Text.AlignHCenter
            }
            Label { textFormat: Text.PlainText; text: "PEER CERTIFICATE · SHA-256"; textSize: Label.XSmall; color: "#64746c" }
            Label { textFormat: Text.PlainText; width: parent.width; text: root.selected ? root.selected.fingerprint : ""; font.family: "monospace"; textSize: Label.XSmall; wrapMode: Text.WrapAnywhere }
            Label { textFormat: Text.PlainText; width: parent.width; text: root.selected && root.selected.version === 7 ? "Legacy protocol 7: compare certificate fingerprints; the other device may show a different code format." : "The request expires after 30 seconds."; wrapMode: Text.WordWrap; textSize: Label.Small }
            Button {
                text: root.selected && root.selected.pending ? (root.selected.pending.direction === "incoming" ? "Codes match · Accept" : "Waiting for other device…") : "Request pairing"
                color: root.accent
                enabled: !!root.selected && root.selected.online && (!root.selected.pending || root.selected.pending.direction === "incoming")
                onClicked: { root.call(root.selected.pending ? "AcceptPair" : "RequestPair", root.selected.fingerprint); if (previewMode) PopupUtils.close(pairing) }
            }
            Button { text: "Reject / Cancel"; onClicked: { if (root.selected) root.dismissedRequest = root.selected.id; root.call("Unpair"); PopupUtils.close(pairing); root.pairPopup = null } }
        }
    }
}
