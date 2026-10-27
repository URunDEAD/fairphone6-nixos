/*
 * FP6 quick settings: notifications kept in the panel until dismissed,
 * grouped by app (newest group first). Swipe a card (or a group header)
 * sideways to dismiss it; "Clear all" empties the list. The list lives in
 * the shell's memory only; the optional on-disk history is fp6-notifyd's
 * (Settings > Do Not Disturb > Notification history).
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15
import Lomiri.Components 1.3

Column {
    id: list
    property var model: null             // ListModel kept by Shell.qml
    property bool hideContent: false     // lock screen with "hide notification content"
    property bool lightMode: false
    property bool historyAvailable: false
    signal openApp(string appId)
    signal historyRequested()

    readonly property int count: model ? model.count : 0
    readonly property color cardColor: lightMode ? Qt.rgba(0, 0, 0, 0.06) : Qt.rgba(1, 1, 1, 0.09)
    readonly property color fgColor: lightMode ? "#111111" : "#F7F7F7"
    readonly property color fg2Color: lightMode ? "#5D5D5D" : "#AEA79F"
    spacing: units.gu(0.5)

    function removeGroup(app) {
        for (var i = model.count - 1; i >= 0; i--) if (model.get(i).app === app) model.remove(i);
    }

    Item {
        width: parent.width
        height: units.gu(4)
        Label {
            anchors { left: parent.left; leftMargin: units.gu(1); verticalCenter: parent.verticalCenter }
            text: list.count > 0 ? i18n.tr("Notifications") : i18n.tr("No notifications")
            textSize: Label.Small
            font.weight: Font.Medium
            color: list.fg2Color
        }
        Row {
            anchors { right: parent.right; verticalCenter: parent.verticalCenter }
            spacing: units.gu(1)
            AbstractButton {
                visible: list.historyAvailable
                width: histLabel.width + units.gu(2); height: units.gu(4)
                onClicked: list.historyRequested()
                Label { id: histLabel; anchors.centerIn: parent; text: i18n.tr("History"); textSize: Label.Small; color: theme.palette.normal.focus }
            }
            AbstractButton {
                objectName: "qsClearAll"
                visible: list.count > 0
                width: clearLabel.width + units.gu(2); height: units.gu(4)
                onClicked: list.model.clear()
                Label { id: clearLabel; anchors.centerIn: parent; text: i18n.tr("Clear all"); textSize: Label.Small; color: theme.palette.normal.focus }
            }
        }
    }

    Repeater {
        model: list.model
        delegate: Column {
            id: entry
            width: list.width
            readonly property bool groupStart: index === 0 || list.model.get(index - 1).app !== model.app
            spacing: units.gu(0.5)

            // group header: app name, count; swipe or × clears the group
            Item {
                visible: entry.groupStart
                width: parent.width
                height: visible ? units.gu(3.5) : 0
                Label {
                    anchors { left: parent.left; leftMargin: units.gu(1); verticalCenter: parent.verticalCenter }
                    width: parent.width - units.gu(6)
                    text: model.app
                    textSize: Label.Small
                    font.weight: Font.Medium
                    color: list.fgColor
                    elide: Text.ElideRight
                }
                AbstractButton {
                    anchors { right: parent.right; verticalCenter: parent.verticalCenter }
                    width: units.gu(4); height: units.gu(3.5)
                    onClicked: list.removeGroup(model.app)
                    Icon { anchors.centerIn: parent; name: "close"; width: units.gu(1.75); height: width; color: list.fg2Color }
                }
            }

            Item {
                width: parent.width
                height: card.height
                clip: false
                Rectangle {
                    id: card
                    width: parent.width
                    height: Math.max(units.gu(6.5), textCol.height + units.gu(2))
                    radius: units.gu(1.5)
                    color: list.cardColor
                    opacity: 1 - Math.min(0.8, Math.abs(x) / width)
                    Behavior on x { id: xAnim; enabled: !swipe.drag.active; LomiriNumberAnimation { duration: LomiriAnimation.FastDuration } }

                    Item {
                        id: iconBox
                        x: units.gu(1.25); y: units.gu(1.25)
                        width: units.gu(4); height: width
                        Image {
                            id: img
                            anchors.fill: parent
                            source: model.icon || ""
                            sourceSize { width: width; height: height }
                            fillMode: Image.PreserveAspectFit
                            asynchronous: true
                            visible: status == Image.Ready
                        }
                        Icon {
                            anchors.fill: parent
                            visible: img.status != Image.Ready
                            name: "notification"
                            color: list.fg2Color
                        }
                    }
                    Column {
                        id: textCol
                        anchors { left: iconBox.right; leftMargin: units.gu(1.25); right: parent.right; rightMargin: units.gu(1.25) }
                        y: units.gu(1)
                        Item {
                            width: parent.width
                            height: summary.height
                            Label {
                                id: summary
                                width: parent.width - when.width - units.gu(1)
                                text: list.hideContent ? model.app : model.summary
                                textSize: Label.Small
                                font.weight: Font.Medium
                                elide: Text.ElideRight
                                color: list.fgColor
                            }
                            Label {
                                id: when
                                anchors.right: parent.right
                                text: Qt.formatTime(new Date(model.time), Qt.locale().timeFormat(Locale.ShortFormat))
                                textSize: Label.XSmall
                                color: list.fg2Color
                            }
                        }
                        Label {
                            width: parent.width
                            visible: text !== ""
                            text: list.hideContent ? i18n.tr("Unlock to see the content") : model.body
                            textSize: Label.XSmall
                            wrapMode: Text.Wrap
                            maximumLineCount: 3
                            elide: Text.ElideRight
                            color: list.fg2Color
                            textFormat: Text.PlainText
                        }
                    }
                    Timer {
                        id: dismissTimer
                        interval: LomiriAnimation.FastDuration
                        onTriggered: if (index >= 0 && index < list.model.count) list.model.remove(index)
                    }
                    MouseArea {
                        id: swipe
                        anchors.fill: parent
                        drag.target: card
                        drag.axis: Drag.XAxis
                        drag.threshold: units.gu(1.5)
                        onClicked: if (model.appId) list.openApp(model.appId)
                        onReleased: {
                            if (Math.abs(card.x) > card.width * 0.35) {
                                card.x = card.x > 0 ? card.width : -card.width;
                                dismissTimer.start();
                            } else {
                                card.x = 0;
                            }
                        }
                    }
                }
            }
        }
    }
}
