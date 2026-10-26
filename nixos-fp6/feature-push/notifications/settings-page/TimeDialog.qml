/*
 * Hour/minute picker dialog; calls onAccepted(text "HH:MM").
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import Lomiri.Components.Pickers 1.3

Dialog {
    id: dialog
    property string time: "00:00"
    property var onAccepted: null

    DatePicker {
        id: picker
        objectName: "fp6TimePicker"
        mode: "Hours|Minutes"
        date: {
            var p = dialog.time.split(":")
            var d = new Date()
            d.setHours(parseInt(p[0]) || 0, parseInt(p[1]) || 0, 0, 0)
            return d
        }
    }
    Button {
        objectName: "fp6TimeOk"
        text: i18n.tr("OK")
        color: theme.palette.normal.positive
        onClicked: {
            if (dialog.onAccepted) dialog.onAccepted(Qt.formatTime(picker.date, "hh:mm"))
            PopupUtils.close(dialog)
        }
    }
    Button {
        text: i18n.tr("Cancel")
        onClicked: PopupUtils.close(dialog)
    }
}
