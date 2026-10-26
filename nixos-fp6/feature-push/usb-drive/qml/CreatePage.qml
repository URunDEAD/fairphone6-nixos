// Create a blank image (bottom edge): name, size, optional FAT32/exFAT.
import QtQuick 2.12
import Lomiri.Components 1.3
import "Fmt.js" as Fmt

Page {
    id: page
    property var bottomEdgeRef: null
    readonly property var sizes: [256, 1024, 4096, 8192, 16384, 32768, 0]
    readonly property var sizeNames: ["256 MiB", "1 GiB", "4 GiB", "8 GiB", "16 GiB", "32 GiB", i18n.tr("Custom")]
    readonly property var fsKeys: ["fat32", "exfat", "none"]
    readonly property int sizeMiB: sizes[sizeSel.selectedIndex] > 0 ? sizes[sizeSel.selectedIndex]
                                   : Math.round(parseFloat(customSize.text || "0") * 1024)
    property var job: null

    function close() {
        if (bottomEdgeRef) bottomEdgeRef.collapse();
        else stack.pop();
    }

    header: PageHeader {
        id: header
        title: i18n.tr("Create blank image")
        leadingActionBar.actions: [
            Action {
                iconName: "close"
                text: i18n.tr("Close")
                onTriggered: page.close()
            }
        ]
    }

    Connections {
        target: root
        function onJobFinished(job) {
            if (page.job === null || job.id !== page.job) return;
            page.job = null;
            if (job.status === "done") { root.refreshLibrary(); page.close(); }
            else root.showError(job.message);
        }
    }

    Flickable {
        anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
        contentHeight: col.height + units.gu(4)
        clip: true
        Column {
            id: col
            anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
            spacing: units.gu(1.5)

            Label { text: i18n.tr("Name") }
            TextField {
                id: nameField
                width: parent.width
                text: "usb-drive"
                inputMethodHints: Qt.ImhNoPredictiveText | Qt.ImhNoAutoUppercase
                validator: RegExpValidator { regExp: /[A-Za-z0-9._+~-]{1,120}/ }
            }
            Label { text: i18n.tr("Size") }
            OptionSelector {
                id: sizeSel
                width: parent.width
                model: page.sizeNames
                selectedIndex: 2
            }
            TextField {
                id: customSize
                visible: sizeSel.selectedIndex === page.sizes.length - 1
                width: parent.width
                placeholderText: i18n.tr("Size in GiB, e.g. 2.5")
                inputMethodHints: Qt.ImhFormattedNumbersOnly
            }
            Label { text: i18n.tr("Format") }
            OptionSelector {
                id: fsSel
                width: parent.width
                expanded: true
                model: [i18n.tr("FAT32 (with partition table; every computer)"),
                        i18n.tr("exFAT (files over 4 GB)"),
                        i18n.tr("Empty (the computer formats or writes it)")]
            }
            Label { text: i18n.tr("Volume label") ; visible: fsSel.selectedIndex !== 2 }
            TextField {
                id: labelField
                visible: fsSel.selectedIndex !== 2
                width: parent.width
                text: "FP6DRIVE"
                maximumLength: 11
                inputMethodHints: Qt.ImhNoPredictiveText | Qt.ImhUppercaseOnly
            }
            Label {
                width: parent.width
                wrapMode: Text.Wrap
                textSize: Label.Small
                color: theme.palette.normal.backgroundTertiaryText
                text: i18n.tr("The image is sparse: it only takes space on the phone as the computer writes to it. Share it as a read-write USB disk.")
                      + (root.library.free ? " " + i18n.tr("Free: %1.").arg(Fmt.size(root.library.free)) : "")
            }
            Button {
                width: parent.width
                text: page.job !== null ? i18n.tr("Creating…") : i18n.tr("Create %1").arg(page.sizeMiB >= 1024 ? (page.sizeMiB / 1024).toFixed(1) + " GiB" : page.sizeMiB + " MiB")
                enabled: page.job === null && page.sizeMiB > 0 && nameField.acceptableInput
                color: theme.palette.normal.positive
                onClicked: {
                    var r = root.start("create", { name: nameField.text, sizeMiB: page.sizeMiB,
                                                   filesystem: page.fsKeys[fsSel.selectedIndex], label: labelField.text });
                    if (r && r.ok) page.job = r.job;
                }
            }
        }
    }
}
