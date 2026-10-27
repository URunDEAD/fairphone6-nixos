/*
 * FP6 quick settings: a slider bound to a stateful indicator action whose
 * state is 0..1 (power "brightness", sound "volume"). Changes are sent with
 * SetState while dragging (at most every 50 ms) and once on release.
 * Copyright (C) 2026 Paul Schuldesz. GPL-3.0 (as Lomiri).
 */
import QtQuick 2.15
import Lomiri.Components 1.3

Item {
    id: row
    property var action: null
    property string iconMin
    property string iconMax
    property real minimum: 0
    property bool lightMode: false
    property alias trailing: trailingHolder.data
    readonly property bool available: !!action && action.valid
    readonly property real serverValue: available ? Number(action.state) : 0
    readonly property color fgColor: lightMode ? "#111111" : "#F7F7F7"

    implicitHeight: units.gu(6)
    visible: available

    function send() {
        if (available) action.updateState(Math.max(minimum, Math.min(1, slider.value)));
    }

    Icon {
        id: minIcon
        name: row.iconMin
        width: units.gu(2.25); height: width
        color: row.fgColor
        anchors { left: parent.left; leftMargin: units.gu(1); verticalCenter: parent.verticalCenter }
    }
    Slider {
        id: slider
        objectName: "qsSlider"
        anchors {
            left: minIcon.right; leftMargin: units.gu(1)
            right: maxIcon.left; rightMargin: units.gu(1)
            verticalCenter: parent.verticalCenter
        }
        minimumValue: 0
        maximumValue: 1
        live: true
        function formatValue(v) { return Math.round(v * 100) + "%"; }
        onValueChanged: if (pressed) throttle.start()
        onPressedChanged: if (!pressed) { throttle.stop(); row.send(); }
        Component.onCompleted: value = row.serverValue
    }
    Connections {
        target: row
        function onServerValueChanged() { if (!slider.pressed && !throttle.running) slider.value = row.serverValue; }
    }
    Timer {
        id: throttle
        interval: 50
        onTriggered: row.send()
    }
    Icon {
        id: maxIcon
        name: row.iconMax
        width: units.gu(2.25); height: width
        color: row.fgColor
        anchors { right: trailingHolder.left; rightMargin: trailingHolder.width > 0 ? units.gu(1) : 0; verticalCenter: parent.verticalCenter }
    }
    Item {
        id: trailingHolder
        anchors { right: parent.right; rightMargin: units.gu(0.5); verticalCenter: parent.verticalCenter }
        width: childrenRect.width
        height: childrenRect.height
    }
}
