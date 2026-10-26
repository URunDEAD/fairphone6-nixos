// Curated downloads from the projects' own servers, SHA-256 verified.
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3

Page {
    id: page
    property var catalog: root.backend_cmd("catalog")
    header: PageHeader {
        id: header
        title: i18n.tr("Download images")
    }
    ListView {
        id: list
        anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
        clip: true
        model: page.catalog.entries || []
        header: Column {
            width: list.width
            JobsColumn { filterKind: "download" }
            Notice {
                anchors { left: parent.left; right: parent.right; margins: units.gu(1.5) }
                visible: page.catalog.ok === false
                iconName: "security-alert"
                accent: theme.palette.normal.negative
                text: page.catalog.error || ""
            }
            ListItem {
                height: hint.height + units.gu(2)
                divider.visible: false
                Label {
                    id: hint
                    anchors { left: parent.left; right: parent.right; margins: units.gu(2); verticalCenter: parent.verticalCenter }
                    wrapMode: Text.Wrap
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundTertiaryText
                    text: i18n.tr("Only the projects' official servers (HTTPS). The newest image in the project's checksum list is downloaded and its SHA-256 checked; a mismatching file is deleted. Large files: use Wi-Fi.")
                }
            }
        }
        delegate: ListItem {
            height: lay.height + divider.height
            onClicked: PopupUtils.open(confirm, page, { entry: modelData })
            ListItemLayout {
                id: lay
                title.text: modelData.name
                subtitle.text: modelData.description + (modelData.size ? " · " + modelData.size : "")
                summary.text: modelData.host
                Icon {
                    SlotsLayout.position: SlotsLayout.Leading
                    name: "media-optical-symbolic"
                    width: units.gu(4); height: width
                }
                Icon {
                    SlotsLayout.position: SlotsLayout.Trailing
                    name: "save-to"
                    width: units.gu(2.5); height: width
                }
            }
        }
    }
    Component {
        id: confirm
        Dialog {
            id: d
            property var entry
            title: entry.name
            text: i18n.tr("Download from %1 (%2) into the image folder?").arg(entry.host).arg(entry.size)
            Button {
                text: i18n.tr("Download")
                color: theme.palette.normal.positive
                onClicked: { root.start("download", { id: d.entry.id }); PopupUtils.close(d); }
            }
            Button {
                text: i18n.tr("Cancel")
                onClicked: PopupUtils.close(d)
            }
        }
    }
}
