/*
 * Do Not Disturb > Schedules: weekly windows that turn DND on.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "dndSchedulesPage"
    property var dm
    title: i18n.tr("Schedules")
    flickable: flick

    function setEnabled(i, on) {
        var l = dm.schedules.slice()
        l[i].enabled = on
        dm.saveSchedules(l)
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        Column {
            id: content
            anchors { left: parent.left; right: parent.right }
            spacing: units.gu(1)
            Item { width: 1; height: units.gu(0.5) }

            Label {
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Do Not Disturb turns on at the start of a schedule and off at its end. A window that ends before it starts runs overnight.")
            }

            Repeater {
                model: page.dm.schedules
                delegate: SettingsListItems.Standard {
                    objectName: "schedule" + index
                    text: modelData.name || i18n.tr("Schedule")
                    layout.subtitle.text: page.dm.windowText(modelData)
                    Switch {
                        objectName: "scheduleSwitch" + index
                        checked: modelData.enabled
                        onTriggered: page.setEnabled(index, checked)
                    }
                    onClicked: pageStack.addPageToNextColumn(page, Qt.resolvedUrl("ScheduleEdit.qml"), { dm: page.dm, index: index })
                }
            }

            SettingsListItems.Standard {
                objectName: "addSchedule"
                text: i18n.tr("Add schedule…")
                onClicked: {
                    var l = page.dm.schedules.slice()
                    l.push({ name: i18n.tr("Schedule %1").arg(l.length + 1), enabled: true, days: [1, 2, 3, 4, 5, 6, 7], start: "22:00", end: "07:00" })
                    page.dm.saveSchedules(l)
                    pageStack.addPageToNextColumn(page, Qt.resolvedUrl("ScheduleEdit.qml"), { dm: page.dm, index: l.length - 1 })
                }
            }
        }
    }
}
