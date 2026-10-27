/*
 * Copyright (C) 2026 Paul Schuldesz
 * SPDX-License-Identifier: GPL-3.0-only
 *
 * FP6 wallpapers: pick a picture from ~/Pictures (and its sub-folders)
 * without Content Hub. Selecting one opens the stock Preview page.
 */
import QtQuick 2.12
import Qt.labs.folderlistmodel 2.12
import SystemSettings 1.0
import Lomiri.Components 1.3

ItemPage {
    id: page
    objectName: "fp6PicturesPage"
    title: folderName

    property string folder
    property string rootFolder
    property var chooser   // the Background MainPage (has preview())
    readonly property string folderName: {
        var f = decodeURIComponent(folder.toString());
        if (f === rootFolder || f === decodeURIComponent(rootFolder)) return i18n.tr("Pictures");
        return f.substring(f.lastIndexOf("/") + 1);
    }

    FolderListModel {
        id: folders
        folder: page.folder
        showFiles: false
        showDirs: true
        showDotAndDotDot: false
        sortField: FolderListModel.Name
    }
    FolderListModel {
        id: pictures
        folder: page.folder
        showDirs: false
        nameFilters: [ "*.jpg", "*.jpeg", "*.png", "*.webp", "*.JPG", "*.JPEG", "*.PNG", "*.WEBP" ]
        caseSensitive: false
        sortField: FolderListModel.Time   // newest first
    }

    GridView {
        id: grid
        anchors.fill: parent
        anchors.topMargin: page.header ? page.header.height : 0
        anchors.leftMargin: units.gu(1)
        clip: true
        readonly property int columns: Math.max(3, Math.floor(width / units.gu(13)))
        cellWidth: Math.floor(width / columns)
        cellHeight: cellWidth
        model: pictures

        header: Column {
            width: grid.width
            Repeater {
                model: folders
                delegate: ListItem {
                    height: units.gu(6)
                    ListItemLayout {
                        anchors.centerIn: parent
                        title.text: fileName
                        Icon { name: "folder-symbolic"; width: units.gu(3); height: width; SlotsLayout.position: SlotsLayout.Leading }
                        ProgressionSlot { }
                    }
                    onClicked: pageStack.addPageToNextColumn(page, Qt.resolvedUrl("Fp6PicturesPage.qml"),
                                                             { folder: fileURL, rootFolder: page.rootFolder, chooser: page.chooser })
                }
            }
        }

        delegate: Item {
            width: grid.cellWidth
            height: grid.cellHeight
            Image {
                anchors.fill: parent
                anchors.margins: units.gu(0.5)
                source: fileURL
                sourceSize.width: width
                sourceSize.height: height
                fillMode: Image.PreserveAspectCrop
                autoTransform: true
                asynchronous: true
                cache: false
            }
            MouseArea {
                anchors.fill: parent
                onClicked: page.chooser.preview({ uri: fileURL.toString() })
            }
        }
    }

    Label {
        anchors.centerIn: parent
        visible: pictures.status === FolderListModel.Ready && pictures.count === 0 && folders.count === 0
        text: i18n.tr("No pictures here")
        color: theme.palette.normal.backgroundTertiaryText
    }
}
