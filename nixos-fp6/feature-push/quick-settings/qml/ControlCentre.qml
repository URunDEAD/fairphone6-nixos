/*
 * FP6 quick settings: the control centre shown in the pulled-down panel
 * (Android quick settings / iOS Control Centre): date and time, the tile grid
 * (expand with the chevron or by pulling down again), sliders, media and the
 * notifications. The indicator pages stay one tap away in the panel row.
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15
import Lomiri.Components 1.3
import GSettings 1.0

Item {
    id: cc
    objectName: "fp6ControlCentre"
    property QtObject backend
    property bool lightMode: false
    property bool locked: false
    property var notificationsModel: null
    property bool hideNotificationContent: false
    property var lockedRestricted: []
    property real sideInset: 0      // rounded corners: extra side padding (bottom rows)
    property real bottomInset: 0
    property var indicatorExists: function(identifier) { return false; }

    signal openIndicator(string identifier)
    signal openUrl(string url)
    signal unlockRequested()
    signal openApp(string appId)

    readonly property color fgColor: lightMode ? "#111111" : "#F7F7F7"
    readonly property color fg2Color: lightMode ? "#5D5D5D" : "#AEA79F"
    readonly property color cardColor: lightMode ? Qt.rgba(0, 0, 0, 0.06) : Qt.rgba(1, 1, 1, 0.09)
    property bool editing: false
    property alias tilesExpanded: grid.expanded
    readonly property alias editorItem: editorLoader.item
    onLockedChanged: if (locked) editing = false
    onVisibleChanged: if (!visible) { grid.expanded = false; editing = false; flick.contentY = 0; }

    function restricted(identifier) { return lockedRestricted.indexOf(identifier) >= 0; }
    function tap(def) {
        if (!def || !def.available) return;
        if (locked && def.lockPolicy === "unlock") { unlockRequested(); return; }
        def.toggle();
    }
    function detail(def) {
        if (!def) return;
        var page = def.indicator && indicatorExists(def.indicator) ? def.indicator : "";
        if (locked && (def.lockPolicy === "unlock" || !page || restricted(page))) { unlockRequested(); return; }
        if (page) openIndicator(page);
        else if (def.settingsUrl) openUrl(def.settingsUrl);
    }

    GSettings { id: notifSettings; schema.id: "com.fairphone6.notifications" }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: column.height + cc.bottomInset + units.gu(2)
        clip: true
        visible: !cc.editing
        boundsBehavior: Flickable.DragOverBounds
        // pulling down again at the top expands the tiles (Android)
        onDragEnded: if (contentY < -units.gu(5) && grid.canExpand) grid.expanded = true

        Column {
            id: column
            x: units.gu(2)
            width: cc.width - units.gu(4)
            spacing: units.gu(1)

            // ---- date and time
            Item {
                width: parent.width
                height: units.gu(8)
                Column {
                    anchors.verticalCenter: parent.verticalCenter
                    Label {
                        id: clock
                        objectName: "qsClock"
                        font.pixelSize: units.gu(4.5)
                        font.weight: Font.Light
                        color: cc.fgColor
                    }
                    Label {
                        id: date
                        textSize: Label.Small
                        color: cc.fg2Color
                    }
                }
                Timer {
                    id: clockTimer
                    running: cc.visible
                    triggeredOnStart: true
                    repeat: true
                    onTriggered: {
                        var now = new Date();
                        clock.text = Qt.formatTime(now, Qt.locale().timeFormat(Locale.ShortFormat));
                        date.text = Qt.formatDate(now, "dddd, d MMMM");
                        interval = Math.max(1000, (60 - now.getSeconds()) * 1000 - now.getMilliseconds() + 50);
                    }
                }
                Row {
                    anchors { right: parent.right; verticalCenter: parent.verticalCenter }
                    spacing: units.gu(0.5)
                    AbstractButton {
                        objectName: "qsEditButton"
                        visible: !cc.locked
                        width: units.gu(5); height: width
                        onClicked: cc.editing = true
                        Rectangle { anchors.fill: parent; radius: width / 2; color: cc.cardColor }
                        Icon { anchors.centerIn: parent; name: "edit"; width: units.gu(2.25); height: width; color: cc.fgColor }
                    }
                    AbstractButton {
                        objectName: "qsSettingsButton"
                        width: units.gu(5); height: width
                        onClicked: cc.locked ? cc.unlockRequested() : cc.openUrl("settings:///system")
                        Rectangle { anchors.fill: parent; radius: width / 2; color: cc.cardColor }
                        Icon { anchors.centerIn: parent; name: "settings"; width: units.gu(2.25); height: width; color: cc.fgColor }
                    }
                }
            }

            // ---- tiles
            QsGrid {
                id: grid
                objectName: "qsGrid"
                width: parent.width
                backend: cc.backend
                lightMode: cc.lightMode
                locked: cc.locked
                onTileTapped: cc.tap(def)
                onTileDetail: cc.detail(def)
            }
            AbstractButton {
                objectName: "qsExpand"
                visible: grid.canExpand
                width: parent.width
                height: units.gu(3)
                onClicked: grid.expanded = !grid.expanded
                Icon {
                    anchors.centerIn: parent
                    name: "toolkit_chevron-down_2gu"
                    width: units.gu(2); height: width
                    rotation: grid.expanded ? 180 : 0
                    Behavior on rotation { LomiriNumberAnimation { duration: LomiriAnimation.FastDuration } }
                    color: cc.fg2Color
                }
            }

            // ---- sliders, in the configured order
            Rectangle {
                width: parent.width
                height: sliderCol.height + units.gu(1)
                radius: units.gu(2)
                color: cc.cardColor
                visible: sliderCol.height > 0
                Column {
                    id: sliderCol
                    y: units.gu(0.5)
                    width: parent.width
                    Repeater {
                        model: cc.backend.sliders
                        delegate: Loader {
                            width: sliderCol.width
                            // hidden (no height in the column) while its backend is missing
                            visible: !!item && (item.hasOwnProperty("hasPlayers") ? item.hasPlayers : item.available)
                            sourceComponent: modelData === "brightness" ? brightnessRow
                                           : modelData === "volume" ? volumeRow
                                           : modelData === "media" ? mediaRow : null
                        }
                    }
                }
            }

            // ---- notifications
            QsNotifications {
                width: parent.width
                visible: cc.backend.showNotifications && !!cc.notificationsModel
                model: cc.notificationsModel
                hideContent: cc.hideNotificationContent
                lightMode: cc.lightMode
                historyAvailable: !!notifSettings.historyEnabled
                onOpenApp: cc.openApp(appId)
                onHistoryRequested: cc.locked ? cc.unlockRequested() : cc.openUrl("settings:///system/do-not-disturb")
            }
        }
    }

    Component {
        id: brightnessRow
        QsSlider {
            objectName: "qsBrightness"
            action: cc.backend.brightnessAction
            minimum: 0.01
            iconMin: "display-brightness-min"
            iconMax: "display-brightness-max"
            lightMode: cc.lightMode
            trailing: AbstractButton {
                objectName: "qsAutoBrightness"
                readonly property bool on: cc.backend.on(cc.backend.autoBrightnessAction)
                visible: cc.backend.valid(cc.backend.autoBrightnessAction)
                width: units.gu(4.5); height: units.gu(4)
                onClicked: cc.backend.toggleAutoBrightness()
                Rectangle {
                    anchors.fill: parent; radius: height / 2
                    color: parent.on ? theme.palette.normal.focus : "transparent"
                    border.width: parent.on ? 0 : units.dp(1)
                    border.color: cc.fg2Color
                }
                Label { anchors.centerIn: parent; text: "A"; font.weight: Font.Medium; color: parent.on ? theme.palette.normal.focusText : cc.fgColor }
            }
        }
    }
    Component {
        id: volumeRow
        QsSlider {
            objectName: "qsVolume"
            action: cc.backend.volumeAction
            iconMin: "audio-volume-low"
            iconMax: "audio-volume-high"
            lightMode: cc.lightMode
        }
    }
    Component {
        id: mediaRow
        QsMedia {
            shown: cc.visible
            info: cc.backend.indicatorsRevision >= 0 ? cc.backend.findIndicator("ayatana-indicator-sound") : null
        }
    }

    Loader {
        id: editorLoader
        anchors.fill: parent
        active: cc.editing && !cc.locked
        sourceComponent: QsEditor {
            backend: cc.backend
            lightMode: cc.lightMode
            onDone: cc.editing = false
        }
    }
}
