/*
 * Copyright (C) 2026 Paul Schuldesz
 * SPDX-License-Identifier: GPL-3.0-only
 *
 * Settings > Sound > Lock sound > Custom... (FP6): audio files in ~/Music
 * and ~/Ringtones (and their sub-folders, and ringtones imported by the
 * stock Ringtone page). Tapping a file selects it, plays it and returns to
 * the picker; fp6-lock-sounds then prepares a short WAV of it in the
 * background for Lomiri.
 */
import QtQuick 2.12
import Qt.labs.folderlistmodel 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3

ItemPage {
    id: page
    objectName: "fp6LockSoundFiles"
    title: folder === "" ? i18n.tr("Custom sound") : folderName
    flickable: flick

    property var picker                  // Fp6LockSoundPicker
    property var rootPage: page          // the first Custom page (removed with its children)
    property string folder: ""           // file:// URL, "" = the start folders
    property string home: "@fp6Home@"

    readonly property string folderName: {
        var f = decodeURIComponent(folder.toString());
        return f.substring(f.lastIndexOf("/") + 1);
    }
    readonly property var roots: [
        { name: i18n.tr("Music"), url: "file://" + home + "/Music" },
        { name: i18n.tr("Ringtones"), url: "file://" + home + "/Ringtones" },
        { name: i18n.tr("Imported ringtones"), url: "file://" + home + "/.local/share/lomiri-system-settings/Music" }
    ]

    function choose(url) {
        var path = decodeURIComponent(url.toString().replace(/^file:\/\//, ""));
        picker.choose(path);
        pageStack.removePages(rootPage); // back to the picker
    }

    FolderListModel {
        id: folders
        folder: page.folder
        showFiles: false
        showDirs: true
        showDotAndDotDot: false
        showHidden: false
        sortField: FolderListModel.Name
    }
    FolderListModel {
        id: files
        folder: page.folder
        showDirs: false
        caseSensitive: false
        nameFilters: [ "*.ogg", "*.oga", "*.mp3", "*.wav", "*.flac", "*.m4a", "*.aac", "*.opus" ]
        sortField: FolderListModel.Name
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentHeight: list.height
        boundsBehavior: (contentHeight > height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick

        Column {
            id: list
            anchors { left: parent.left; right: parent.right }

            Repeater {
                model: page.folder === "" ? page.roots : folders
                delegate: ListItem {
                    readonly property string url: page.folder === "" ? modelData.url : fileURL
                    height: rowLayout.height + (divider.visible ? divider.height : 0)
                    onClicked: pageStack.addPageToNextColumn(page, Qt.resolvedUrl("Fp6LockSoundFiles.qml"),
                                                             { folder: url, picker: page.picker, rootPage: page.rootPage, home: page.home })
                    ListItemLayout {
                        id: rowLayout
                        title.text: page.folder === "" ? modelData.name : fileName
                        Icon { name: "folder-symbolic"; width: units.gu(3); height: width; SlotsLayout.position: SlotsLayout.Leading }
                        ProgressionSlot {}
                    }
                }
            }

            Repeater {
                model: page.folder === "" ? null : files
                delegate: ListItem {
                    objectName: "fp6LockSoundFile_" + index
                    height: fileLayout.height + (divider.visible ? divider.height : 0)
                    onClicked: page.choose(fileURL)
                    ListItemLayout {
                        id: fileLayout
                        title.text: fileBaseName
                        subtitle.text: fileSuffix.toUpperCase()
                        Icon { name: "audio-x-generic-symbolic"; width: units.gu(3); height: width; SlotsLayout.position: SlotsLayout.Leading }
                    }
                }
            }
        }
    }

    Label {
        anchors.centerIn: parent
        width: parent.width - units.gu(4)
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.WordWrap
        visible: page.folder !== "" && files.status === FolderListModel.Ready && files.count === 0 && folders.count === 0
        text: i18n.tr("No sounds here")
        color: theme.palette.normal.backgroundTertiaryText
    }
}
