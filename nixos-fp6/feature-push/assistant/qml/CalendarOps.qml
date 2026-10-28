// Calendar through QtOrganizer with the "mkcal" manager (the store of the
// calendar app on this phone).
import QtQuick 2.12
import QtOrganizer 5.0

Item {
    id: cal
    property var pendingAdd: null

    function pad(n) { return (n < 10 ? "0" : "") + n }
    function local(d) {
        return d.getFullYear() + "-" + pad(d.getMonth() + 1) + "-" + pad(d.getDate()) + "T" + pad(d.getHours()) + ":" + pad(d.getMinutes())
    }
    function parseLocal(s) {
        var m = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(s)
        return m ? new Date(+m[1], +m[2] - 1, +m[3], +m[4], +m[5]) : null
    }

    function handle(p) {
        if (p.op === "calendar_list") {
            var start = new Date(); start.setHours(0, 0, 0, 0)
            var end = new Date(start.getTime() + (p.args.days || 7) * 86400000)
            model.startPeriod = start
            model.endPeriod = end
            model.update()
            listTimer.prompt = p
            listTimer.restart()
        } else {
            var s = parseLocal(p.args.start)
            if (!s) { root.respond(p.id, {ok: false, error: "bad start time"}); return }
            var e = p.args.end ? parseLocal(p.args.end) : new Date(s.getTime() + 3600000)
            var ev = Qt.createQmlObject('import QtOrganizer 5.0; Event {}', cal)
            ev.displayLabel = p.args.title
            ev.startDateTime = s
            ev.endDateTime = e
            if (p.args.location) ev.location = p.args.location
            if (p.args.notes) ev.description = p.args.notes
            cal.pendingAdd = p
            model.saveItem(ev)
            addTimer.restart()
        }
    }

    OrganizerModel {
        id: model
        manager: "mkcal"
        autoUpdate: true
        onItemCountChanged: {}
        onErrorChanged: if (cal.pendingAdd && error !== OrganizerModel.NoError) {
            root.respond(cal.pendingAdd.id, {ok: false, error: "the calendar refused the event (" + error + ")"})
            cal.pendingAdd = null
        }
    }
    // saveItem is asynchronous; mkcal reports errors through `error`.
    Timer {
        id: addTimer
        interval: 1500
        onTriggered: if (cal.pendingAdd) { root.respond(cal.pendingAdd.id, {ok: true}); cal.pendingAdd = null }
    }
    Timer {
        id: listTimer
        property var prompt: null
        interval: 800
        onTriggered: {
            var out = []
            var items = model.items
            for (var i = 0; i < items.length && out.length < 100; i++) {
                var it = items[i]
                if (!it.startDateTime) continue
                out.push({title: it.displayLabel, start: cal.local(it.startDateTime), end: it.endDateTime ? cal.local(it.endDateTime) : "",
                          location: it.location || "", all_day: it.allDay === true})
            }
            out.sort(function(a, b) { return a.start < b.start ? -1 : 1 })
            root.respond(prompt.id, {ok: true, events: out})
        }
    }
}
