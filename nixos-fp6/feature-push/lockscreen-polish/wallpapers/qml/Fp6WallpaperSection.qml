/*
 * Copyright (C) 2026 Paul Schuldesz
 * SPDX-License-Identifier: GPL-3.0-only
 *
 * FP6 wallpapers (nixos-fp6/feature-push/lockscreen-polish): the lock and
 * home wallpapers side by side, plus the home-screen blur and dim.
 */
import QtQuick 2.12
import QtGraphicalEffects 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItem
import SystemSettings.ListItems 1.0 as SettingsListItems

Column {
    id: root
    objectName: "fp6WallpaperSection"

    property string lockSource
    property string homeSource
    property bool separateLock: false
    property bool busy: false
    property real homeDim: 0
    property bool homeBlur: false

    signal useHomeForLock()
    signal homeDimChosen(real value)
    signal homeBlurChosen(bool value)

    readonly property real cardWidth: Math.min(units.gu(16), (width - units.gu(8)) / 2)
    readonly property real cardHeight: cardWidth * 2484 / 1116

    Item { width: 1; height: units.gu(1) }

    Row {
        anchors.horizontalCenter: parent.horizontalCenter
        spacing: units.gu(4)

        // Lock screen
        Column {
            spacing: units.gu(1)
            Item {
                id: lockCard
                objectName: "fp6LockCard"
                width: root.cardWidth
                height: root.cardHeight
                Image {
                    id: lockImage
                    anchors.fill: parent
                    source: root.lockSource
                    sourceSize.height: height
                    fillMode: Image.PreserveAspectCrop
                    autoTransform: true
                    asynchronous: true
                    visible: false
                }
                OpacityMask {
                    anchors.fill: parent
                    source: lockImage
                    maskSource: Rectangle { width: lockCard.width; height: lockCard.height; radius: units.gu(1.5) }
                }
                Rectangle {
                    anchors.fill: parent
                    radius: units.gu(1.5)
                    color: "transparent"
                    border.color: theme.palette.normal.base
                    border.width: units.dp(1)
                }
                Label {
                    anchors.horizontalCenter: parent.horizontalCenter
                    y: parent.height * 0.14
                    text: Qt.formatTime(new Date(2026, 0, 1, 12, 30), Qt.DefaultLocaleShortDate)
                    color: "white"
                    font.weight: Font.Light
                    fontSize: "x-large"
                    style: Text.Raised
                    styleColor: "#60000000"
                }
                Icon {
                    anchors.horizontalCenter: parent.horizontalCenter
                    anchors.bottom: parent.bottom
                    anchors.bottomMargin: units.gu(1.5)
                    width: units.gu(2); height: width
                    name: "lock"
                    color: "white"
                }
                ActivityIndicator {
                    anchors.centerIn: parent
                    running: root.busy
                    visible: running
                }
            }
            Label {
                anchors.horizontalCenter: parent.horizontalCenter
                text: i18n.tr("Lock screen")
                fontSize: "small"
            }
            Label {
                anchors.horizontalCenter: parent.horizontalCenter
                width: root.cardWidth
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                text: root.separateLock ? "" : i18n.tr("Same as home")
                visible: text !== ""
                fontSize: "x-small"
                color: theme.palette.normal.backgroundTertiaryText
            }
        }

        // Home screen
        Column {
            spacing: units.gu(1)
            Item {
                id: homeCard
                objectName: "fp6HomeCard"
                width: root.cardWidth
                height: root.cardHeight
                Image {
                    id: homeImage
                    anchors.fill: parent
                    source: root.homeSource
                    sourceSize.height: height
                    fillMode: Image.PreserveAspectCrop
                    autoTransform: true
                    asynchronous: true
                    visible: false
                }
                FastBlur {
                    id: homeBlurred
                    anchors.fill: parent
                    source: homeImage
                    radius: root.homeBlur ? units.gu(1.5) : 0
                    visible: false
                }
                Item {
                    id: homeLayer
                    anchors.fill: parent
                    visible: false
                    ShaderEffectSource { anchors.fill: parent; sourceItem: homeBlurred }
                    Rectangle { anchors.fill: parent; color: "black"; opacity: root.homeDim }
                }
                OpacityMask {
                    anchors.fill: parent
                    source: homeLayer
                    maskSource: Rectangle { width: homeCard.width; height: homeCard.height; radius: units.gu(1.5) }
                }
                Rectangle {
                    anchors.fill: parent
                    radius: units.gu(1.5)
                    color: "transparent"
                    border.color: theme.palette.normal.base
                    border.width: units.dp(1)
                }
                // A hint of the launcher strip.
                Rectangle {
                    anchors { left: parent.left; top: parent.top; bottom: parent.bottom; margins: units.gu(0.6) }
                    width: units.gu(2.2)
                    radius: units.gu(0.6)
                    color: "#55000000"
                    Column {
                        anchors.horizontalCenter: parent.horizontalCenter
                        y: units.gu(1)
                        spacing: units.gu(0.6)
                        Repeater {
                            model: 5
                            Rectangle { width: units.gu(1.4); height: width; radius: units.gu(0.4); color: "#ccffffff" }
                        }
                    }
                }
            }
            Label {
                anchors.horizontalCenter: parent.horizontalCenter
                text: i18n.tr("Home screen")
                fontSize: "small"
            }
        }
    }

    Item { width: 1; height: units.gu(1) }

    ListItem.Caption {
        text: i18n.tr("Choose a picture below, then tap ✓ to set it for the home screen, the lock screen or both.")
    }

    SettingsListItems.Standard {
        objectName: "fp6UseHomeForLock"
        text: i18n.tr("Use home wallpaper on lock screen")
        visible: root.separateLock
        Button {
            text: i18n.tr("Use")
            enabled: !root.busy
            onClicked: root.useHomeForLock()
        }
    }

    SettingsListItems.Standard {
        objectName: "fp6HomeBlur"
        text: i18n.tr("Blur home wallpaper")
        Switch {
            checked: root.homeBlur
            onClicked: root.homeBlurChosen(checked)
        }
    }

    SettingsListItems.SingleValue {
        objectName: "fp6HomeDimLabel"
        text: i18n.tr("Dim home wallpaper")
        value: Math.round(dimSlider.value) + " %"
        showDivider: false
    }
    Slider {
        id: dimSlider
        objectName: "fp6HomeDim"
        anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
        minimumValue: 0
        maximumValue: 60
        stepSize: 5
        live: true
        value: root.homeDim * 100
        function formatValue(v) { return Math.round(v) + " %"; }
        onPressedChanged: if (!pressed) root.homeDimChosen(Math.round(value) / 100)
    }
    Item { width: 1; height: units.gu(2) }
}
