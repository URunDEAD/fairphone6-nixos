// Settings: image folder; what the service allows.
import QtQuick 2.12
import Lomiri.Components 1.3

Page {
    id: page
    header: PageHeader {
        id: header
        title: i18n.tr("Settings")
    }
    readonly property var allowed: root.status.allowedDirs || []
    function inside(d) {
        for (var i = 0; i < allowed.length; i++)
            if (d === allowed[i] || d.indexOf(allowed[i] + "/") === 0) return true;
        return false;
    }
    Flickable {
        anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
        contentHeight: col.height + units.gu(4)
        clip: true
        Column {
            id: col
            anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
            spacing: units.gu(1.5)
            Label { text: i18n.tr("Image folder"); font.weight: Font.DemiBold }
            TextField {
                id: dir
                width: parent.width
                text: root.settings.libraryDir
                inputMethodHints: Qt.ImhNoPredictiveText | Qt.ImhNoAutoUppercase
            }
            Notice {
                visible: page.allowed.length > 0 && !page.inside(dir.text)
                iconName: "security-alert"
                accent: theme.palette.normal.negative
                text: i18n.tr("Images in this folder can be listed but not shared: the USB service only shares files inside %1.").arg(page.allowed.join(", "))
            }
            Row {
                spacing: units.gu(1)
                Button {
                    text: i18n.tr("Save")
                    color: theme.palette.normal.positive
                    onClicked: {
                        root.start("setSettings", { libraryDir: dir.text });
                        root.settings = root.backend_cmd("settings");
                        root.refreshLibrary();
                    }
                }
                Button {
                    text: i18n.tr("Default")
                    onClicked: dir.text = root.settings.defaultLibraryDir
                }
            }
            Label { text: i18n.tr("Safety"); font.weight: Font.DemiBold }
            Label {
                width: parent.width
                wrapMode: Text.Wrap
                textSize: Label.Small
                color: theme.palette.normal.backgroundSecondaryText
                text: i18n.tr("A system service adds the drive to the phone's USB connection next to USB networking and ADB, and removes it again when you stop. It only accepts your own regular files inside the allowed folders (no symbolic links). Exclusive mode removes the other USB functions and puts them back when you stop, when the cable is unplugged, or after 4 hours.")
            }
            Label {
                width: parent.width
                wrapMode: Text.Wrap
                textSize: Label.Small
                color: theme.palette.normal.backgroundTertiaryText
                text: i18n.tr("Allowed folders: %1").arg(page.allowed.length ? page.allowed.join(", ") : i18n.tr("(service not reachable)"))
                      + "\n" + i18n.tr("USB functions now: %1").arg((root.status.functions || []).join(", "))
                      + "\n" + i18n.tr("Download list: %1").arg(root.settings.catalog)
            }
        }
    }
}
