// Operations the daemon asks the app to do (prompt kind "ui"):
//  syscheck         the app-permissions broker (this process = the Assistant app)
//  camera           a photo from the camera app over Content Hub
//  screenshot_hint  banner while the user takes a screenshot
//  calendar_list / calendar_add   QtOrganizer (mkcal, like the calendar app)
//  alarm_set / timer_set / alarms_list   UITK alarms (like the clock app)
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    id: ops
    property var screenshotPrompt: null

    function reply(p, v) { root.respond(p.id, v) }
    function fail(p, e) { reply(p, {ok: false, error: e}) }

    function handle(p) {
        switch (p.op) {
        case "syscheck":
            root.call("syscheck", [p.args.permission], function(r, err) {
                if (err) { ops.reply(p, {ok: true, result: "denied"}); return }
                ops.reply(p, JSON.parse(r))
            })
            break
        case "camera":
            var page = stack.push(Qt.resolvedUrl("CameraImport.qml"), {reason: p.args.reason || ""})
            page.done.connect(function(path) {
                if (path) ops.reply(p, {ok: true, path: path})
                else ops.fail(p, "The user closed the camera without a photo.")
            })
            break
        case "screenshot_hint":
            screenshotPrompt = p
            break
        case "calendar_list":
        case "calendar_add":
            if (calendar.status !== Loader.Ready) { fail(p, "The calendar is not available on this phone."); break }
            calendar.item.handle(p)
            break
        case "alarm_set":
        case "timer_set":
        case "alarms_list":
            alarms.handle(p)
            break
        default:
            fail(p, "unknown operation " + p.op)
        }
    }
    function closed(id) {
        if (screenshotPrompt && screenshotPrompt.id === id) screenshotPrompt = null
    }
    function cancelScreenshot() {
        if (screenshotPrompt) fail(screenshotPrompt, "cancelled")
        screenshotPrompt = null
    }

    // QtOrganizer is optional (missing in some previews): load lazily.
    Loader {
        id: calendar
        source: Qt.resolvedUrl("CalendarOps.qml")
        asynchronous: false
    }

    AlarmOps { id: alarms }
}
