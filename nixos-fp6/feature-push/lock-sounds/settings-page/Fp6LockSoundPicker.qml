/*
 * Copyright (C) 2026 Paul Schuldesz
 * SPDX-License-Identifier: GPL-3.0-only
 *
 * Settings > Sound > Lock sound / Unlock sound (FP6): None, the bundled
 * sounds, the current custom file and "Custom..." (Music / Ringtones).
 * Tapping a sound selects and plays it; Preview plays the selection.
 */
import QtQuick 2.12
import GSettings 1.0
import SystemSettings 1.0
import Lomiri.Components 1.3
import "Fp6LockSoundCatalog.js" as Catalog

ItemPage {
    id: picker
    objectName: "fp6LockSoundPicker"
    flickable: flick

    property string key: "lockSound"     // GSettings property: lockSound / unlockSound
    readonly property string current: gs[key] !== undefined ? gs[key] : ""

    GSettings {
        id: gs
        schema.id: "com.fairphone6.lock-sounds"
    }

    function choose(value) {
        gs[key] = value;
        preview(value);
    }
    function preview(value) {
        player.source = Catalog.resolve(value, gs.preparedFiles);
        player.play();
    }

    Fp6LockSoundPlayer { id: player }

    // None, bundled sounds, then the custom file if one is chosen.
    readonly property var entries: {
        var e = [{ value: "", name: i18n.tr("None") }];
        for (var i = 0; i < Catalog.sounds.length; i++)
            e.push({ value: Catalog.sounds[i].id, name: i18n.tr(Catalog.sounds[i].name) });
        if (Catalog.isCustom(current))
            e.push({ value: current, name: Catalog.fileName(current), custom: true });
        return e;
    }

    Flickable {
        id: flick
        anchors.fill: parent
        anchors.bottomMargin: previewBar.height
        contentHeight: list.height
        clip: true
        boundsBehavior: (contentHeight > height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick

        Column {
            id: list
            anchors { left: parent.left; right: parent.right }

            Repeater {
                model: picker.entries
                delegate: ListItem {
                    objectName: "fp6LockSoundEntry_" + (modelData.custom ? "custom" : (modelData.value || "none"))
                    height: layout.height + (divider.visible ? divider.height : 0)
                    onClicked: picker.choose(modelData.value)
                    ListItemLayout {
                        id: layout
                        title.text: modelData.name
                        subtitle.text: modelData.custom ? modelData.value : ""
                        Icon {
                            SlotsLayout.position: SlotsLayout.Trailing
                            width: units.gu(2); height: width
                            name: "tick"
                            color: theme.palette.normal.focus
                            visible: picker.current === modelData.value
                        }
                    }
                }
            }

            ListItem {
                objectName: "fp6LockSoundCustom"
                height: customLayout.height + (divider.visible ? divider.height : 0)
                onClicked: pageStack.addPageToNextColumn(picker, Qt.resolvedUrl("Fp6LockSoundFiles.qml"),
                                                         { picker: picker })
                ListItemLayout {
                    id: customLayout
                    title.text: i18n.tr("Custom…")
                    subtitle.text: i18n.tr("A sound from Music or Ringtones")
                    ProgressionSlot {}
                }
            }

            Label {
                anchors { left: parent.left; right: parent.right; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                wrapMode: Text.WordWrap
                fontSize: "small"
                color: theme.palette.normal.backgroundSecondaryText
                text: i18n.tr("Played at the output volume, not in Silent Mode, with Do Not Disturb on or during calls. Long files are cut to their first 3 seconds.")
            }
            Item { width: 1; height: units.gu(2) }
        }
    }

    Rectangle {
        id: previewBar
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        height: units.gu(7)
        color: theme.palette.normal.background
        Button {
            objectName: "fp6LockSoundPreview"
            anchors.centerIn: parent
            width: Math.min(parent.width - units.gu(4), units.gu(24))
            enabled: picker.current !== ""
            text: player.playing ? i18n.tr("Stop") : i18n.tr("Preview")
            iconName: player.playing ? "media-playback-stop" : "media-playback-start"
            onClicked: player.playing ? player.stop() : picker.preview(picker.current)
        }
    }

    Component.onDestruction: player.stop()
}
