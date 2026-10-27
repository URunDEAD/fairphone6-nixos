/*
 * Storage > System: old system versions (generations), unused store
 * paths (nix-collect-garbage) and the system journal. Versions that are
 * current, running, booted or pinned by a boot partition are always kept;
 * if a boot partition cannot be read, nothing is removed.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import Lomiri.Components.Popups 1.3
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: page
    objectName: "fp6StorageSystemPage"
    title: i18n.tr("System")
    flickable: flick

    property var sm
    readonly property var sys: sm && sm.storage ? sm.storage.system : null
    readonly property var plan: sys ? sys.generations : null
    readonly property var st: sys && sys.store && !sys.store.error ? sys.store : null
    readonly property var last: sm && sm.storage ? sm.storage.lastAction : null
    readonly property bool busy: !!(sm && sm.busy)
    readonly property var journalSizes: [50, 100, 200, 500]
    property int journalIndex: 1

    function reasonText(r) {
        if (r === "current") return i18n.tr("current")
        if (r === "running") return i18n.tr("running now")
        if (r === "booted") return i18n.tr("started at boot")
        if (r === "unresolved") return i18n.tr("unreadable, kept")
        return i18n.tr("starts from %1").arg(r)
    }

    function entrySub(e) {
        var s = e.date ? Qt.formatDate(new Date(e.date * 1000), "dd.MM.yyyy") : ""
        if (e.reasons.length > 0) {
            var r = []
            for (var i = 0; i < e.reasons.length; i++) r.push(reasonText(e.reasons[i]))
            s += (s ? " · " : "") + i18n.tr("Kept: %1").arg(r.join(", "))
        } else if (!plan.certain) {
            s += (s ? " · " : "") + i18n.tr("Kept")
        } else {
            s += (s ? " · " : "") + i18n.tr("Can be removed")
        }
        return s
    }

    function lastText() {
        if (!last) return ""
        if (last.error) return i18n.tr("Failed: %1").arg(last.error)
        var r = last.result || {}
        if (last.kind === "delete-generations") return i18n.tr("Removed versions %1.").arg((r.deleted || []).join(", "))
        if (last.kind === "gc") return r.summary ? r.summary : i18n.tr("Unused files removed.")
        if (last.kind === "vacuum-journal") return i18n.tr("Logs: %1 → %2.").arg(sm.size(r.before)).arg(sm.size(r.after))
        return ""
    }

    Component {
        id: confirmGens
        Dialog {
            id: d
            title: i18n.tr("Remove old system versions?")
            text: i18n.tr("Versions %1 will be removed. Kept: %2. The space is freed when unused system files are removed.")
                .arg(plan.deletable.join(", "))
                .arg(page.keptList())
            Button {
                objectName: "fp6ConfirmDeleteGens"
                text: i18n.tr("Remove")
                color: theme.palette.normal.negative
                onClicked: { sm.call("storage.deleteGenerations", { nums: plan.deletable }); PopupUtils.close(d) }
            }
            Button { text: i18n.tr("Cancel"); onClicked: PopupUtils.close(d) }
        }
    }
    Component {
        id: confirmGc
        Dialog {
            id: d
            title: i18n.tr("Remove unused system files?")
            text: i18n.tr("Runs nix-collect-garbage. Store paths that no system version, profile or boot partition uses are deleted; this can take a few minutes.")
            Button {
                objectName: "fp6ConfirmGc"
                text: i18n.tr("Remove")
                color: theme.palette.normal.negative
                onClicked: { sm.call("storage.gc", {}); PopupUtils.close(d) }
            }
            Button { text: i18n.tr("Cancel"); onClicked: PopupUtils.close(d) }
        }
    }
    Component {
        id: confirmJournal
        Dialog {
            id: d
            title: i18n.tr("Shrink system logs?")
            text: i18n.tr("The oldest log files are deleted until the logs use at most %1 MB.").arg(page.journalSizes[page.journalIndex])
            Button {
                objectName: "fp6ConfirmJournal"
                text: i18n.tr("Shrink")
                color: theme.palette.normal.negative
                onClicked: { sm.call("storage.vacuumJournal", { mb: page.journalSizes[page.journalIndex] }); PopupUtils.close(d) }
            }
            Button { text: i18n.tr("Cancel"); onClicked: PopupUtils.close(d) }
        }
    }

    function keptList() {
        if (!plan) return ""
        var k = []
        for (var i = 0; i < plan.entries.length; i++)
            if (plan.entries[i].keep) k.push(plan.entries[i].num)
        return k.length ? k.join(", ") : i18n.tr("none")
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        boundsBehavior: (contentHeight > page.height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick

        Column {
            id: content
            anchors.left: parent.left
            anchors.right: parent.right
            spacing: units.gu(1)

            Item { width: 1; height: units.gu(1) }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !sys
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: sm && sm.storage && sm.storage.systemError ? i18n.tr("Not available: %1").arg(sm.storage.systemError) : i18n.tr("Scanning…")
            }

            // ------------------------------------------------ versions
            SettingsItemTitle { text: i18n.tr("System versions"); visible: !!plan }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!plan
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Each system update leaves the previous version on the phone so it can be started again. The current version, the one running now and every version a boot partition starts are always kept.")
            }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!plan && !plan.certain
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: theme.palette.normal.negative
                text: plan ? i18n.tr("Nothing can be removed now: %1.").arg(plan.uncertainReason) : ""
            }

            Repeater {
                model: plan ? plan.entries.slice().reverse() : []
                delegate: ListItem.Subtitled {
                    objectName: "fp6Gen" + modelData.num
                    text: i18n.tr("Version %1").arg(modelData.num)
                    subText: page.entrySub(modelData)
                    showDivider: false
                }
            }

            Button {
                objectName: "fp6DeleteGens"
                anchors { left: parent.left; leftMargin: units.gu(2) }
                visible: !!plan
                enabled: !!plan && plan.deletable.length > 0 && !page.busy
                text: plan && plan.deletable.length > 0
                    ? (st && st.oldGenerations > 0
                        ? i18n.tr("Remove %1 old versions (up to %2)").arg(plan.deletable.length).arg(sm.size(st.oldGenerations))
                        : i18n.tr("Remove %1 old versions").arg(plan.deletable.length))
                    : i18n.tr("No old versions to remove")
                onClicked: PopupUtils.open(confirmGens, page)
            }

            ListItem.ThinDivider {}

            // ------------------------------------------------ garbage
            SettingsItemTitle { text: i18n.tr("Unused system files"); visible: !!st }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!st
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: !st ? "" : st.garbageKnown
                    ? i18n.tr("%1 in %2 store paths are not used by any system version (system and apps: %3, store total %4).")
                        .arg(sm.size(st.garbage)).arg(st.garbagePaths).arg(sm.size(st.currentSystem)).arg(sm.size(st.total))
                    : i18n.tr("Store total %1; unused files were not counted.").arg(sm.size(st.total))
            }
            Button {
                objectName: "fp6Gc"
                anchors { left: parent.left; leftMargin: units.gu(2) }
                visible: !!st
                enabled: !!plan && plan.certain && !page.busy
                text: i18n.tr("Remove unused files")
                onClicked: PopupUtils.open(confirmGc, page)
            }

            ListItem.ThinDivider {}

            // ------------------------------------------------ journal
            SettingsItemTitle { text: i18n.tr("System logs"); visible: !!sys }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!sys
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: sys ? i18n.tr("The system journal uses %1. Shrinking deletes the oldest entries.").arg(sm.size(sys.journal)) : ""
            }
            OptionSelector {
                objectName: "fp6JournalSize"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!sys
                model: [i18n.tr("Keep 50 MB"), i18n.tr("Keep 100 MB"), i18n.tr("Keep 200 MB"), i18n.tr("Keep 500 MB")]
                Component.onCompleted: selectedIndex = page.journalIndex
                onDelegateClicked: page.journalIndex = index
            }
            Button {
                objectName: "fp6Vacuum"
                anchors { left: parent.left; leftMargin: units.gu(2) }
                visible: !!sys
                enabled: !page.busy
                text: i18n.tr("Shrink logs")
                onClicked: PopupUtils.open(confirmJournal, page)
            }

            Row {
                anchors { left: parent.left; leftMargin: units.gu(2) }
                spacing: units.gu(1)
                visible: page.busy
                ActivityIndicator { running: page.busy }
                Label { anchors.verticalCenter: parent.verticalCenter; text: i18n.tr("Working…") }
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !page.busy && text !== ""
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: last && last.error ? theme.palette.normal.negative : theme.palette.normal.backgroundSecondaryText
                text: page.lastText()
            }
        }
    }
}
