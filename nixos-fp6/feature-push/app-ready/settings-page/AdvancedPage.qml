/*
 * Memory & performance > Advanced (FP6): prediction, how many predicted
 * apps are kept ready, warming at login, zram size and switch, low-memory
 * closing, suspend-all, never-keep apps, preload budget, clear the learned
 * history. GSettings com.fairphone6.app-ready; "-1" / "0" values follow the
 * profile.
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
    objectName: "memoryAdvancedPage"
    property var model
    readonly property var gs: model.settings
    readonly property var st: model.status
    title: i18n.tr("Advanced")
    flickable: flick

    // the profile's own numbers (from the daemon's status)
    readonly property int profileReady: st && st.tuning ? st.tuning.readyApps : 6
    readonly property int profileZram: ({ conservative: 25, balanced: 50, aggressive: 75 })[gs.profile] || 75

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

            // ----------------------------------------------- prediction
            SettingsItemTitle { text: i18n.tr("Prediction") }

            SettingsListItems.Standard {
                text: i18n.tr("Predict the apps you open next")
                Switch {
                    objectName: "predictionSwitch"
                    property bool serverChecked: gs.prediction
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.prediction = checked
                }
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Learned on this phone only and never sent anywhere: which app you open at what hour, on weekdays or weekends, whether the phone was charging, on Wi-Fi or had headphones connected (yes or no only: no network names, no places), and which app follows which. Old habits fade after a few weeks.")
            }

            SettingsListItems.Standard {
                text: i18n.tr("Apps kept ready by prediction")
                enabled: gs.prediction
                showDivider: false
            }
            OptionSelector {
                objectName: "readyAppsSelector"
                enabled: gs.prediction
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                readonly property var values: [-1, 0, 1, 2, 3, 4, 6, 8, 12]
                model: values.map(function(v) {
                    return v < 0 ? i18n.tr("As the profile (%1)").arg(page.profileReady) : String(v)
                })
                selectedIndex: Math.max(0, values.indexOf(gs.readyApps))
                onDelegateClicked: gs.readyApps = values[index]
            }

            SettingsListItems.Standard {
                text: i18n.tr("Prepare apps after start-up")
                Switch {
                    objectName: "bootWarmSwitch"
                    property bool serverChecked: gs.preloadAtBoot
                    onServerCheckedChanged: checked = serverChecked
                    Component.onCompleted: checked = serverChecked
                    onTriggered: gs.preloadAtBoot = checked
                }
                showDivider: false
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Once the phone is idle after you log in, the files of the shell (fonts, icons, libraries) and of the likely apps are read into the cache.")
            }

            Button {
                objectName: "clearHistoryButton"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                text: i18n.tr("Clear learned history")
                onClicked: PopupUtils.open(clearDialog)
            }
            Label {
                objectName: "clearedLabel"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                visible: page.cleared
                fontSize: "small"
                text: i18n.tr("Learned history cleared.")
            }

            ListItem.ThinDivider {}

            // ------------------------------------------------ never keep
            SettingsItemTitle { text: i18n.tr("Never keep ready") }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("These apps are never started in advance or kept ready, and they are the first to be closed when memory runs low.")
            }
            Repeater {
                model: page.model.neverList
                delegate: ListItem.Standard {
                    objectName: "never_" + modelData
                    readonly property var info: page.model.app(modelData)
                    iconSource: page.model.iconSource(info.icon)
                    iconFrame: false
                    text: info.name
                    control: Button {
                        text: i18n.tr("Remove")
                        onClicked: page.model.setNever(modelData, false)
                    }
                }
            }
            SettingsListItems.StandardProgression {
                objectName: "addNeverItem"
                text: i18n.tr("Add apps…")
                onClicked: pageStack.addPageToNextColumn(page, Qt.resolvedUrl("AppPicker.qml"), { model: page.model, target: "never" })
            }

            // ------------------------------------------- compressed memory
            SettingsItemTitle { text: i18n.tr("Compressed memory") }

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
            OptionSelector {
                objectName: "zramSizeSelector"
                enabled: gs.zramEnabled
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                readonly property var values: [0, 25, 50, 75, 100]
                model: values.map(function(v) {
                    return v === 0 ? i18n.tr("As the profile (%1 % of RAM)").arg(page.profileZram) : i18n.tr("%1 % of RAM").arg(v)
                })
                selectedIndex: Math.max(0, values.indexOf(gs.zramPercent))
                onDelegateClicked: gs.zramPercent = values[index]
            }
            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                text: i18n.tr("Memory of apps in the background is compressed in RAM (zstd, about 3:1) instead of the app being closed. A larger size keeps more apps; it costs a little CPU when an app comes back. A new size applies when little is compressed, at the latest at the next login.")
            }

            SettingsListItems.Standard {
                text: i18n.tr("Close apps when memory is low")
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
                text: i18n.tr("When memory runs out, the app least likely to be used next is closed first: never-keep apps, then other background apps, then predicted ones, apps you always keep last. The app on screen and the shell are never closed. A closed app stays in the app switcher and restarts when you open it.")
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
                    : i18n.tr("Off: only kept-ready apps are suspended; other apps keep running in the background (music, downloads, terminal jobs).")
            }

            SettingsListItems.Standard {
                text: i18n.tr("Files cached per app")
                showDivider: false
            }
            OptionSelector {
                objectName: "budgetSelector"
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                readonly property var values: [200, 400, 800]
                model: [i18n.tr("200 MB"), i18n.tr("400 MB"), i18n.tr("800 MB")]
                selectedIndex: Math.max(0, values.indexOf(gs.preloadBudgetMb))
                onDelegateClicked: gs.preloadBudgetMb = values[index]
            }
        }
    }

    property bool cleared: false

    Component {
        id: clearDialog
        Dialog {
            id: dlg
            objectName: "clearHistoryDialog"
            title: i18n.tr("Clear learned history?")
            text: i18n.tr("The phone forgets which apps you use when and which files they need. Predictions start again from nothing.")
            Button {
                objectName: "clearConfirm"
                text: i18n.tr("Clear")
                color: theme.palette.normal.negative
                onClicked: {
                    page.model.clearHistory()
                    page.cleared = true
                    PopupUtils.close(dlg)
                }
            }
            Button {
                text: i18n.tr("Cancel")
                onClicked: PopupUtils.close(dlg)
            }
        }
    }
}
