/*
 * FP6 quick settings: a tile. Small = round button with the label below;
 * wide = rounded pill with icon, title, state and a chevron for the detail.
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15
import Lomiri.Components 1.3

Item {
    id: tile
    property QtObject def: null
    property bool wide: false
    property bool lightMode: false
    property bool locked: false        // lock screen
    property bool editMode: false
    property bool inTray: false
    property real diameter: units.gu(6.5)
    property int labelLines: 1
    readonly property bool needsUnlock: locked && def && def.lockPolicy === "unlock"
    readonly property bool on: !!def && def.toggleable && def.active
    readonly property bool usable: !!def && def.available

    signal tapped()
    signal held()

    readonly property color offColor: lightMode ? "#E3E3E3" : "#333333"
    readonly property color onColor: theme.palette.normal.focus
    readonly property color onText: theme.palette.normal.focusText
    readonly property color fgColor: lightMode ? "#111111" : "#F7F7F7"
    readonly property color fg2Color: lightMode ? "#5D5D5D" : "#AEA79F"

    Rectangle {
        id: shape
        objectName: "tileShape"
        x: (wide ? units.gu(0.5) : (tile.width - width) / 2)
        y: units.gu(0.5)
        width: tile.wide ? tile.width - units.gu(1) : tile.diameter
        height: tile.diameter
        radius: height / 2
        color: tile.on ? tile.onColor : tile.offColor
        opacity: tile.usable ? 1 : 0.45
        Behavior on color { ColorAnimation { duration: LomiriAnimation.FastDuration } }
        scale: mouse.pressed && !tile.editMode ? 0.94 : 1
        Behavior on scale { NumberAnimation { duration: LomiriAnimation.SnapDuration } }

        Icon {
            id: icon
            source: tile.def && tile.def.iconSource != "" ? tile.def.iconSource : ""
            name: tile.def && tile.def.iconSource == "" ? (tile.on || !tile.def.toggleable ? tile.def.iconOn : tile.def.iconOff) : ""
            width: units.gu(2.75); height: width
            color: tile.on ? tile.onText : tile.fgColor
            anchors.verticalCenter: parent.verticalCenter
            x: tile.wide ? units.gu(2) : (parent.width - width) / 2
        }
        Column {
            visible: tile.wide
            anchors {
                left: icon.right; leftMargin: units.gu(1.5)
                right: chevron.left; rightMargin: units.gu(0.5)
                verticalCenter: parent.verticalCenter
            }
            Label {
                width: parent.width
                text: tile.def ? tile.def.label : ""
                textSize: Label.Small
                font.weight: Font.Medium
                elide: Text.ElideRight
                color: tile.on ? tile.onText : tile.fgColor
            }
            Label {
                width: parent.width
                // Android: the state (network name, device) is shown on the lock screen too
                text: !tile.def ? "" : (!tile.usable ? i18n.tr("Not available")
                      : (tile.def.subtitle || (tile.def.toggleable ? (tile.def.active ? i18n.tr("On") : i18n.tr("Off")) : "")))
                textSize: Label.XSmall
                elide: Text.ElideRight
                color: tile.on ? tile.onText : tile.fg2Color
                opacity: 0.85
            }
        }
        // a hint that holding opens more (the lock when the passcode is needed)
        Icon {
            id: chevron
            visible: tile.wide && !tile.editMode
            name: tile.needsUnlock ? "system-lock-screen" : "go-next"
            width: tile.wide ? units.gu(1.75) : 0; height: units.gu(1.75)
            color: tile.on ? tile.onText : tile.fg2Color
            anchors { right: parent.right; rightMargin: units.gu(1.5); verticalCenter: parent.verticalCenter }
        }
        Icon {
            // lock-screen badge
            visible: tile.needsUnlock && !tile.wide
            name: "system-lock-screen"
            width: units.gu(1.4); height: width
            color: tile.on ? tile.onText : tile.fg2Color
            anchors { right: parent.right; bottom: parent.bottom; margins: parent.height * 0.06 }
        }
    }

    Label {
        visible: !tile.wide
        anchors { top: shape.bottom; topMargin: units.gu(0.6); horizontalCenter: parent.horizontalCenter }
        width: Math.min(tile.width, units.gu(10))
        horizontalAlignment: Text.AlignHCenter
        text: tile.def ? tile.def.label : ""
        textSize: Label.XSmall
        wrapMode: tile.labelLines > 1 ? Text.Wrap : Text.NoWrap
        elide: Text.ElideRight
        maximumLineCount: tile.labelLines
        lineHeight: 0.9
        color: tile.fgColor
        opacity: tile.usable ? 1 : 0.5
    }

    MouseArea {
        id: mouse
        anchors.fill: shape
        enabled: !tile.editMode
        // tap = switch (or the tile's action), hold = details (indicator page / Settings)
        onClicked: tile.tapped()
        onPressAndHold: tile.held()
    }
}
