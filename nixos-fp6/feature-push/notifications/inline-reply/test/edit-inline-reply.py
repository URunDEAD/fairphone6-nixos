#!/usr/bin/env python3
"""Applies the FP6 inline-reply edits to a patched Lomiri tree (argv[1]).

The tree is the full target's Lomiri source after every other module's
patches (test/patched-src.nix); test/mkpatch.sh diffs the result into
../lomiri-inline-reply.patch. Each substitution must match exactly once.
"""
import pathlib
import sys

root = pathlib.Path(sys.argv[1])


def sub(path, old, new):
    f = root / path
    s = f.read_text()
    n = s.count(old)
    if n != 1:
        sys.exit(f"{path}: expected 1 match, found {n}: {old[:80]!r}")
    f.write_text(s.replace(old, new))


# ---------------------------------------------------------------- bubble
N = "qml/Notifications/Notification.qml"
sub(N, "    property bool fp6WokeScreen: false\n", '''    property bool fp6WokeScreen: false

    // FP6 inline reply (nixos-fp6/feature-push/notifications/inline-reply):
    // lomiri-notifications turns the sender's "inline-reply" action into the
    // hint x-fp6-inline-reply (the button label) and notification.canReply.
    // The field is never shown while the lock screen is up: "Reply" asks for
    // the passcode first and opens the field after the unlock.
    property bool fp6Locked: false
    readonly property bool fp6CanReply: type !== Notification.PlaceHolder && type !== Notification.Confirmation &&
                                        !fullscreen && hints["x-fp6-inline-reply"] !== undefined &&
                                        !!notification.notification && notification.notification.canReply === true
    property bool fp6Replying: false
    property bool fp6ReplyAfterUnlock: false
    function fp6StartReply() {
        if (!fp6CanReply) return;
        notification.notification.holdForReply(true); // no expiry while typing
        if (fp6Locked) {
            fp6ReplyAfterUnlock = true;
            fp6UnlockTimeout.restart();
            var view = notification.ListView.view;
            if (view && view.fp6UnlockRequested) view.fp6UnlockRequested();
            return;
        }
        fp6Replying = true;
        fp6ReplyField.forceActiveFocus();
    }
    function fp6CancelReply() {
        fp6Replying = false;
        fp6ReplyAfterUnlock = false;
        fp6UnlockTimeout.stop();
        fp6ReplyField.text = "";
        if (notification.notification) notification.notification.holdForReply(false);
    }
    function fp6SendReply() {
        var text = fp6ReplyField.text;
        if (!fp6CanReply || fp6Locked || text.trim() === "") return;
        fp6Replying = false;
        fp6ReplyField.text = "";
        notification.notification.reply(text); // NotificationReplied, then closed
    }
    onFp6LockedChanged: {
        if (fp6Locked) {
            if (fp6Replying) fp6CancelReply(); // locked while typing: the draft goes
        } else if (fp6ReplyAfterUnlock) {
            fp6ReplyAfterUnlock = false;
            fp6UnlockTimeout.stop();
            fp6Replying = true;
            fp6ReplyField.forceActiveFocus();
        }
    }
    onFp6CanReplyChanged: if (!fp6CanReply) { fp6Replying = false; fp6ReplyAfterUnlock = false; }
    Timer {
        id: fp6UnlockTimeout
        interval: 30000 // passcode prompt abandoned: let the bubble expire again
        onTriggered: if (notification.fp6ReplyAfterUnlock) notification.fp6CancelReply()
    }
''')
# DND "hide popups": a replyable bubble is hidden but stays repliable from the panel.
sub(N, "            Qt.callLater(function() { if (notification.notification) notification.notification.close(); });\n",
    '''            Qt.callLater(function() {
                var n = notification.notification;
                if (!n) return;
                // FP6 inline reply: keep it open for the panel's Reply.
                if (n.canReply && fp6View.model && fp6View.model.fp6ParkNow &&
                        fp6View.model.fp6ParkNow(notification.notificationId)) return;
                n.close();
            });
''')
sub(N, '''                onTriggered: {
                    notification.notification.invokeAction(id)
                }
            }
''', '''                onTriggered: {
                    notification.notification.invokeAction(id)
                }
            }

            // FP6 inline reply: a Reply button, then the field (unlocked only).
            Row {
                objectName: "fp6ReplyButtonRow"
                anchors {
                    left: parent.left
                    right: parent.right
                }
                layoutDirection: Qt.RightToLeft
                visible: notification.fp6CanReply && !notification.fp6Replying

                NotificationButton {
                    objectName: "fp6ReplyButton"
                    width: parent.width / 2 - notification.margins / 2
                    text: notification.fp6Locked ? i18n.tr("Unlock to reply")
                                                 : (notification.hints["x-fp6-inline-reply"] || i18n.tr("Reply"))
                    color: theme.name == "Lomiri.Components.Themes.SuruDark" ? "#888" : "#666"
                    onClicked: notification.fp6StartReply()
                }
            }

            Row {
                objectName: "fp6ReplyRow"
                anchors {
                    left: parent.left
                    right: parent.right
                }
                spacing: notification.margins
                visible: notification.fp6Replying && !notification.fp6Locked

                TextField {
                    id: fp6ReplyField
                    objectName: "fp6ReplyField"
                    width: parent.width - fp6SendButton.width - fp6CancelButton.width - 2 * parent.spacing
                    height: units.gu(4)
                    placeholderText: notification.hints["x-kde-reply-placeholder-text"] || i18n.tr("Reply…")
                    maximumLength: 4096
                    onAccepted: notification.fp6SendReply()
                }
                NotificationButton {
                    id: fp6SendButton
                    objectName: "fp6SendButton"
                    width: units.gu(8)
                    text: notification.hints["x-kde-reply-submit-button-text"] || i18n.tr("Send")
                    color: theme.palette.normal.positive
                    opacity: fp6ReplyField.text.trim() !== "" ? 1 : 0.5
                    onClicked: notification.fp6SendReply()
                }
                NotificationButton {
                    id: fp6CancelButton
                    objectName: "fp6CancelButton"
                    width: units.gu(4)
                    iconName: "close"
                    color: theme.name == "Lomiri.Components.Themes.SuruDark" ? "#888" : "#666"
                    onClicked: notification.fp6CancelReply()
                }
            }
''')

# ------------------------------------------------------------ bubble list
L = "qml/Notifications/Notifications.qml"
sub(L, "    property var fp6Shade: null\n", '''    property var fp6Shade: null

    // FP6 inline reply (notifications/inline-reply): the lock state for the
    // bubbles, and the panel's access to replies (by notification id) for
    // the notifications the server still keeps open.
    property bool fp6Locked: false
    signal fp6UnlockRequested()
    readonly property QtObject fp6Reply: QtObject {
        property int rev: 0 // bumped whenever a notification starts/stops accepting replies
        function canReply(nid) {
            var m = notificationList.model;
            return !!m && !!m.fp6CanReply && nid > 0 && m.fp6CanReply(nid);
        }
        function reply(nid, text) {
            return canReply(nid) && notificationList.model.fp6Reply(nid, text);
        }
        function release(nid) {
            var m = notificationList.model;
            if (m && m.fp6Release && nid > 0) m.fp6Release(nid);
        }
    }
    Connections {
        target: notificationList.model
        ignoreUnknownSignals: true
        function onFp6ReplyableChanged() { notificationList.fp6Reply.rev++; }
    }
    // A card dismissed in the panel (swipe, group, Clear all) closes the
    // notification it kept open for replies.
    Connections {
        target: notificationList.fp6Shade
        function onRowsAboutToBeRemoved(parent, first, last) {
            for (var i = first; i <= last; i++) {
                var e = notificationList.fp6Shade.get(i);
                if (e && e.nid) notificationList.fp6Reply.release(e.nid);
            }
        }
    }
''')
sub(L, "    function fp6Record(type, hints, icon, summary, body) {\n",
    "    function fp6Record(type, hints, icon, summary, body, nid) {\n")
sub(L, '''                      summary: String(summary), body: String(body || ""), time: Date.now() };''',
    '''                      summary: String(summary), body: String(body || ""), time: Date.now(),
                      nid: Number(nid || 0), // FP6 inline reply
                      replyLabel: String(h["x-fp6-inline-reply"] || ""),
                      replyHint: String(h["x-kde-reply-placeholder-text"] || "") };''')
sub(L, "            notificationList.fp6Record(model.type, model.hints, model.icon, model.summary, model.body); // FP6\n",
    "            notificationList.fp6Record(model.type, model.hints, model.icon, model.summary, model.body, model.id); // FP6\n")
sub(L, "        background: notificationList.background\n",
    "        background: notificationList.background\n        fp6Locked: notificationList.fp6Locked // FP6 inline reply\n")

# ------------------------------------------------------------------ shell
S = "qml/Shell.qml"
sub(S, "            fp6Shade: fp6NotificationShade\n", '''            fp6Shade: fp6NotificationShade
            // FP6 inline reply: no typing on the lock screen; Reply unlocks first.
            fp6Locked: !!greeter && greeter.shown && greeter.locked
            onFp6UnlockRequested: if (greeter) greeter.notifyUserRequestedApp()
''')
sub(S, "            fp6HideNotificationContent: notifications.privacyMode\n",
    "            fp6HideNotificationContent: notifications.privacyMode\n"
    "            fp6NotificationReply: notifications.fp6Reply // FP6 inline reply\n")

P = "qml/Panel/Panel.qml"
sub(P, "    property bool fp6HideNotificationContent: false\n",
    "    property bool fp6HideNotificationContent: false\n"
    "    property QtObject fp6NotificationReply: null // FP6 inline reply (Notifications.qml fp6Reply)\n")
sub(P, "            hideNotificationContent: root.fp6HideNotificationContent\n",
    "            hideNotificationContent: root.fp6HideNotificationContent\n"
    "            notificationReply: root.fp6NotificationReply\n")

C = "qml/Panel/QuickSettings/ControlCentre.qml"
sub(C, "    property bool hideNotificationContent: false\n",
    "    property bool hideNotificationContent: false\n"
    "    property QtObject notificationReply: null // FP6 inline reply\n")
sub(C, "                hideContent: cc.hideNotificationContent\n",
    "                hideContent: cc.hideNotificationContent\n"
    "                replyBackend: cc.notificationReply\n"
    "                locked: cc.locked\n"
    "                onReplyUnlockRequested: cc.unlockRequested()\n")

# ------------------------------------------------------------ panel cards
Q = "qml/Panel/QuickSettings/QsNotifications.qml"
sub(Q, "    property bool historyAvailable: false\n", '''    property bool historyAvailable: false
    // FP6 inline reply (notifications/inline-reply): Notifications.qml fp6Reply.
    property QtObject replyBackend: null
    property bool locked: false
    signal replyUnlockRequested()
''')
sub(Q, "                    height: Math.max(units.gu(6.5), textCol.height + units.gu(2))\n",
    "                    height: Math.max(units.gu(6.5), textCol.height + units.gu(2)\n"
    "                                     + (replyBox.visible ? replyBox.height + units.gu(0.75) : 0))\n")
sub(Q, '''                        onReleased: {
                            if (Math.abs(card.x) > card.width * 0.35) {
                                card.x = card.x > 0 ? card.width : -card.width;
                                dismissTimer.start();
                            } else {
                                card.x = 0;
                            }
                        }
                    }
''', '''                        onReleased: {
                            if (Math.abs(card.x) > card.width * 0.35) {
                                card.x = card.x > 0 ? card.width : -card.width;
                                dismissTimer.start();
                            } else {
                                card.x = 0;
                            }
                        }
                    }

                    // FP6 inline reply: while the sender still accepts one;
                    // typing only when unlocked (the button asks for the passcode).
                    Item {
                        id: replyBox
                        objectName: "qsReplyBox"
                        readonly property bool replyable: !!list.replyBackend && model.nid > 0 &&
                                                          list.replyBackend.rev >= 0 && list.replyBackend.canReply(model.nid)
                        property bool editing: false
                        visible: replyable
                        anchors { left: textCol.left; right: textCol.right; top: textCol.bottom; topMargin: units.gu(0.75) }
                        height: visible ? units.gu(4) : 0
                        onReplyableChanged: if (!replyable) editing = false
                        Connections {
                            target: list
                            function onLockedChanged() { if (list.locked) { replyBox.editing = false; replyField.text = ""; } }
                        }
                        function send() {
                            var text = replyField.text;
                            if (text.trim() === "" || list.locked) return;
                            if (list.replyBackend.reply(model.nid, text)) {
                                replyField.text = "";
                                editing = false;
                                dismissTimer.start(); // replied: the card goes
                            }
                        }
                        AbstractButton {
                            objectName: "qsReplyButton"
                            visible: !replyBox.editing || list.locked
                            anchors.right: parent.right
                            width: replyLabel.width + units.gu(2)
                            height: parent.height
                            onClicked: {
                                if (list.locked) { list.replyUnlockRequested(); return; }
                                replyBox.editing = true;
                                replyField.forceActiveFocus();
                            }
                            Label {
                                id: replyLabel
                                objectName: "qsReplyLabel"
                                anchors.centerIn: parent
                                text: list.locked ? i18n.tr("Unlock to reply") : (model.replyLabel || i18n.tr("Reply"))
                                textSize: Label.Small
                                color: theme.palette.normal.focus
                            }
                        }
                        Row {
                            visible: replyBox.editing && !list.locked
                            anchors.fill: parent
                            spacing: units.gu(1)
                            TextField {
                                id: replyField
                                objectName: "qsReplyField"
                                width: parent.width - sendButton.width - parent.spacing
                                height: parent.height
                                placeholderText: model.replyHint || i18n.tr("Reply…")
                                maximumLength: 4096
                                onAccepted: replyBox.send()
                            }
                            AbstractButton {
                                id: sendButton
                                objectName: "qsReplySend"
                                width: sendLabel.width + units.gu(2)
                                height: parent.height
                                enabled: replyField.text.trim() !== ""
                                onClicked: replyBox.send()
                                Label {
                                    id: sendLabel
                                    anchors.centerIn: parent
                                    text: i18n.tr("Send")
                                    textSize: Label.Small
                                    color: theme.palette.normal.focus
                                    opacity: sendButton.enabled ? 1 : 0.5
                                }
                            }
                        }
                    }
''')
print("inline-reply edits applied", file=sys.stderr)
