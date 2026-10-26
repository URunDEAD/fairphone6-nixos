// One image: details, checksum, and how to share it.
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import "Fmt.js" as Fmt

Page {
    id: page
    property var image
    property bool verifyNow: false
    readonly property var info: image.info
    readonly property var modes: info.modes || []
    readonly property string mode: modes.length > 0 ? modes[modeSel.selectedIndex] : ""
    readonly property bool thisHosted: root.hosting && root.status.path === image.path
    property var verifyJob: null

    header: PageHeader {
        id: header
        title: image.name
        subtitle: Fmt.size(image.size)
    }

    readonly property var modeText: ({
        "cdrom": [i18n.tr("CD-ROM"), i18n.tr("Read-only optical drive. Best for installer and live ISO files.")],
        "disk-ro": [i18n.tr("USB disk, read-only"), i18n.tr("Looks like a USB stick the computer cannot change. Hybrid ISOs and disk images.")],
        "disk-rw": [i18n.tr("USB disk, read-write"), i18n.tr("The computer can write to the image (persistence, live USB with storage, installing onto it).")]
    })

    Connections {
        target: root
        function onJobFinished(job) {
            if (page.verifyJob !== null && job.id === page.verifyJob) { page.verifyJob = null; root.refreshLibrary(); page.refreshImage(); }
        }
    }
    function refreshImage() {
        var l = root.library.images;
        for (var i = 0; i < l.length; i++) if (l[i].path === image.path) image = l[i];
    }
    function verify() {
        var r = root.start("verify", { path: image.path, expected: sumField.text });
        if (r && r.ok) verifyJob = r.job;
    }
    Component.onCompleted: if (verifyNow) verify()

    Flickable {
        anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
        contentHeight: col.height + units.gu(4)
        clip: true

        Column {
            id: col
            width: parent.width

            ListItem {
                height: l1.height
                ListItemLayout {
                    id: l1
                    title.text: info.description
                    subtitle.text: (info.volumeId ? i18n.tr("Label: %1").arg(info.volumeId) + "  ·  " : "") + Fmt.size(image.size)
                    Icon {
                        SlotsLayout.position: SlotsLayout.Leading
                        name: Fmt.kindIcon(info.kind)
                        width: units.gu(4); height: width
                    }
                }
            }

            ListItem {
                height: l2.height
                ListItemLayout {
                    id: l2
                    title.text: i18n.tr("Checksum (SHA-256)")
                    subtitle.text: {
                        if (page.verifyJob !== null) return i18n.tr("Checking…");
                        var v = image.verified;
                        if (v && v.match === true) return i18n.tr("Verified against %1").arg(v.source);
                        if (v && v.match === false) return i18n.tr("Does NOT match %1. The file is damaged or not the one you wanted.").arg(v.source);
                        if (v) return Fmt.shortHash(v.sha256) + " " + i18n.tr("(no reference sum)");
                        if (image.sumFile) return i18n.tr("Sum available in %1").arg(image.sumFile.file);
                        return i18n.tr("Not checked. Paste the sum from the download page, or put a .sha256 file next to the image.");
                    }
                    subtitle.maximumLineCount: 3
                    subtitle.wrapMode: Text.Wrap
                    subtitle.color: image.verified && image.verified.match === false ? theme.palette.normal.negative
                                  : image.verified && image.verified.match === true ? theme.palette.normal.positive
                                  : theme.palette.normal.backgroundSecondaryText
                    Icon {
                        SlotsLayout.position: SlotsLayout.Leading
                        width: units.gu(3); height: width
                        name: image.verified && image.verified.match === true ? "tick"
                            : image.verified && image.verified.match === false ? "security-alert" : "info"
                    }
                }
            }
            Item {
                width: parent.width
                height: sumRow.height + units.gu(2)
                Row {
                    id: sumRow
                    anchors { left: parent.left; right: parent.right; margins: units.gu(2); verticalCenter: parent.verticalCenter }
                    spacing: units.gu(1)
                    TextField {
                        id: sumField
                        width: parent.width - verifyBtn.width - parent.spacing
                        placeholderText: image.sumFile ? i18n.tr("Use %1").arg(image.sumFile.file) : i18n.tr("Expected SHA-256 (optional)")
                        inputMethodHints: Qt.ImhNoPredictiveText | Qt.ImhNoAutoUppercase
                    }
                    Button {
                        id: verifyBtn
                        text: i18n.tr("Verify")
                        enabled: page.verifyJob === null
                        onClicked: page.verify()
                    }
                }
            }
            JobsColumn { filterKind: "verify" }

            ListItem {
                height: units.gu(5)
                divider.visible: false
                Label {
                    anchors { left: parent.left; leftMargin: units.gu(2); bottom: parent.bottom; bottomMargin: units.gu(1) }
                    text: i18n.tr("Share as")
                    font.weight: Font.DemiBold
                }
            }
            Item {
                width: parent.width
                height: modeSel.height + units.gu(1)
                OptionSelector {
                    id: modeSel
                    anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                    expanded: true
                    model: page.modes
                    delegate: OptionSelectorDelegate {
                        text: page.modeText[modelData][0]
                        subText: page.modeText[modelData][1]
                    }
                }
            }
            Notice {
                visible: page.mode === "disk-rw"
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                iconName: "security-alert"
                accent: theme.palette.normal.negative
                text: i18n.tr("The computer will change this image. Keep a copy if you need the original, and eject it on the computer before you stop sharing.")
            }
            Notice {
                visible: page.mode === "cdrom" && info.kind !== "iso" && info.kind !== "iso-hybrid"
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                text: i18n.tr("This is not an ISO 9660 image; as a CD-ROM most computers will not read it.")
            }
            Notice {
                visible: page.modes.length === 0
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                text: i18n.tr("This file cannot be shared as it is.")
            }
            Item { width: 1; height: units.gu(1) }

            ListItem {
                height: lx.height
                onClicked: excl.checked = !excl.checked
                ListItemLayout {
                    id: lx
                    title.text: i18n.tr("Exclusive mode")
                    subtitle.text: i18n.tr("Only the drive on USB, for computers that will not boot from a combined device. USB networking and ADB stop until you stop sharing or unplug the cable.")
                    subtitle.maximumLineCount: 4
                    subtitle.wrapMode: Text.Wrap
                    Switch {
                        id: excl
                        SlotsLayout.position: SlotsLayout.Trailing
                    }
                }
            }

            Item { width: 1; height: units.gu(2) }
            Button {
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                visible: !page.thisHosted
                enabled: page.modes.length > 0 && !root.hosting && !root.busy && root.status.available !== false
                text: root.busy ? i18n.tr("Starting…")
                      : root.hosting ? i18n.tr("Another image is being shared")
                      : i18n.tr("Start sharing")
                color: theme.palette.normal.positive
                onClicked: {
                    if (excl.checked || page.mode === "disk-rw")
                        PopupUtils.open(confirmDialog, page);
                    else
                        page.go();
                }
            }
            StatusCard {
                visible: page.thisHosted
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
            }
        }
    }

    function go() {
        root.start("host", { path: image.path, mode: page.mode, exclusive: excl.checked });
    }

    Component {
        id: confirmDialog
        Dialog {
            id: cd
            title: excl.checked ? i18n.tr("Exclusive mode") : i18n.tr("Read-write")
            text: (excl.checked ? i18n.tr("USB networking and ADB will be off while sharing. They come back when you stop, or automatically when you unplug the cable.") + "\n\n" : "")
                  + (page.mode === "disk-rw" ? i18n.tr("The computer can modify %1.").arg(image.name) : "")
            Button {
                text: i18n.tr("Start sharing")
                color: theme.palette.normal.positive
                onClicked: { PopupUtils.close(cd); page.go(); }
            }
            Button {
                text: i18n.tr("Cancel")
                onClicked: PopupUtils.close(cd)
            }
        }
    }
}
