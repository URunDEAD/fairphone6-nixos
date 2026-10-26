/*
 * System Settings > App Launch (FP6): keep chosen apps ready (suspended in
 * memory, started hidden at login, files preloaded), compressed memory
 * (zram) and the low-memory policy. Backend: fp6-app-ready (user service),
 * GSettings com.fairphone6.app-ready; Lomiri reads the same keys to decide
 * which background apps it suspends.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems

ItemPage {
    id: root
    objectName: "appLaunchPage"
    title: i18n.tr("App Launch")
    flickable: flick

    AppReadyModel { id: ar }
    readonly property var gs: ar.settings
    readonly property var st: ar.status

    // Memory used by the apps the user keeps ready (RAM + zram).
    readonly property var keptUsage: {
        var ram = 0, swap = 0, n = 0
        if (st && st.apps) {
            for (var i = 0; i < st.apps.length; i++) {
                var a = st.apps[i]
                if (a.running && (a.keepReady || a.prelaunch)) { ram += a.memory; swap += a.swap; n++ }
            }
        }
        return { ram: ram, swap: swap, count: n }
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: content.height + units.gu(4)
        boundsBehavior: (contentHeight > root.height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick

        Column {
            id: content
            anchors.left: parent.left
            anchors.right: parent.right
            spacing: units.gu(1)

            Label {
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                topPadding: units.gu(1)
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Apps you choose here open almost instantly: they stay in memory, suspended (no CPU, no battery) while you use other apps, they can start hidden when you log in, and their files are kept in the cache.")
            }

            // ------------------------------------------------------ memory
            SettingsItemTitle { text: i18n.tr("Memory") }

            Column {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                spacing: units.gu(0.5)
                visible: ar.statusOk

                Label {
                    text: st ? i18n.tr("In use: %1 of %2").arg(ar.mb(st.mem.total - st.mem.available)).arg(ar.mb(st.mem.total)) : ""
                }
                ProgressBar {
                    width: parent.width
                    minimumValue: 0
                    maximumValue: st ? st.mem.total : 1
                    value: st ? st.mem.total - st.mem.available : 0
                    showProgressPercentage: false
                }
                Label {
                    width: parent.width
                    wrapMode: Text.WordWrap
                    fontSize: "small"
                    text: !st ? "" : st.zram.active
                        ? i18n.tr("Compressed memory: %1 of app data stored in %2 (size %3).").arg(ar.mb(st.zram.orig)).arg(ar.mb(st.zram.used)).arg(ar.mb(st.zram.disksize))
                        : i18n.tr("Compressed memory is off.")
                }
            }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !ar.statusOk
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Memory details are not available: the fp6-app-ready service is not running.")
            }

            SettingsListItems.Standard {
                text: i18n.tr("Compressed memory (zram)")
                Switch {
                    objectName: "zramSwitch"
                    property bool serverChecked: gs.zramEnabled
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.zramEnabled = checked
                }
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Rarely used memory of background apps is compressed in RAM (zstd, up to half of the RAM) instead of the app being closed. Costs a little CPU when an app is swapped back in. Recommended: on.")
            }

            SettingsListItems.Standard {
                text: i18n.tr("Close background apps when memory runs low")
                Switch {
                    objectName: "lmkSwitch"
                    property bool serverChecked: gs.lowMemoryKiller
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.lowMemoryKiller = checked
                }
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Other background apps are closed first, apps kept ready last, the least recently used first. The app on screen and the shell are never closed. A closed app stays in the app switcher and restarts when you open it.")
            }

            Repeater {
                model: st && st.lowMemoryKills ? st.lowMemoryKills : []
                delegate: Label {
                    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                    fontSize: "small"
                    color: theme.palette.normal.backgroundTertiaryText
                    text: i18n.tr("Closed %1 (%2) at %3").arg(ar.app(modelData.id).name).arg(ar.mb(modelData.memory))
                        .arg(Qt.formatTime(new Date(modelData.at * 1000), "hh:mm"))
                }
            }

            ListItem.ThinDivider {}

            // -------------------------------------------------- keep ready
            SettingsItemTitle { text: i18n.tr("Keep apps ready") }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: ar.chosen.length === 0
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("No apps chosen. Add the apps you open most often, for example the browser or the camera.")
            }

            Repeater {
                model: ar.chosen
                delegate: ListItem.Subtitled {
                    objectName: "keptApp_" + modelData
                    readonly property var info: ar.app(modelData)
                    readonly property var s: ar.appStatus(modelData)
                    iconSource: ar.iconSource(info.icon)
                    iconFrame: false
                    progression: true
                    text: info.name
                    subText: ar.stateText(s) + (s && s.running ? " · " + ar.mb(s.memory + s.swap) : "")
                    onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("AppOptions.qml"),
                                                             { appId: modelData, model: ar })
                }
            }

            SettingsListItems.StandardProgression {
                objectName: "addAppsItem"
                text: i18n.tr("Add apps…")
                onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("AppPicker.qml"), { model: ar })
            }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !!(st && st.batterySaver)
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Battery saver is on: every app is suspended in the background, starting apps at login and preloading wait until it is off, and background apps are closed earlier when memory gets low.")
            }

            SettingsListItems.Standard {
                text: i18n.tr("Suspend all other apps in the background")
                Switch {
                    objectName: "freezeAllSwitch"
                    property bool serverChecked: gs.freezeBackground
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.freezeBackground = checked
                }
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: gs.freezeBackground
                    ? i18n.tr("Every app you leave is suspended: best for the battery, but music, downloads and terminal jobs pause too while their app is in the background.")
                    : i18n.tr("Off: only the apps above are suspended; other apps keep running in the background (music, downloads, terminal jobs).")
            }

            ListItem.ThinDivider {}

            // -------------------------------------------------------- cost
            SettingsItemTitle { text: i18n.tr("Cost") }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: (keptUsage.count > 0
                        ? i18n.tr("Apps kept ready now use %1 of RAM and %2 of compressed memory.").arg(ar.mb(keptUsage.ram)).arg(ar.mb(keptUsage.swap)) + " "
                        : "")
                    + i18n.tr("Memory: a kept app holds its memory (typically 100–400 MB; a browser with tabs more). Battery: suspended apps use no CPU; on this phone 3 suspended apps added no measurable idle drain (see the notes). Starting apps at login costs a few seconds of CPU once; preloading reads up to the preload budget from storage at login and every two hours (a fraction of a mAh). Cached files are given back as soon as other apps need the memory.")
            }

            SettingsListItems.Standard {
                text: i18n.tr("Preload budget per app")
                showDivider: false
            }
            OptionSelector {
                objectName: "budgetSelector"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                readonly property var values: [150, 300, 600]
                model: [i18n.tr("150 MB"), i18n.tr("300 MB"), i18n.tr("600 MB")]
                selectedIndex: Math.max(0, values.indexOf(gs.preloadBudgetMb))
                onDelegateClicked: gs.preloadBudgetMb = values[index]
            }
        }
    }
}
