/*
 * Security & Privacy > Locking and unlocking > Lock after the display turns
 * off (FP6 privacy-lock). Lomiri locks this long after the display went off;
 * 0 = immediately (the default, as before).
 * SPDX-License-Identifier: GPL-3.0-only
 */
import QtQuick 2.4
import SystemSettings 1.0
import Lomiri.Components 1.3
import Lomiri.Components.ListItems 1.3 as ListItems

ItemPage {
    id: root
    objectName: "fp6LockGracePage"
    title: i18n.tr("Lock after the display turns off")

    property var settings
    readonly property var values: [0, 5, 15, 30, 60, 300, 900, 1800]

    function label(v) {
        if (v === 0) return i18n.tr("Immediately");
        if (v < 60) return i18n.tr("After %1 second", "After %1 seconds", v).arg(v);
        return i18n.tr("After %1 minute", "After %1 minutes", v / 60).arg(v / 60);
    }

    Column {
        anchors { left: parent.left; right: parent.right; top: parent.top }
        ListItems.ItemSelector {
            id: selector
            objectName: "fp6LockGraceSelector"
            expanded: true
            model: root.values.map(root.label)
            // ItemSelector resets selectedIndex when its model is set: bind afterwards.
            Component.onCompleted: selectedIndex = Qt.binding(function () { return Math.max(0, root.values.indexOf(root.settings ? (root.settings.lockGraceSeconds || 0) : 0)); })
            onDelegateClicked: root.settings.lockGraceSeconds = root.values[index]
        }
        ListItems.Caption {
            text: i18n.tr("Turning the display back on within this time skips the lock screen. The power button still locks at once unless you turn that off. When the phone is idle, the display turns off after the time set in \"Lock when idle\".")
        }
    }
}
