/*
 * Small wrapped explanatory text with the page margins.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import Lomiri.Components 1.3

Label {
    anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
    wrapMode: Text.WordWrap
    fontSize: "small"
    visible: text !== ""
}
