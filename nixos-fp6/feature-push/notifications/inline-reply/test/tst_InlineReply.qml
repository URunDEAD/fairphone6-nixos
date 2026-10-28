// Laptop test of inline replies in the shell (offscreen, 1116x2484, 24 px/gu).
// Real QML from the target's patched Lomiri tree with lomiri-inline-reply.patch:
// Notifications/Notifications.qml + Notification.qml (bubbles) and
// Panel/QuickSettings/QsNotifications.qml (the pulled-down panel's cards),
// on the real x86 Lomiri.Notifications plugin (the patched server, on a
// private session bus). run-tests.sh sends the notifications with
// fp6-notify-reply, one after the other, and checks what each client got:
//   1 "Test Contact 1": locked -> "Unlock to reply" -> unlock -> field -> Send
//   2 "Test Contact 2": hidden content while locked, bubble expires (parked),
//     panel card: "Unlock to reply" while locked, then reply from the panel
//   3 "Test Contact 3": expires, "Clear all" in the panel closes it
//   4 "Test Contact 4": Do Not Disturb hides the bubble, still repliable
import QtQuick 2.15
import QtTest 1.0
import Lomiri.Components 1.3
import Lomiri.Notifications 1.0 as LN
import "Notifications"
import "Panel/QuickSettings"
import "Shot.js" as Shot

Item {
    id: root
    width: 1116
    height: 2484

    property int unlockRequests: 0
    property int panelUnlockRequests: 0

    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            GradientStop { position: 0; color: "#5E2750" }
            GradientStop { position: 1; color: "#2C001E" }
        }
    }

    ListModel { id: shade }

    Notifications {
        id: notifications
        anchors { left: parent.left; right: parent.right; top: parent.top; topMargin: units.gu(3) }
        height: units.gu(40)
        model: LN.Model
        margin: units.gu(1)
        fp6Shade: shade
        privacyMode: false
        onFp6UnlockRequested: root.unlockRequests++
    }

    Rectangle {
        id: panel
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        height: units.gu(52)
        color: "#E61E1E1E"
        QsNotifications {
            id: qs
            x: units.gu(1.5)
            y: units.gu(1.5)
            width: parent.width - units.gu(3)
            model: shade
            replyBackend: notifications.fp6Reply
            locked: false
            onReplyUnlockRequested: root.panelUnlockRequests++
        }
    }

    function find(item, name) {
        if (!item) return null;
        if (item.objectName === name) return item;
        var kids = [];
        if (item.contentItem && item.contentItem !== item) kids.push(item.contentItem);
        for (var i = 0; i < (item.children ? item.children.length : 0); i++) kids.push(item.children[i]);
        for (var j = 0; j < kids.length; j++) {
            var r = find(kids[j], name);
            if (r) return r;
        }
        return null;
    }
    function bubbleFor(summary) {
        for (var i = 0; i < 4; i++) {
            var b = find(notifications, "notification" + i);
            if (b && b.summary === summary) return b;
        }
        return null;
    }
    function shadeIndex(summary) {
        for (var i = 0; i < shade.count; i++) if (shade.get(i).summary === summary) return i;
        return -1;
    }
    function cardButton(name) { return find(qs, name); }
    // Leave only this notification's card in the panel (earlier rounds' cards).
    function keepOnly(summary) {
        for (var i = shade.count - 1; i >= 0; i--) if (shade.get(i).summary !== summary) shade.remove(i);
    }

    TestCase {
        id: tc
        name: "InlineReply"
        when: windowShown

        function shot(name) {
            waitForRendering(root);
            grabImage(root).save(Shot.out + "/" + name + ".png");
            console.log("shot " + name);
        }

        function test_1_bubble_reply_after_unlock() {
            notifications.fp6Locked = true;
            var b = null;
            tryVerify(function() { b = bubbleFor("Test Contact 1"); return b !== null; }, 60000, "notification 1 arrived");
            tryCompare(b, "fp6CanReply", true);
            var nid = b.notificationId;
            var button = find(b, "fp6ReplyButton");
            verify(button.visible);
            compare(button.text, "Unlock to reply");
            verify(!find(b, "fp6ReplyRow").visible);
            wait(300);
            shot("bubble-locked");

            mouseClick(button);
            compare(root.unlockRequests, 1, "Reply on the lock screen asks for the passcode");
            verify(b.fp6ReplyAfterUnlock);
            verify(!find(b, "fp6ReplyRow").visible, "no field while locked");
            wait(9000); // longer than the 8 s display time: held while unlocking
            verify(LN.Model.getRaw(nid) !== null, "bubble held while the passcode is asked");

            notifications.fp6Locked = false; // unlocked
            var row = find(b, "fp6ReplyRow");
            tryCompare(row, "visible", true);
            verify(!find(b, "fp6ReplyButtonRow").visible);
            var field = find(b, "fp6ReplyField");
            compare(field.placeholderText, "Reply to Test Contact");
            field.text = "On my way";
            wait(300);
            shot("bubble-reply-field");

            // Locking while typing drops the draft; Reply again after unlock.
            notifications.fp6Locked = true;
            tryCompare(row, "visible", false);
            compare(field.text, "");
            notifications.fp6Locked = false;
            mouseClick(find(b, "fp6ReplyButton"));
            tryCompare(row, "visible", true);
            field.text = "On my way";
            mouseClick(find(b, "fp6SendButton"));
            tryVerify(function() { return LN.Model.getRaw(nid) === null; }, 5000, "closed after the reply");
            verify(!LN.Model.fp6CanReply(nid));
            // The panel card of a replied notification offers no reply any more.
            tryCompare(cardButton("qsReplyBox"), "visible", false);
        }

        function test_2_hidden_content_then_panel_reply() {
            notifications.fp6Locked = true;
            notifications.privacyMode = true; // "hide notification content while locked"
            qs.locked = true;
            qs.hideContent = true;
            var b = null;
            tryVerify(function() { b = bubbleFor("Test Contact 2"); return b !== null; }, 60000, "notification 2 arrived");
            var nid = b.notificationId;
            verify(find(b, "privacySummaryLabel").visible, "content hidden");
            compare(find(b, "fp6ReplyButton").text, "Unlock to reply");
            verify(!find(b, "fp6ReplyRow").visible);
            wait(300);
            shot("bubble-locked-hidden");

            // Not answered: the bubble times out (8 s) but stays open for replies.
            tryVerify(function() { return LN.Model.getRaw(nid) === null; }, 15000, "bubble expired");
            verify(LN.Model.fp6CanReply(nid), "parked: still repliable");
            keepOnly("Test Contact 2");
            var i = shadeIndex("Test Contact 2");
            verify(i >= 0);
            compare(shade.get(i).nid, nid);
            var box = cardButton("qsReplyBox");
            tryCompare(box, "visible", true);
            var rb = cardButton("qsReplyButton");
            compare(cardButton("qsReplyLabel").text, "Unlock to reply");
            wait(300);
            shot("panel-locked");
            mouseClick(rb);
            compare(root.panelUnlockRequests, 1);
            verify(!box.editing || !cardButton("qsReplyField").visible, "no field while locked");

            notifications.fp6Locked = false;
            notifications.privacyMode = false;
            qs.locked = false;
            qs.hideContent = false;
            mouseClick(cardButton("qsReplyButton"));
            var field = cardButton("qsReplyField");
            tryCompare(field, "visible", true);
            field.text = "Sent from the panel";
            wait(300);
            shot("panel-reply");
            mouseClick(cardButton("qsReplySend"));
            tryVerify(function() { return shadeIndex("Test Contact 2") < 0; }, 5000, "card removed after the reply");
            verify(!LN.Model.fp6CanReply(nid));
        }

        function test_3_clear_all_closes() {
            tryVerify(function() { return shadeIndex("Test Contact 3") >= 0; }, 60000, "notification 3 arrived");
            var nid = shade.get(shadeIndex("Test Contact 3")).nid;
            tryVerify(function() { return LN.Model.getRaw(nid) === null; }, 15000, "bubble expired");
            verify(LN.Model.fp6CanReply(nid));
            shade.clear(); // "Clear all"
            verify(!LN.Model.fp6CanReply(nid), "released: the sender gets NotificationClosed");
        }

        function test_4_dnd_hidden_bubble_stays_repliable() {
            notifications.fp6Dnd.dndHidePopups = true;
            notifications.fp6Dnd.dndActive = true;
            tryVerify(function() { return shadeIndex("Test Contact 4") >= 0; }, 60000, "notification 4 recorded");
            var nid = shade.get(shadeIndex("Test Contact 4")).nid;
            tryVerify(function() { return LN.Model.getRaw(nid) === null; }, 3000, "no bubble under DND");
            verify(LN.Model.fp6CanReply(nid), "parked instead of closed");
            verify(notifications.fp6Reply.reply(nid, "Reply while DND"));
            notifications.fp6Dnd.dndActive = false;
        }
    }
}
