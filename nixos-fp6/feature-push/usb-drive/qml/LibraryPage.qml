// Image library: images in the library folder, hosting status, tasks.
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import "Fmt.js" as Fmt

Page {
    id: page
    header: PageHeader {
        id: header
        title: i18n.tr("USB Drive")
        trailingActionBar.actions: [
            Action {
                iconName: "settings"
                text: i18n.tr("Settings")
                onTriggered: stack.push(Qt.resolvedUrl("SettingsPage.qml"))
            },
            Action {
                iconName: "save-to"
                text: i18n.tr("Download")
                onTriggered: stack.push(Qt.resolvedUrl("DownloadsPage.qml"))
            },
            Action {
                iconName: "reload"
                text: i18n.tr("Refresh")
                onTriggered: { root.refreshLibrary(); root.refreshStatus(); }
            }
        ]
    }

    function open(img) {
        if (img.info.kind === "compressed")
            PopupUtils.open(decompressDialog, page, { image: img });
        else
            stack.push(Qt.resolvedUrl("HostPage.qml"), { image: img });
    }

    ListView {
        id: list
        anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
        clip: true
        model: root.library.images
        header: Column {
            width: list.width
            spacing: units.gu(1)
            Item { width: 1; height: units.gu(0.5) }
            StatusCard {
                visible: root.hosting
                anchors { left: parent.left; right: parent.right; margins: units.gu(1.5) }
            }
            Notice {
                visible: root.status.available === false
                anchors { left: parent.left; right: parent.right; margins: units.gu(1.5) }
                iconName: "security-alert"
                accent: theme.palette.normal.negative
                text: i18n.tr("The USB Drive service is not available: %1").arg(root.status.error || "")
            }
            Notice {
                visible: !root.hosting && root.status.lastStopReason !== undefined
                         && root.status.lastStopReason.indexOf("unplugged") >= 0
                anchors { left: parent.left; right: parent.right; margins: units.gu(1.5) }
                text: i18n.tr("Sharing stopped: %1. USB networking is back.").arg(root.status.lastStopReason || "")
            }
            JobsColumn { width: parent.width }
            ListItem {
                height: units.gu(4)
                divider.visible: false
                Label {
                    anchors { left: parent.left; leftMargin: units.gu(2); verticalCenter: parent.verticalCenter }
                    text: root.library.dir + (root.library.free !== undefined && root.library.free !== null
                          ? "  ·  " + i18n.tr("%1 free").arg(Fmt.size(root.library.free)) : "")
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundTertiaryText
                    elide: Text.ElideMiddle
                    width: parent.width - units.gu(4)
                }
            }
        }

        delegate: ListItem {
            id: li
            readonly property var img: modelData
            readonly property bool isHosted: root.hosting && root.status.path === img.path
            height: lay.height + (divider.visible ? divider.height : 0)
            onClicked: page.open(img)
            leadingActions: ListItemActions {
                actions: [
                    Action {
                        iconName: "delete"
                        text: i18n.tr("Delete")
                        enabled: !li.isHosted
                        onTriggered: PopupUtils.open(deleteDialog, page, { image: li.img })
                    }
                ]
            }
            trailingActions: ListItemActions {
                actions: [
                    Action {
                        iconName: "tick"
                        text: i18n.tr("Verify checksum")
                        visible: li.img.info.kind !== "compressed"
                        onTriggered: stack.push(Qt.resolvedUrl("HostPage.qml"), { image: li.img, verifyNow: true })
                    },
                    Action {
                        iconName: "import"
                        text: i18n.tr("Decompress")
                        visible: li.img.info.kind === "compressed"
                        onTriggered: PopupUtils.open(decompressDialog, page, { image: li.img })
                    }
                ]
            }
            ListItemLayout {
                id: lay
                title.text: img.name
                title.elide: Text.ElideMiddle
                subtitle.text: img.info.description || ""
                summary.text: {
                    var s = Fmt.size(img.size);
                    if (img.verified && img.verified.match === true) s += " · " + i18n.tr("SHA-256 verified");
                    else if (img.verified && img.verified.match === false) s += " · " + i18n.tr("checksum MISMATCH");
                    else if (img.sumFile) s += " · " + i18n.tr("checksum available");
                    if (li.isHosted) s += " · " + i18n.tr("sharing now");
                    return s;
                }
                summary.color: img.verified && img.verified.match === false ? theme.palette.normal.negative
                             : li.isHosted ? theme.palette.normal.positive : theme.palette.normal.backgroundTertiaryText
                Icon {
                    SlotsLayout.position: SlotsLayout.Leading
                    width: units.gu(4); height: width
                    name: Fmt.kindIcon(img.info.kind)
                    color: li.isHosted ? theme.palette.normal.positive : theme.palette.normal.backgroundText
                }
                ProgressionSlot {}
            }
        }

        footer: Item {
            width: list.width
            height: empty.visible ? empty.height + units.gu(4) : units.gu(2)
            Column {
                id: empty
                visible: root.library.images.length === 0
                anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(3); topMargin: units.gu(4) }
                spacing: units.gu(2)
                Icon {
                    anchors.horizontalCenter: parent.horizontalCenter
                    name: "media-optical-symbolic"
                    width: units.gu(8); height: width
                    color: theme.palette.normal.backgroundTertiaryText
                }
                Label {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.Wrap
                    text: root.library.exists
                          ? i18n.tr("No images yet. Put .iso or .img files in %1, download one, or create a blank image.").arg(root.library.dir)
                          : i18n.tr("The image folder %1 does not exist yet.").arg(root.library.dir)
                    color: theme.palette.normal.backgroundSecondaryText
                }
                Button {
                    anchors.horizontalCenter: parent.horizontalCenter
                    visible: !root.library.exists
                    text: i18n.tr("Create folder")
                    onClicked: { root.start("makeLibrary"); root.refreshLibrary(); }
                }
                Button {
                    anchors.horizontalCenter: parent.horizontalCenter
                    text: i18n.tr("Download an image")
                    color: theme.palette.normal.positive
                    onClicked: stack.push(Qt.resolvedUrl("DownloadsPage.qml"))
                }
            }
        }
    }

    BottomEdge {
        id: bottomEdge
        height: page.height
        hint.text: i18n.tr("Create blank image")
        hint.iconName: "add"
        preloadContent: false
        contentUrl: Qt.resolvedUrl("CreatePage.qml")
        onCommitCompleted: if (contentItem) contentItem.bottomEdgeRef = bottomEdge
    }

    Component {
        id: deleteDialog
        Dialog {
            id: d
            property var image
            title: i18n.tr("Delete image?")
            text: i18n.tr("%1 (%2) will be deleted from the phone.").arg(image.name).arg(Fmt.size(image.size))
            Button {
                text: i18n.tr("Delete")
                color: theme.palette.normal.negative
                onClicked: { root.start("delete", { path: d.image.path }); root.refreshLibrary(); PopupUtils.close(d); }
            }
            Button {
                text: i18n.tr("Cancel")
                onClicked: PopupUtils.close(d)
            }
        }
    }

    Component {
        id: decompressDialog
        Dialog {
            id: dd
            property var image
            title: i18n.tr("Decompress image?")
            text: i18n.tr("%1 is compressed (%2). It has to be decompressed before it can be shared. This needs more free space than the compressed file.")
                  .arg(image.name).arg(image.info.compression)
            CheckBox {
                id: removeSrc
                checked: false
            }
            Label {
                text: i18n.tr("Delete the compressed file afterwards")
                wrapMode: Text.Wrap
            }
            Button {
                text: i18n.tr("Decompress")
                color: theme.palette.normal.positive
                onClicked: { root.start("decompress", { path: dd.image.path, removeSource: removeSrc.checked }); PopupUtils.close(dd); }
            }
            Button {
                text: i18n.tr("Cancel")
                onClicked: PopupUtils.close(dd)
            }
        }
    }
}
