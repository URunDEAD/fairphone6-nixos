// Preview of System Settings > Gesture Shortcuts, scrolled to Back tap (the
// real page, offscreen). The keys fp6-gestures writes are set by hand.
import QtQuick 2.12
import GSettings 1.0
import Lomiri.Components 1.3
import "Shot.js" as Shot

MainView {
    id: mv
    width: units.gu(46.5)
    height: units.gu(103.5)
    applicationName: "lomiri-system-settings"
    GSettings { id: gs; schema.id: "com.fairphone6.gestures" }
    Rectangle { anchors.fill: parent; color: theme.palette.normal.background; z: -1 }
    PageStack { id: pageStack; Component.onCompleted: push(Qt.resolvedUrl("PageComponent.qml")) }

    function find(item, name) {
        if (!item) return null
        if (item.objectName === name) return item
        var kids = item.children || []
        for (var i = 0; i < kids.length; i++) {
            var r = find(kids[i], name)
            if (r) return r
        }
        return null
    }
    function check(name, ok) { console.log((ok ? "PASS " : "FAIL ") + name) }

    readonly property string profile: "v1 peak=0.912 min=0.640 ratio=4.10 gapmin=0.210 gapmax=0.300 noise=0.0050 sign=-1 n=8"

    Component.onCompleted: {
        gs.backDoubleTapAction = "flashlight"
        gs.backTripleTapAction = "none"
    }
    Timer {
        interval: 1200; running: true
        onTriggered: {
            var s = Shot.shot
            if (s === "calibrating") {
                gs.backTapMode = "calibrate"
                gs.backTapCalProgress = 2
                gs.backTapLog = ["1\ttap\t0.62\t0.08", "2\ttap\t0.71\t0.08", "3\ttap\t0.55\t0.08", "4\ttap\t0.66\t0.08", "5\ttap\t0.58\t0.08"]
            } else if (s === "calibrated") {
                gs.backTapProfile = mv.profile
                gs.backTapCalProgress = 4
                gs.backTapLog = ["8\ttap\t0.66\t0.08", "9\tcalibrated\t0.91\t0.30"]
            } else if (s === "failed") {
                gs.backTapLog = ["7\ttap\t0.31\t0.08", "8\tfailed\tuneven"]
            } else if (s === "test") {
                gs.backTapProfile = mv.profile
                gs.backTapLevel = 60
                gs.backTapMode = "test"
                gs.backTapLog = ["1\tmoving", "2\ttap\t0.74\t0.40", "3\ttap\t0.52\t0.40", "4\tgesture\t2",
                                 "5\tignored\tsideways\t0.48", "6\ttap\t0.66\t0.40", "7\ttap\t0.61\t0.40",
                                 "8\trejected\ttouch\t2", "9\ttap\t0.83\t0.40", "10\ttap\t0.58\t0.40", "11\ttap\t0.70\t0.40",
                                 "12\tgesture\t3"]
            }
            later.start()
        }
    }
    Timer {
        id: later; interval: 600
        onTriggered: {
            var page = pageStack.currentPage
            var section = find(page, "backTapSettings")
            if (Shot.shot === "checks") return checks(page, section)
            if (section && page.flickable)
                page.flickable.contentY = Math.max(0, section.mapToItem(page.flickable.contentItem, 0, 0).y - units.gu(34))
            grab.start()
        }
    }
    Timer { id: grab; interval: 500; onTriggered: mv.grabToImage(function(r) { r.saveToFile(Shot.out); Qt.quit() }) }

    // Steps 250 ms apart: gsettings-qt writes arrive asynchronously.
    property var steps: []
    Timer {
        id: stepper; interval: 250; repeat: true
        onTriggered: {
            if (mv.steps.length === 0) { stop(); Qt.quit(); return }
            mv.steps.shift()()
        }
    }

    function checks(page, section) {
        var slider = find(page, "backTapLevelSlider")
        var cal = find(page, "backTapCalibrateButton")
        var sw = find(page, "backTapTestSwitch")
        steps = [
            function() {
                check("back tap section present", section !== null)
                check("slider starts at the stored level", slider && Math.round(slider.value) === 50)
                slider.value = 80
            },
            function() { check("slider writes back-tap-level", gs.backTapLevel === 80); gs.backTapLevel = 20 },
            function() { check("slider follows the key", Math.round(slider.value) === 20); cal.clicked() },
            function() {
                check("Start sets calibrate mode", gs.backTapMode === "calibrate")
                check("calibration panel shown", find(page, "backTapCalibrationPanel").visible)
                check("test panel hidden while calibrating", !find(page, "backTapTestPanel").visible)
                gs.backTapCalProgress = 3
            },
            function() { check("progress shown", find(page, "backTapCalibrationPanel").children[2].text.indexOf("3 of 4") === 0); cal.clicked() },
            function() { check("Cancel sets normal mode", gs.backTapMode === "normal"); sw.trigger() },
            function() {
                check("test switch sets test mode", gs.backTapMode === "test")
                check("test panel shown", find(page, "backTapTestPanel").visible)
                gs.backTapLog = ["1\ttap\t0.70\t0.40", "2\ttap\t0.60\t0.40", "3\tgesture\t2"]
            },
            function() {
                var panel = find(page, "backTapTestPanel")
                var rows = []
                for (var i = 0; i < panel.children.length; i++)
                    if (panel.children[i].children.length === 2) rows.push(panel.children[i].children[1].text)
                check("live entries listed newest first", rows.length === 3 && rows[0] === "Double tap recognised" && /^Tap, 0[.,]70 g/.test(rows[2]))
                pageStack.pop()
            },
            function() { check("leaving the page ends test mode", gs.backTapMode === "normal") }
        ]
        stepper.start()
    }
}
