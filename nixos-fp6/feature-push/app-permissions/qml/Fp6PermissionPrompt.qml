/*
 * FP6 app permissions (nixos-fp6/feature-push/app-permissions): the
 * runtime-permission prompt ("Allow Morph Browser to access a USB device?"
 * Allow / Allow only while using the app / Don't allow; for the camera,
 * the microphone and location: Allow / Only this time / Don't allow) and
 * the focus hook for "only while using".
 *
 * fp6-permission-agent (session action group com.fairphone6.Permissions)
 * publishes the open question as the "prompt" state (JSON, "{}" when there
 * is none). The shell shows it while unlocked, tells the agent it did
 * ("ack"; otherwise the agent falls back to a snap-decision notification
 * after 1.5 s) and sends the answer ("respond": "<id> allow|while_using|
 * once|deny|dismissed"). The focused app id goes to "focus" ("" while the lock
 * screen shows).
 *
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15
import Lomiri.Components 1.3
import QMenuModel 1.0

Item {
    id: root
    objectName: "fp6PermissionPrompt"

    // Set by the shell.
    property bool locked: false
    property string focusedAppId: ""

    QDBusActionGroup {
        id: agent
        busType: DBus.SessionBus
        busName: "com.fairphone6.Permissions"
        objectPath: "/com/fairphone6/Permissions"
        property variant promptAction: action("prompt")
        property variant ackAction: action("ack")
        property variant respondAction: action("respond")
        property variant focusAction: action("focus")
        Component.onCompleted: start()
    }

    readonly property var req: {
        if (!agent.promptAction || !agent.promptAction.valid) return ({});
        try { return JSON.parse(String(agent.promptAction.state)) || ({}); } catch (e) { return ({}); }
    }
    readonly property bool pending: req.id !== undefined
    readonly property bool showing: pending && !locked
    property var answeredId: -1
    // Portal access dialogs offer only Allow / Don't allow, with their labels.
    readonly property bool offerWhileUsing: !req.options || req.options.indexOf("while_using") >= 0
    readonly property bool offerOnce: !!req.options && req.options.indexOf("once") >= 0

    // ---- focus hook --------------------------------------------------------
    readonly property string reportedFocus: locked ? "" : focusedAppId
    readonly property bool focusValid: !!(agent.focusAction && agent.focusAction.valid)
    function sendFocus() { if (focusValid) agent.focusAction.activate(reportedFocus); }
    onReportedFocusChanged: sendFocus()
    onFocusValidChanged: sendFocus()   // agent (re)started

    onShowingChanged: {
        if (showing && agent.ackAction && agent.ackAction.valid) agent.ackAction.activate(String(req.id));
    }
    onReqChanged: {
        if (showing && agent.ackAction && agent.ackAction.valid) agent.ackAction.activate(String(req.id));
    }

    function respond(answer) {
        if (!pending || answeredId === req.id) return;
        answeredId = req.id;
        if (agent.respondAction && agent.respondAction.valid) agent.respondAction.activate(String(req.id) + " " + answer);
    }

    visible: showing && answeredId !== req.id
    enabled: visible

    // Dim the shell; taps outside the card do nothing (no accidental
    // answers), as on Android.
    Rectangle {
        anchors.fill: parent
        color: "black"
        opacity: root.visible ? 0.5 : 0
        Behavior on opacity { LomiriNumberAnimation {} }
        MouseArea { anchors.fill: parent; onWheel: wheel.accepted = true }
    }

    Rectangle {
        id: card
        objectName: "fp6PermissionCard"
        anchors.centerIn: parent
        width: Math.min(parent.width - units.gu(4), units.gu(46))
        height: column.height + units.gu(4)
        radius: units.gu(2)
        color: theme.palette.normal.overlay
        border.width: units.dp(1)
        border.color: theme.palette.normal.base

        Column {
            id: column
            anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
            spacing: units.gu(1.5)

            Item {
                width: parent.width
                height: units.gu(6)
                Image {
                    id: appIcon
                    anchors.centerIn: parent
                    width: units.gu(5.5); height: width
                    sourceSize { width: width; height: height }
                    source: root.req.icon || ""
                    visible: status === Image.Ready
                    fillMode: Image.PreserveAspectFit
                }
                Icon {
                    anchors.centerIn: parent
                    width: units.gu(5); height: width
                    visible: !appIcon.visible
                    name: {
                        switch (root.req.perm) {
                        case "usb": return "stock_usb";
                        case "bluetooth": return "bluetooth-active";
                        case "contacts": return "contact";
                        case "calendar": return "calendar";
                        case "messages": return "message";
                        case "files": return "document-open";
                        case "sensors": return "sensors";
                        case "camera": return "camera-app-symbolic";
                        case "microphone": return "audio-input-microphone-symbolic";
                        case "location": return "location";
                        default: return "security-alert";
                        }
                    }
                    color: theme.palette.normal.overlayText
                }
            }

            Label {
                objectName: "fp6PermissionQuestion"
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                textSize: Label.Large
                color: theme.palette.normal.overlayText
                text: root.req.question || ""
            }
            Label {
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                textSize: Label.Small
                color: theme.palette.normal.overlaySecondaryText
                visible: text !== ""
                text: root.req.detail || ""
            }
            Item { width: 1; height: units.gu(0.5) }

            Button {
                objectName: "fp6PermissionAllow"
                width: parent.width
                color: theme.palette.normal.positive
                text: root.req.grant_label || i18n.tr("Allow")
                onClicked: root.respond("allow")
            }
            Button {
                objectName: "fp6PermissionWhileUsing"
                width: parent.width
                visible: root.offerWhileUsing
                text: i18n.tr("Allow only while using the app")
                onClicked: root.respond("while_using")
            }
            Button {
                objectName: "fp6PermissionOnce"
                width: parent.width
                visible: root.offerOnce
                text: i18n.tr("Only this time")
                onClicked: root.respond("once")
            }
            Button {
                objectName: "fp6PermissionDeny"
                width: parent.width
                text: root.req.deny_label || i18n.tr("Don't allow")
                onClicked: root.respond("deny")
            }
            Label {
                width: parent.width
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                textSize: Label.XSmall
                color: theme.palette.normal.overlaySecondaryText
                visible: root.offerWhileUsing || root.offerOnce
                text: i18n.tr("You can change this in System Settings > Security & Privacy > App permissions.")
            }
        }
    }
}
