/*
 * System Settings > Memory & performance (FP6; was "App Launch"): the RAM
 * picture, the memory profile, the apps the phone predicts and keeps ready,
 * the apps the user always keeps ready (suspended in memory, started hidden
 * at login, files preloaded); compressed memory, the low-memory policy and
 * the rest on the Advanced page. Backend: fp6-app-ready (user service),
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
    title: i18n.tr("Memory & performance")
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

    // RAM picture: apps (their cgroups), cache (droppable), free, the rest = system.
    readonly property var ram: {
        if (!st || !st.ram || !st.mem) return null
        var total = st.mem.total
        var apps = Math.min(st.ram.apps, total)
        var cache = Math.min(st.ram.cache, total - apps)
        var free = Math.min(st.ram.free, total - apps - cache)
        return { total: total, apps: apps, cache: cache, free: free, system: Math.max(0, total - apps - cache - free) }
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

            Item { width: 1; height: units.gu(1) }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Free memory makes nothing faster. The phone learns which apps you use when (on the phone only), keeps the likely next ones ready in memory, suspended (no CPU, no battery), and keeps their files in the cache, so they open fast. Apps you choose below are always kept ready.")
            }

            // ------------------------------------------------------ memory
            SettingsItemTitle { text: i18n.tr("Memory") }

            Column {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                spacing: units.gu(0.5)
                visible: ar.statusOk

                Label {
                    text: st ? i18n.tr("Available: %1 of %2").arg(ar.mb(st.mem.available)).arg(ar.mb(st.mem.total)) : ""
                }
                // apps | system | cache | free
                Row {
                    id: ramBar
                    objectName: "ramBar"
                    width: parent.width
                    height: units.gu(1.5)
                    visible: !!root.ram
                    readonly property var parts: root.ram ? [
                        { v: root.ram.apps, c: theme.palette.normal.focus },
                        { v: root.ram.system, c: LomiriColors.orange },
                        { v: root.ram.cache, c: theme.palette.normal.positive },
                        { v: root.ram.free, c: theme.palette.normal.base }
                    ] : []
                    Repeater {
                        model: ramBar.parts
                        delegate: Rectangle {
                            width: root.ram ? Math.round(ramBar.width * modelData.v / root.ram.total) : 0
                            height: ramBar.height
                            color: modelData.c
                        }
                    }
                }
                Label {
                    objectName: "ramLegend"
                    width: parent.width
                    wrapMode: Text.WordWrap
                    fontSize: "small"
                    visible: !!root.ram
                    text: !root.ram ? "" : i18n.tr("Apps %1 · system %2 · cache %3 · free %4")
                        .arg(ar.mb(root.ram.apps)).arg(ar.mb(root.ram.system)).arg(ar.mb(root.ram.cache)).arg(ar.mb(root.ram.free))
                }
                Label {
                    width: parent.width
                    wrapMode: Text.WordWrap
                    fontSize: "small"
                    color: theme.palette.normal.backgroundTertiaryText
                    text: i18n.tr("Cache is memory used to keep files ready; it is given back at once when apps need it.")
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

            // --------------------------------------------------- profile
            SettingsItemTitle { text: i18n.tr("Profile") }

            OptionSelector {
                objectName: "profileSelector"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                model: [ar.profileName("conservative"), ar.profileName("balanced"), ar.profileName("aggressive")]
                selectedIndex: Math.max(0, ar.profiles.indexOf(gs.profile))
                onDelegateClicked: gs.profile = ar.profiles[index]
            }
            Label {
                objectName: "profileText"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: ar.profileText(gs.profile)
            }

            // ----------------------------------------------- predicted apps
            SettingsItemTitle { text: i18n.tr("Ready for you") }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: !gs.prediction || ar.predicted.length === 0
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: !gs.prediction
                    ? i18n.tr("Prediction is off (Advanced).")
                    : i18n.tr("Nothing learned yet: the phone learns from the apps you open.")
            }
            Repeater {
                model: gs.prediction ? ar.predicted.slice(0, Math.max(3, st && st.readyApps ? st.readyApps : 0)) : []
                delegate: ListItem.Subtitled {
                    objectName: "predicted_" + modelData.id
                    readonly property var info: ar.app(modelData.id)
                    iconSource: ar.iconSource(info.icon)
                    iconFrame: false
                    text: info.name
                    subText: i18n.tr("%1 % likely next · %2").arg(Math.round(modelData.p * 100)).arg(ar.predictedState(modelData))
                        + (modelData.ready ? " · " + i18n.tr("kept ready") : "")
                    showDivider: false
                }
            }
            Label {
                objectName: "hitRate"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: gs.prediction && !!(st && st.hits)
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: st && st.hits ? i18n.tr("Since the phone started: %1").arg(ar.hitText(st.hits.boot)) : ""
            }

            // closed by the low-memory killer
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
            SettingsItemTitle { text: i18n.tr("Always keep ready") }

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

            SettingsListItems.StandardProgression {
                objectName: "advancedItem"
                text: i18n.tr("Advanced")
                onClicked: pageStack.addPageToNextColumn(root, Qt.resolvedUrl("AdvancedPage.qml"), { model: ar })
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
                    + i18n.tr("Memory: a kept app holds its memory (typically 100–400 MB; a browser with tabs more). Battery: suspended apps use no CPU; on this phone, 3 apps kept ready used 83 % less CPU in the background than the same apps running (4 s instead of 25 s in 5 minutes with the screen off). Starting apps at login costs a few seconds of CPU once; preloading reads up to the preload budget from storage at login and every two hours (a fraction of a mAh). Cached files are given back as soon as other apps need the memory.")
            }

        }
    }
}
