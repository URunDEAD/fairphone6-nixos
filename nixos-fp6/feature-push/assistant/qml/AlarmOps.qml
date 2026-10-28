// Alarms and timers with Lomiri UITK's Alarm API (the clock app's store).
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    id: al
    function pad(n) { return (n < 10 ? "0" : "") + n }
    function show(d) { return d.getFullYear() + "-" + pad(d.getMonth() + 1) + "-" + pad(d.getDate()) + " " + pad(d.getHours()) + ":" + pad(d.getMinutes()) }

    function handle(p) {
        if (p.op === "alarms_list") {
            var out = []
            for (var i = 0; i < alarmModel.count; i++) {
                var a = alarmModel.get(i)
                out.push({when: show(a.date), label: a.message, enabled: a.enabled})
            }
            root.respond(p.id, {ok: true, alarms: out})
            return
        }
        var when
        if (p.op === "timer_set") {
            when = new Date(Date.now() + p.args.seconds * 1000)
        } else {
            var hm = p.args.time.split(":")
            when = new Date()
            if (p.args.date) {
                var d = p.args.date.split("-")
                when = new Date(+d[0], +d[1] - 1, +d[2])
            }
            when.setHours(+hm[0], +hm[1], 0, 0)
            if (!p.args.date && when.getTime() <= Date.now()) when = new Date(when.getTime() + 86400000)
        }
        var alarm = Qt.createQmlObject('import Lomiri.Components 1.3; Alarm {}', al)
        alarm.message = p.args.label || (p.op === "timer_set" ? i18n.tr("Timer") : i18n.tr("Alarm"))
        alarm.date = when
        alarm.type = Alarm.OneTime
        alarm.enabled = true
        alarm.save()
        if (alarm.error !== Alarm.NoError) root.respond(p.id, {ok: false, error: "the alarm could not be saved (" + alarm.error + ")"})
        else root.respond(p.id, {ok: true, when: show(when)})
    }

    AlarmModel { id: alarmModel }
}
