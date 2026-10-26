/*
 * Fixed latitude/longitude for the sunset/sunrise schedule.
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3

Dialog {
    id: dialog
    property var gs
    title: i18n.tr("Fixed coordinates")
    text: i18n.tr("Degrees; north and east are positive (Bucharest: 44.43, 26.10).")

    function parse(t, lim) {
        var v = parseFloat(String(t).replace(",", "."))
        return (isNaN(v) || v < -lim || v > lim) ? null : v
    }

    TextField {
        id: lat
        objectName: "latitudeField"
        placeholderText: i18n.tr("Latitude")
        text: dialog.gs ? dialog.gs.latitude.toFixed(2) : ""
        inputMethodHints: Qt.ImhFormattedNumbersOnly
    }
    TextField {
        id: lon
        objectName: "longitudeField"
        placeholderText: i18n.tr("Longitude")
        text: dialog.gs ? dialog.gs.longitude.toFixed(2) : ""
        inputMethodHints: Qt.ImhFormattedNumbersOnly
    }
    Label {
        visible: !ok.enabled
        text: i18n.tr("Latitude -90 to 90, longitude -180 to 180.")
        color: theme.palette.normal.negative
        wrapMode: Text.WordWrap
    }
    Button {
        id: ok
        text: i18n.tr("Save")
        color: theme.palette.normal.positive
        enabled: dialog.parse(lat.text, 90) !== null && dialog.parse(lon.text, 180) !== null
        onClicked: {
            dialog.gs.latitude = dialog.parse(lat.text, 90)
            dialog.gs.longitude = dialog.parse(lon.text, 180)
            PopupUtils.close(dialog)
        }
    }
    Button {
        text: i18n.tr("Cancel")
        onClicked: PopupUtils.close(dialog)
    }
}
