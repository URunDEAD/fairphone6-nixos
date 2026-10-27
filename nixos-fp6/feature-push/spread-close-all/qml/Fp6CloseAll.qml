/*
 * FP6: close all apps from the app switcher (Lomiri's spread).
 * nixos-fp6/feature-push/spread-close-all, installed as qml/Stage/Spread/Fp6CloseAll.qml
 *
 *  * Gesture: a left-to-right drag that STARTS while the spread is already at
 *    its leftmost end (the first app at the start; always the case when all
 *    cards fit on screen) overscrolls: the cards follow the finger (the closable
 *    ones fade). Past `threshold` "Release to close all apps" appears with a
 *    haptic tick; releasing there closes all apps, releasing before springs
 *    back. A drag that starts anywhere else only scrolls, also when it
 *    reaches the start; a drag that first goes right-to-left is a normal
 *    scroll for the rest of that touch.
 *  * "Clear all" button at the bottom (Android-like), same close path.
 *  * Close path = the per-card swipe-up close (appDelegate.close() ->
 *    Window::close: the surface is asked to close, qtmir force-closes it
 *    after 3 s, a window without surface closes its application); apps that
 *    still have a window 2 s later get ApplicationManager.stopApplication().
 *  * Never closed: the dialer while a call is in progress; with
 *    "Keep 'ready' apps when closing all" (default on) the apps in App
 *    Launch's keep-ready / prelaunch lists. The shell, the OSK, indicators
 *    and services are not windows of the spread.
 *  * Settings: com.fairphone6.app-switcher close-all-gesture (b, default
 *    true) and close-all-keep-ready (b, default true); a missing schema
 *    means the defaults.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.15
import Lomiri.Components 1.3
import GSettings 1.0

Item {
    id: root

    // Environment (set by Stage.qml)
    property Item spread: null          // the Spread item (contentMargin)
    property Item flickable: null       // the spread's FloatingFlickable (fp6 drag hooks)
    property var repeater: null         // appRepeater: itemAt(i).appId, .close()
    property var applicationManager: null
    property bool active: false         // the spread is on screen
    property bool callActive: false     // a call is in progress

    // Emitted once the closed apps are gone (or after 2 s): leave the spread, go home.
    signal finished()

    // Outputs for the app cards (Stage.qml spread state)
    readonly property real offset: d.offset
    readonly property real fade: d.fade
    readonly property bool closing: d.closing
    readonly property bool dragging: d.armed
    readonly property bool beyondThreshold: d.beyond

    // A clear distance: 40 % of the width, 16-30 grid units.
    readonly property real threshold: Math.max(units.gu(16), Math.min(width * 0.4, units.gu(30)))

    readonly property bool gestureEnabled: switcherSettings.closeAllGesture !== false
    readonly property bool keepReadyApps: switcherSettings.closeAllKeepReady !== false

    // Haptic ticks played (tests read it).
    property int hapticCount: 0

    GSettings {
        id: switcherSettings
        schema.id: "com.fairphone6.app-switcher"
    }
    GSettings {
        id: appReady
        schema.id: "com.fairphone6.app-ready"
    }

    readonly property var readyApps: {
        var out = [];
        var lists = [appReady.keepReady || [], appReady.prelaunch || []];
        for (var l = 0; l < lists.length; l++) {
            for (var i = 0; i < lists[l].length; i++) {
                out.push(String(lists[l][i]));
            }
        }
        return out;
    }

    function isClosable(appId) {
        if (!appId) {
            return false;
        }
        var id = String(appId).split('_')[0];
        if (callActive && d.dialerIds.indexOf(id) >= 0) {
            return false;
        }
        if (keepReadyApps && readyApps.indexOf(id) >= 0) {
            return false;
        }
        return true;
    }

    readonly property int windowCount: repeater ? repeater.count : 0
    readonly property int closableCount: {
        var n = 0;
        for (var i = 0; i < windowCount; i++) {
            var item = repeater.itemAt(i);
            if (item && isClosable(item.appId)) {
                n++;
            }
        }
        return n;
    }
    readonly property int keptCount: {
        var n = 0;
        for (var i = 0; i < windowCount; i++) {
            var item = repeater.itemAt(i);
            if (item && item.appId && !isClosable(item.appId)) {
                n++;
            }
        }
        return n;
    }

    QtObject {
        id: d
        property real offset: 0
        property real fade: 0
        property bool pressOk: false
        property bool armed: false
        property real startX: 0     // where the finger came down
        property bool beyond: false
        property bool closing: false
        property var pending: []
        readonly property var dialerIds: ["lomiri-dialer-app", "dialer-app"]
    }

    function atStart() {
        return flickable !== null && Math.abs(flickable.contentX) <= 1;
    }

    function canArm() {
        return active && gestureEnabled && !d.closing && closableCount > 0 && atStart();
    }

    function setOffset(dx) {
        var o = Math.max(0, dx);
        if (o > threshold) {
            o = threshold + (o - threshold) * 0.4;   // resistance past the threshold
        }
        d.offset = o;
        d.fade = 0.65 * Math.min(1, o / threshold);
        var beyond = dx >= threshold;
        if (beyond !== d.beyond) {
            d.beyond = beyond;
            if (beyond) {
                tick();
            }
        }
    }

    function tick() {
        hapticCount++;
        try {
            Haptics.play({ duration: 10 });   // hfd adds the FP6's +15 ms (haptics module)
        } catch (e) {
        }
    }

    function reset() {
        springBack.stop();
        closeAnimation.stop();
        d.armed = false;
        d.pressOk = false;
        d.beyond = false;
        d.offset = 0;
        d.fade = 0;
    }

    function closeAll() {
        if (d.closing) {
            return;
        }
        if (closableCount === 0) {
            springBack.start();
            return;
        }
        springBack.stop();
        d.armed = false;
        d.beyond = false;
        d.closing = true;
        closeAnimation.start();
    }

    function closeWindows() {
        var items = [];
        var ids = [];
        for (var i = windowCount - 1; i >= 0; i--) {
            var item = repeater.itemAt(i);
            if (!item || !isClosable(item.appId)) {
                continue;
            }
            items.push(item);
            if (ids.indexOf(item.appId) < 0) {
                ids.push(item.appId);
            }
        }
        console.info("fp6-close-all: closing " + items.length + " window(s): " + ids.join(" "));
        d.pending = ids;
        for (var j = 0; j < items.length; j++) {
            try {
                items[j].close();
            } catch (e) {
                // the delegate went away with an earlier window of the same app
            }
        }
        waitTimer.elapsed = 0;
        waitTimer.start();
    }

    function remainingPending() {
        var left = [];
        for (var i = 0; i < windowCount; i++) {
            var item = repeater.itemAt(i);
            if (item && d.pending.indexOf(item.appId) >= 0 && left.indexOf(item.appId) < 0) {
                left.push(item.appId);
            }
        }
        return left;
    }

    function finish() {
        waitTimer.stop();
        var left = remainingPending();
        if (left.length > 0) {
            console.info("fp6-close-all: still open after the close request: " + left.join(" "));
            stopTimer.apps = left;
            stopTimer.start();
        }
        var wasActive = active;
        d.closing = false;
        if (wasActive) {
            root.finished();
        }
        Qt.callLater(root.reset);
    }

    Timer {
        id: waitTimer
        property int elapsed: 0
        interval: 100
        repeat: true
        onTriggered: {
            elapsed += interval;
            if (root.remainingPending().length === 0 || elapsed >= 2000) {
                root.finish();
            }
        }
    }

    // Apps that ignored the close request and still have a window.
    Timer {
        id: stopTimer
        property var apps: []
        interval: 2000
        onTriggered: {
            var left = root.remainingPending();
            for (var i = 0; i < apps.length; i++) {
                if (left.indexOf(apps[i]) < 0 || apps[i] === "xwayland.qtmir" || !root.isClosable(apps[i])) {
                    continue;
                }
                console.info("fp6-close-all: stopApplication " + apps[i]);
                if (root.applicationManager && root.applicationManager.stopApplication) {
                    root.applicationManager.stopApplication(apps[i]);
                }
            }
            apps = [];
            d.pending = [];
        }
    }

    ParallelAnimation {
        id: springBack
        NumberAnimation { target: d; property: "offset"; to: 0; duration: 320; easing.type: Easing.OutBack; easing.overshoot: 1.2 }
        NumberAnimation { target: d; property: "fade"; to: 0; duration: 200 }
        onStopped: d.beyond = false
    }

    SequentialAnimation {
        id: closeAnimation
        ParallelAnimation {
            NumberAnimation { target: d; property: "offset"; to: root.width; duration: 220; easing.type: Easing.InQuad }
            NumberAnimation { target: d; property: "fade"; to: 1; duration: 220 }
        }
        ScriptAction { script: root.closeWindows() }
    }

    onActiveChanged: {
        if (!active && !d.closing) {
            reset();
        }
    }

    Connections {
        target: root.flickable
        ignoreUnknownSignals: true

        // Where was the list when the finger came down?
        function onFp6PressedChanged() {
            if (root.flickable.fp6Pressed) {
                // The SwipeArea's touchPosition is the touch-down point here.
                d.pressOk = root.canArm() && !root.flickable.moving;
                d.startX = root.flickable.fp6TouchPosition.x;
            }
        }
        // The flickable's SwipeArea recognised a horizontal drag (before the
        // inner Flickable gets its press).
        function onFp6DragStarted(position) {
            d.armed = d.pressOk && root.canArm();
            d.pressOk = false;
            if (d.armed) {
                springBack.stop();
                root.flickable.boundsBehavior = Flickable.StopAtBounds;
                root.setOffset(position.x - d.startX);
            }
        }
        function onFp6TouchPositionChanged() {
            if (!d.armed) {
                return;
            }
            var dx = root.flickable.fp6TouchPosition.x - d.startX;
            if (dx < -units.gu(1)) {
                // Right-to-left first: an ordinary scroll for the rest of this touch.
                d.armed = false;
                springBack.start();
                return;
            }
            root.setOffset(dx);
        }
        function onFp6DragEnded(position) {
            root.flickable.boundsBehavior = Flickable.DragAndOvershootBounds;
            if (!d.armed) {
                return;
            }
            d.armed = false;
            if (d.beyond) {
                root.closeAll();
            } else {
                springBack.start();
            }
        }
    }

    // Placed in the spread's bottom margin, below the cards' app titles.
    readonly property real bottomBandCenter: height - (spread ? spread.contentMargin : units.gu(8)) / 2

    AbstractButton {
        id: clearAllButton
        objectName: "fp6ClearAllButton"
        readonly property bool shown: root.active && root.closableCount > 0 && !d.closing && !d.armed && Math.abs(d.offset) < 1
        anchors.horizontalCenter: parent.horizontalCenter
        height: Math.min(units.gu(5), (spread ? spread.contentMargin : units.gu(8)) * 0.8)
        width: clearAllLabel.implicitWidth + units.gu(5)
        y: root.bottomBandCenter - height / 2
        enabled: shown
        opacity: shown ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { LomiriNumberAnimation { duration: LomiriAnimation.SnapDuration } }
        onClicked: root.closeAll()

        Rectangle {
            anchors.fill: parent
            radius: height / 2
            color: clearAllButton.pressed ? "#66FFFFFF" : "#33FFFFFF"
            border.color: "#80FFFFFF"
            border.width: units.dp(1)
        }
        Label {
            id: clearAllLabel
            anchors.centerIn: parent
            text: i18n.tr("Clear all")
            color: "#FFFFFF"
            fontSize: "medium"
            font.weight: Font.Medium
        }
    }

    Column {
        id: releaseHint
        objectName: "fp6ReleaseHint"
        anchors.horizontalCenter: parent.horizontalCenter
        y: root.bottomBandCenter - releaseLabel.height / 2
        spacing: units.gu(0.5)
        opacity: d.armed && d.beyond ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { LomiriNumberAnimation { duration: LomiriAnimation.SnapDuration } }

        Label {
            id: releaseLabel
            objectName: "fp6ReleaseLabel"
            anchors.horizontalCenter: parent.horizontalCenter
            text: i18n.tr("Release to close all apps")
            color: "#FFFFFF"
            fontSize: "large"
            font.weight: Font.Medium
        }
        Label {
            anchors.horizontalCenter: parent.horizontalCenter
            visible: root.keptCount > 0
            text: i18n.tr("%1 app stays open", "%1 apps stay open", root.keptCount).arg(root.keptCount)
            color: "#CCFFFFFF"
            fontSize: "small"
        }
    }
}
