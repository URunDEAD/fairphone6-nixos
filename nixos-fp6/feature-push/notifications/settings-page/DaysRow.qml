/*
 * Seven day toggles (1 = Monday .. 7 = Sunday); emits changed(days).
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import Lomiri.Components 1.3

Row {
    id: row
    property var days: []
    property var names: []
    signal toggled(var days)
    spacing: units.gu(0.5)

    Repeater {
        model: 7
        delegate: AbstractButton {
            objectName: "day" + (index + 1)
            readonly property bool on: row.days.indexOf(index + 1) >= 0
            width: (row.width - 6 * row.spacing) / 7
            height: units.gu(4.5)
            Rectangle {
                anchors.fill: parent
                radius: units.gu(1)
                color: parent.on ? theme.palette.normal.focus : "transparent"
                border.width: units.dp(1)
                border.color: theme.palette.normal.base
            }
            Label {
                anchors.centerIn: parent
                text: row.names[index] || ""
                fontSize: "small"
                color: parent.on ? theme.palette.normal.focusText : theme.palette.normal.baseText
            }
            onClicked: {
                var out = []
                for (var i = 0; i < row.days.length; i++) if (row.days[i] !== index + 1) out.push(row.days[i])
                if (!on) out.push(index + 1)
                out.sort()
                row.toggled(out)
            }
        }
    }
}
