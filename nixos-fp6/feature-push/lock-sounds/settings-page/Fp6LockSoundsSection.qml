/*
 * Copyright (C) 2026 Paul Schuldesz
 * SPDX-License-Identifier: GPL-3.0-only
 *
 * Settings > Sound > Other sounds: Lock sounds (FP6). Replaces the stock
 * page's placeholder "Lock sound" switch (showAllUI only, not wired).
 * com.fairphone6.lock-sounds, read live by Lomiri (Fp6LockSounds.qml).
 */
import QtQuick 2.12
import GSettings 1.0
import Lomiri.Components 1.3
import SystemSettings.ListItems 1.0 as SettingsListItems
import "Fp6LockSoundCatalog.js" as Catalog

Column {
    id: section
    objectName: "fp6LockSoundsSection"

    property var page               // the Sound page (for pageStack)

    GSettings {
        id: gs
        schema.id: "com.fairphone6.lock-sounds"
    }

    function displayName(v) {
        if (!v) return i18n.tr("None");
        if (Catalog.isCustom(v)) return Catalog.fileName(v);
        var s = Catalog.find(v);
        return s ? i18n.tr(s.name) : i18n.tr("None");
    }

    function openPicker(key, title) {
        pageStack.addPageToNextColumn(section.page, Qt.resolvedUrl("Fp6LockSoundPicker.qml"),
                                      { title: title, key: key });
    }

    SettingsListItems.Standard {
        text: i18n.tr("Lock sounds")
        divider.visible: false
        Switch {
            objectName: "fp6LockSoundsSwitch"
            property bool serverChecked: gs.enabled === true
            onServerCheckedChanged: checked = serverChecked
            Component.onCompleted: checked = serverChecked
            onTriggered: gs.enabled = checked
        }
    }
    SettingsListItems.ThinDivider {}

    SettingsListItems.SingleValueProgression {
        objectName: "fp6LockSound"
        text: i18n.tr("Lock sound")
        value: section.displayName(gs.lockSound)
        enabled: gs.enabled === true
        divider.visible: false
        onClicked: section.openPicker("lockSound", i18n.tr("Lock sound"))
    }
    SettingsListItems.ThinDivider {}

    SettingsListItems.SingleValueProgression {
        objectName: "fp6UnlockSound"
        text: i18n.tr("Unlock sound")
        value: section.displayName(gs.unlockSound)
        enabled: gs.enabled === true
        divider.visible: false
        onClicked: section.openPicker("unlockSound", i18n.tr("Unlock sound"))
    }
    SettingsListItems.ThinDivider {}
}
