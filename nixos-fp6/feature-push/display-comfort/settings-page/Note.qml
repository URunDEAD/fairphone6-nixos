/*
 * Small wrapped explanatory text with the page margins and a little space
 * below, so it does not touch the next divider at large text sizes.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import Lomiri.Components 1.3

Item {
    property alias text: label.text
    property alias font: label.font
    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
    height: visible ? label.height + units.gu(1) : 0
    visible: label.text !== ""

    Label {
        id: label
        anchors { left: parent.left; right: parent.right; top: parent.top }
        wrapMode: Text.WordWrap
        fontSize: "small"
    }
}
