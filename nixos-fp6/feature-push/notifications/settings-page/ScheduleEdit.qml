/*
 * Do Not Disturb > Schedules > one schedule: name, days, start, end.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "dndScheduleEditPage"
    property var dm
    property int index: 0
    readonly property var w: dm.schedules[index] || { name: "", enabled: false, days: [], start: "22:00", end: "07:00" }
    title: w.name || i18n.tr("Schedule")
    flickable: flick

    function set(key, value) {
        var l = dm.schedules.slice()
        if (!l[index]) return
        l[index][key] = value
        dm.saveSchedules(l)
    }

    Component { id: timeDialog; TimeDialog {} }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        Column {
            id: content
            anchors { left: parent.left; right: parent.right }
            spacing: units.gu(1)

            TextField {
                objectName: "scheduleName"
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                text: page.w.name
                placeholderText: i18n.tr("Name")
                onAccepted: page.set("name", text)
                onActiveFocusChanged: if (!activeFocus && text !== page.w.name) page.set("name", text)
            }
            SettingsListItems.Standard {
                text: i18n.tr("On")
                Switch {
                    checked: page.w.enabled
                    onTriggered: page.set("enabled", checked)
                }
            }
            SettingsItemTitle { text: i18n.tr("Days (the day the window starts)") }
            DaysRow {
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                days: page.w.days || []
                names: page.dm.dayNames
                onToggled: page.set("days", days)
            }
            SettingsListItems.SingleValueProgression {
                objectName: "scheduleStart"
                text: i18n.tr("Start")
                value: page.w.start
                onClicked: PopupUtils.open(timeDialog, page, { title: i18n.tr("Start"), time: page.w.start, onAccepted: function(t) { page.set("start", t) } })
            }
            SettingsListItems.SingleValueProgression {
                objectName: "scheduleEnd"
                text: i18n.tr("End")
                value: page.w.end <= page.w.start ? i18n.tr("%1 next day").arg(page.w.end) : page.w.end
                onClicked: PopupUtils.open(timeDialog, page, { title: i18n.tr("End"), time: page.w.end, onAccepted: function(t) { page.set("end", t) } })
            }
            Button {
                objectName: "deleteSchedule"
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                text: i18n.tr("Delete schedule")
                color: theme.palette.normal.negative
                onClicked: {
                    var l = page.dm.schedules.slice()
                    l.splice(page.index, 1)
                    page.dm.saveSchedules(l)
                    pageStack.removePages(page)
                }
            }
        }
    }
}
