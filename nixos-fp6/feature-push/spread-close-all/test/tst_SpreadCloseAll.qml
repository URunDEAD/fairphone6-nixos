// Laptop test of the spread close-all (offscreen, 1116x2484, 24 px/gu).
// Real QML from the target's patched Lomiri tree: Stage/Spread/Spread.qml,
// SpreadMaths.qml, Components/FloatingFlickable.qml (with the fp6 hooks) and
// Stage/Spread/Fp6CloseAll.qml. Lomiri's QtMir.Application mock is not built
// by nixpkgs, so Stage.qml itself cannot run: the cards below repeat its
// spread-state bindings (x/opacity from fp6CloseAll, SpreadMaths) with
// coloured rectangles, and close() removes the card ~150 ms later like an
// app closing its window. Touches are real QTouchEvents through the
// FloatingFlickable's SwipeArea. Run by run-tests.sh.
import QtQuick 2.15
import QtTest 1.0
import Lomiri.Components 1.3
import Lomiri.Test 1.0 as LomiriTest
import GSettings 1.0
import "Stage/Spread"
import "Components"
import "Shot.js" as Shot

Item {
    id: root
    width: 1116
    height: 2484

    property bool callActive: false
    property int finishedCount: 0
    property var closeRequests: []
    property var stopRequests: []

    Rectangle {
        anchors.fill: parent
        gradient: Gradient {
            GradientStop { position: 0; color: "#2C3E50" }
            GradientStop { position: 1; color: "#4B3B68" }
        }
    }

    GSettings { id: switcherSettings; schema.id: "com.fairphone6.app-switcher" }
    GSettings { id: appReady; schema.id: "com.fairphone6.app-ready" }

    ListModel { id: apps }
    readonly property var palette: ["#E95420", "#3EB34F", "#0E8CBA", "#7A3E9D", "#C7162B", "#F99B11", "#19B6EE", "#5E2750", "#38B44A", "#E5E5E5"]

    QtObject {
        id: surfaceList
        property int count: apps.count
        function surfaceAt(i) { return null; }
        function applicationAt(i) { return i >= 0 && i < apps.count ? { name: apps.get(i).name } : null; }
    }

    QtObject {
        id: fakeApplicationManager
        function stopApplication(appId) { root.stopRequests.push(appId); root.removeApp(appId); return true; }
    }

    function removeApp(appId) {
        for (var i = 0; i < apps.count; i++) {
            if (apps.get(i).appId === appId) { apps.remove(i); return; }
        }
    }

    Component {
        id: closeTimer
        Timer {
            property string appId
            interval: 150
            running: true
            onTriggered: { root.removeApp(appId); destroy(); }
        }
    }

    Spread {
        id: spreadItem
        anchors.fill: parent
        model: surfaceList
        spreadFlickable: floatingFlickable
        z: 100

        FloatingFlickable {
            id: floatingFlickable
            objectName: "spreadFlickable"
            anchors.fill: parent
            enabled: true
            contentWidth: spreadItem.spreadTotalWidth
        }

        Fp6CloseAll {
            id: fp6CloseAll
            objectName: "fp6CloseAll"
            anchors.fill: parent
            spread: spreadItem
            flickable: floatingFlickable
            repeater: cards
            applicationManager: fakeApplicationManager
            active: true
            callActive: root.callActive
            onFinished: root.finishedCount++
        }
    }

    Repeater {
        id: cards
        model: apps
        delegate: Item {
            id: card
            property string appId: model.appId
            // "stubborn-app" ignores close requests (an app with an unsaved-changes dialog)
            function close() {
                root.closeRequests.push(appId);
                if (appId !== "stubborn-app") {
                    closeTimer.createObject(root, { appId: appId });
                }
            }
            SpreadMaths {
                id: spreadMaths
                spread: spreadItem
                itemIndex: index
                flickable: floatingFlickable
            }
            // = Stage.qml's spread state with the fp6 lines
            x: spreadMaths.targetX + fp6CloseAll.offset
            y: spreadMaths.targetY
            z: index
            height: spreadItem.spreadItemHeight
            width: spreadItem.stackHeight * root.width / root.height
            visible: spreadMaths.itemVisible
            opacity: fp6CloseAll.isClosable(card.appId) ? 1 - fp6CloseAll.fade : 1

            Rectangle {
                id: preview
                width: parent.width
                height: spreadItem.stackHeight
                radius: units.gu(1)
                color: model.color
                border.color: "#40000000"
                antialiasing: true
                transform: [
                    Scale { origin.x: 0; origin.y: preview.height / 2; xScale: spreadMaths.targetScale; yScale: spreadMaths.targetScale },
                    Rotation { origin.x: 0; origin.y: preview.height / 2; axis { x: 0; y: 1; z: 0 } angle: spreadMaths.targetAngle }
                ]
                Label {
                    anchors.centerIn: parent
                    width: parent.width - units.gu(2)
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    text: model.name + (fp6CloseAll.isClosable(card.appId) ? "" : "\n(kept)")
                    color: "white"
                    fontSize: "large"
                }
            }
            Label {
                anchors { left: parent.left; top: preview.bottom; topMargin: units.gu(1) }
                text: model.name
                color: "white"
                opacity: spreadMaths.tileInfoOpacity
            }
        }
    }

    TestCase {
        id: tc
        name: "SpreadCloseAll"
        when: windowShown

        property var swipeArea: null
        readonly property real ty: root.height * 0.45

        function findSwipeArea(item) {
            if (item.hasOwnProperty("grabGesture") && item.hasOwnProperty("touchPosition")) return item;
            for (var i = 0; i < item.children.length; i++) {
                var r = findSwipeArea(item.children[i]);
                if (r) return r;
            }
            return null;
        }

        function shot(name) {
            waitForRendering(root);
            var img = grabImage(root);
            img.save(Shot.out + "/" + name + ".png");
            console.log("shot " + name);
        }

        function setApps(list) {
            apps.clear();
            for (var i = 0; i < list.length; i++) {
                apps.append({ appId: list[i][0], name: list[i][1], color: root.palette[i % root.palette.length] });
            }
            floatingFlickable.contentX = 0;
            tryCompare(cards, "count", list.length);
        }

        function fourApps() {
            setApps([["morph-browser", "Browser"], ["lomiri-dialer-app", "Phone"], ["music-app", "Music"], ["lomiri-system-settings", "Settings"]]);
        }

        function tenApps() {
            var l = [];
            var names = ["Browser", "Phone", "Music", "Settings", "Camera", "Gallery", "Messages", "Notes", "Terminal", "Weather"];
            var ids = ["morph-browser", "lomiri-dialer-app", "music-app", "lomiri-system-settings", "lomiri-camera-app", "lomiri-gallery-app", "messaging-app", "notes-app", "lomiri-terminal-app", "weather-app"];
            for (var i = 0; i < 10; i++) l.push([ids[i], names[i]]);
            setApps(l);
        }

        function appIds() {
            var out = [];
            for (var i = 0; i < apps.count; i++) out.push(apps.get(i).appId);
            return out.join(",");
        }

        // A touch drag: press at x0, then move in 1 gu steps through the
        // given x positions (in order). Returns the touch sequence (still pressed).
        function touchDrag(xs) {
            var ev = touchEvent(root);
            var x = xs[0];
            ev.press(0, root, x, ty); ev.commit();
            for (var k = 1; k < xs.length; k++) {
                var target = xs[k];
                var step = target > x ? units.gu(1) : -units.gu(1);
                while (Math.abs(target - x) > Math.abs(step)) {
                    x += step;
                    wait(8);
                    ev = touchEvent(root); ev.move(0, root, x, ty); ev.commit();
                }
                x = target;
                settle(x);
            }
            wait(50);
            return x;
        }
        // The SwipeArea smooths its touchPosition (40 % of the gap per event);
        // a resting finger keeps sending events, so does this.
        function settle(x) {
            for (var i = 0; i < 8; i++) {
                wait(8);
                var ev = touchEvent(root); ev.move(0, root, x, ty); ev.commit();
            }
        }
        function touchUp(x) {
            var ev = touchEvent(root);
            ev.release(0, root, x, ty); ev.commit();
        }

        function initTestCase() {
            swipeArea = findSwipeArea(floatingFlickable);
            verify(swipeArea !== null, "FloatingFlickable SwipeArea");
            LomiriTest.TestExtras.removeTimeConstraintsFromSwipeArea(swipeArea);
            appReady.keepReady = ["music-app"];
            console.log("CHECK threshold=" + fp6CloseAll.threshold + " px (" + (fp6CloseAll.threshold / units.gu(1)).toFixed(1) + " gu), width=" + root.width);
        }

        function init() {
            root.callActive = false;
            root.closeRequests = [];
            root.stopRequests = [];
            root.finishedCount = 0;
            switcherSettings.closeAllGesture = true;
            switcherSettings.closeAllKeepReady = true;
            fp6CloseAll.hapticCount = 0;
            fp6CloseAll.reset();
            fourApps();
            wait(100);
        }

        function test_00_spread_with_clear_all() {
            tryCompare(fp6CloseAll, "closableCount", 3);   // music-app is kept ready
            var b = findChild(fp6CloseAll, "fp6ClearAllButton");
            verify(b.visible && b.enabled);
            shot("00-spread-clear-all");
        }

        function test_01_below_threshold_springs_back() {
            var t = fp6CloseAll.threshold;
            var x = touchDrag([units.gu(8), units.gu(8) + 0.6 * t]);
            verify(fp6CloseAll.dragging, "armed at the start");
            verify(fp6CloseAll.offset > 0.5 * t && fp6CloseAll.offset < t, "offset " + fp6CloseAll.offset);
            compare(fp6CloseAll.beyondThreshold, false);
            compare(findChild(fp6CloseAll, "fp6ReleaseHint").visible, false);
            compare(fp6CloseAll.hapticCount, 0);
            shot("01-below-threshold");
            touchUp(x);
            tryCompare(fp6CloseAll, "offset", 0, 2000);
            wait(300);
            compare(root.closeRequests.length, 0);
            compare(apps.count, 4);
            compare(root.finishedCount, 0);
            console.log("CHECK below threshold: spring back, nothing closed");
        }

        function test_02_at_threshold_then_back_cancels() {
            var t = fp6CloseAll.threshold;
            var x0 = units.gu(8);
            var x = touchDrag([x0, x0 + t + units.gu(1)]);
            verify(fp6CloseAll.beyondThreshold);
            compare(fp6CloseAll.hapticCount, 1);
            tryCompare(findChild(fp6CloseAll, "fp6ReleaseHint"), "opacity", 1, 1000);
            shot("02-at-threshold");
            x = touchDrag2(x, x0 + 0.5 * t);
            compare(fp6CloseAll.beyondThreshold, false);
            touchUp(x);
            tryCompare(fp6CloseAll, "offset", 0, 2000);
            wait(300);
            compare(root.closeRequests.length, 0);
            compare(apps.count, 4);
            console.log("CHECK at threshold: label + 1 haptic tick; back below and release cancels");
        }

        // continue an already pressed drag
        function touchDrag2(x, target) {
            var step = target > x ? units.gu(1) : -units.gu(1);
            while (Math.abs(target - x) > Math.abs(step)) {
                x += step;
                wait(8);
                var ev = touchEvent(root); ev.move(0, root, x, ty); ev.commit();
            }
            x = target;
            settle(x);
            wait(50);
            return x;
        }

        function test_03_after_threshold_closes_all() {
            var t = fp6CloseAll.threshold;
            var x0 = units.gu(8);
            var x = touchDrag([x0, x0 + 1.35 * t]);
            verify(fp6CloseAll.beyondThreshold);
            compare(fp6CloseAll.hapticCount, 1);
            tryCompare(findChild(fp6CloseAll, "fp6ReleaseHint"), "opacity", 1, 1000);
            shot("03-past-threshold");
            touchUp(x);
            wait(120);
            shot("04-closing");
            tryCompare(root, "finishedCount", 1, 4000);
            compare(appIds(), "music-app");
            compare(root.closeRequests.sort().join(","), "lomiri-dialer-app,lomiri-system-settings,morph-browser");
            compare(root.stopRequests.length, 0);
            wait(200);
            compare(fp6CloseAll.offset, 0);
            shot("05-after-close-all");
            console.log("CHECK past threshold: closed " + root.closeRequests.length + ", kept " + appIds() + ", finished -> home");
        }

        function test_04_drag_that_scrolls_to_start_never_closes() {
            tenApps();
            verify(floatingFlickable.contentWidth > root.width, "scrollable: " + floatingFlickable.contentWidth);
            floatingFlickable.contentX = units.gu(25);
            wait(100);
            var x0 = units.gu(6);
            // one continuous drag: scrolls back to the start and keeps going far past it
            var x = touchDrag([x0, x0 + units.gu(25) + 1.6 * fp6CloseAll.threshold]);
            compare(fp6CloseAll.dragging, false, "not armed");
            compare(fp6CloseAll.offset, 0);
            compare(findChild(fp6CloseAll, "fp6ReleaseHint").visible, false);
            shot("06-scrolled-drag-reaches-start");
            touchUp(x);
            wait(800);
            compare(root.closeRequests.length, 0);
            compare(apps.count, 10);
            compare(root.finishedCount, 0);
            compare(fp6CloseAll.hapticCount, 0);
            console.log("CHECK drag started mid-list: scrolled to start (contentX=" + floatingFlickable.contentX.toFixed(1) + "), never closes");
        }

        function test_05_scrollable_list_at_start_closes() {
            tenApps();
            wait(100);
            compare(floatingFlickable.contentX, 0);
            var x0 = units.gu(6);
            var x = touchDrag([x0, x0 + 1.3 * fp6CloseAll.threshold]);
            verify(fp6CloseAll.beyondThreshold);
            compare(floatingFlickable.contentX, 0, "no overshoot while armed");
            shot("07-scrollable-at-start-past-threshold");
            touchUp(x);
            tryCompare(root, "finishedCount", 1, 4000);
            compare(appIds(), "music-app");
            console.log("CHECK long list started at its start: closes all (" + root.closeRequests.length + ")");
        }

        function test_06_left_first_then_right_is_a_scroll() {
            tenApps();
            wait(100);
            var x0 = units.gu(30);
            var x = touchDrag([x0, x0 - units.gu(6), x0 + 1.4 * fp6CloseAll.threshold]);
            compare(fp6CloseAll.dragging, false);
            touchUp(x);
            wait(800);
            compare(root.closeRequests.length, 0);
            compare(apps.count, 10);
            console.log("CHECK right-to-left first: a normal scroll, never closes");
        }

        function test_07_quick_flick_past_threshold() {
            // a fast drag released past the threshold still closes
            var ev = touchEvent(root); var x = units.gu(8);
            ev.press(0, root, x, ty); ev.commit();
            var end = x + 1.2 * fp6CloseAll.threshold;
            while (x < end) { x += units.gu(4); wait(4); ev = touchEvent(root); ev.move(0, root, x, ty); ev.commit(); }
            touchUp(x);
            tryCompare(root, "finishedCount", 1, 4000);
            compare(appIds(), "music-app");
        }

        function test_08_dialer_kept_during_call_and_ready_apps_closed_when_off() {
            root.callActive = true;
            switcherSettings.closeAllKeepReady = false;
            tryCompare(fp6CloseAll, "closableCount", 3);
            var x = touchDrag([units.gu(8), units.gu(8) + 1.3 * fp6CloseAll.threshold]);
            shot("08-call-in-progress");
            touchUp(x);
            tryCompare(root, "finishedCount", 1, 4000);
            compare(appIds(), "lomiri-dialer-app");
            console.log("CHECK call in progress + keep-ready off: kept " + appIds());
        }

        function test_09_clear_all_button() {
            var b = findChild(fp6CloseAll, "fp6ClearAllButton");
            var p = b.mapToItem(root, b.width / 2, b.height / 2);
            var ev = touchEvent(root); ev.press(0, root, p.x, p.y); ev.commit();
            wait(50);
            ev = touchEvent(root); ev.release(0, root, p.x, p.y); ev.commit();
            tryCompare(root, "finishedCount", 1, 4000);
            compare(appIds(), "music-app");
            compare(b.visible, false, "hidden: nothing left to close");
            console.log("CHECK Clear all button: closed " + root.closeRequests.length);
        }

        function test_10_gesture_off() {
            switcherSettings.closeAllGesture = false;
            var x = touchDrag([units.gu(8), units.gu(8) + 1.4 * fp6CloseAll.threshold]);
            compare(fp6CloseAll.dragging, false);
            touchUp(x);
            wait(800);
            compare(root.closeRequests.length, 0);
            verify(findChild(fp6CloseAll, "fp6ClearAllButton").visible, "button stays");
        }

        function test_11_ignored_close_gets_stop_application() {
            // an app that ignores the close request: stopApplication after the wait
            setApps([["stubborn-app", "Stubborn"], ["morph-browser", "Browser"]]);
            fp6CloseAll.closeAll();
            tryCompare(root, "finishedCount", 1, 4000);
            tryCompare(root, "stopRequests", ["stubborn-app"], 5000);
            compare(apps.count, 0);
        }

        function test_12_vertical_card_swipe_untouched() {
            // vertical drags are not the SwipeArea's: nothing arms
            var ev = touchEvent(root); var y = ty;
            ev.press(0, root, units.gu(20), y); ev.commit();
            for (var i = 0; i < 20; i++) { y -= units.gu(1); wait(8); ev = touchEvent(root); ev.move(0, root, units.gu(20), y); ev.commit(); }
            compare(fp6CloseAll.dragging, false);
            ev = touchEvent(root); ev.release(0, root, units.gu(20), y); ev.commit();
        }
    }
}
