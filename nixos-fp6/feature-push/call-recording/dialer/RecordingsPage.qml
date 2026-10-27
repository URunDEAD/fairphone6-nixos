/*
 * FP6 call recording: list of recorded calls (newest first), with playback
 * (both sides, my side = uplink, other side = downlink), sharing through
 * content-hub and deletion. Data and commands: fp6-callrec (CallRecorder).
 * Installed in src/qml/SettingsPage.
 */
import QtQuick 2.9
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import Lomiri.Content 1.3
import "../"

Page {
    id: page
    objectName: "callRecordingsPage"

    property alias recorder: callRecorder
    property string expandedId: ""
    readonly property var playing: callRecorder.playing

    header: PageHeader {
        id: pageHeader
        title: i18n.tr("Call recordings")
        flickable: list
        trailingActionBar.actions: [
            Action {
                objectName: "stopPlayback"
                iconName: "media-playback-stop"
                text: i18n.tr("Stop")
                visible: page.playing.id !== undefined
                onTriggered: callRecorder.stopPlayback()
            },
            Action {
                objectName: "refreshRecordings"
                iconName: "reload"
                text: i18n.tr("Refresh")
                enabled: callRecorder.available
                onTriggered: callRecorder.refresh()
            }
        ]
    }

    CallRecorder {
        id: callRecorder
    }

    function recTitle(rec) {
        if (rec.contact) return rec.contact
        if (rec.number) return rec.number
        return i18n.tr("Unknown number")
    }

    function when(iso) {
        var d = new Date(iso)
        if (isNaN(d.getTime())) return iso
        return Qt.formatDateTime(d, Qt.locale().dateTimeFormat(Locale.ShortFormat))
    }

    function sizeText(bytes) {
        if (!bytes) return ""
        if (bytes >= 1024 * 1024) return i18n.tr("%1 MB").arg((bytes / 1024 / 1024).toFixed(1))
        return i18n.tr("%1 kB").arg(Math.max(1, Math.round(bytes / 1024)))
    }

    function whichLabel(which) {
        if (which === "uplink") return i18n.tr("Playing my side")
        if (which === "downlink") return i18n.tr("Playing other side")
        return i18n.tr("Playing")
    }

    function fileUrl(path) {
        return path.indexOf("file://") === 0 ? path : "file://" + path
    }

    Component {
        id: deleteDialog
        Dialog {
            id: dialog
            property var rec
            title: i18n.tr("Delete recording?")
            text: i18n.tr("The recording of the call with %1 (%2) will be deleted from this phone.")
                    .arg(page.recTitle(rec)).arg(page.when(rec.start))
            Button {
                objectName: "confirmDelete"
                text: i18n.tr("Delete")
                color: theme.palette.normal.negative
                onClicked: {
                    if (page.playing.id === dialog.rec.id) callRecorder.stopPlayback()
                    callRecorder.remove(dialog.rec.id)
                    PopupUtils.close(dialog)
                }
            }
            Button {
                text: i18n.tr("Cancel")
                onClicked: PopupUtils.close(dialog)
            }
        }
    }

    Component {
        id: contentItemComponent
        ContentItem { }
    }

    Component {
        id: sharePage
        Page {
            id: shareP
            property var rec
            header: PageHeader { visible: false; height: 0 }
            ContentPeerPicker {
                anchors.fill: parent
                handler: ContentHandler.Share
                contentType: ContentType.All
                showTitle: true
                onPeerSelected: {
                    var transfer = peer.request()
                    var items = []
                    var files = [shareP.rec.uplink, shareP.rec.downlink]
                    for (var i = 0; i < files.length; i++) {
                        if (files[i])
                            items.push(contentItemComponent.createObject(page, { "url": page.fileUrl(files[i]) }))
                    }
                    transfer.items = items
                    transfer.state = ContentTransfer.Charged
                    pageStackNormalMode.pop()
                }
                onCancelPressed: pageStackNormalMode.pop()
            }
        }
    }

    ListView {
        id: list
        objectName: "recordingsList"
        anchors.fill: parent
        model: callRecorder.recordings
        clip: true

        delegate: ListItem {
            id: item
            objectName: "recording" + index
            property var rec: modelData
            readonly property bool isPlaying: page.playing.id === rec.id
            readonly property bool expanded: page.expandedId === rec.id || isPlaying
            height: layout.height + (expanded ? actions.height + units.gu(1.5) : 0) + (divider.visible ? divider.height : 0)
            color: isPlaying ? theme.palette.highlighted.background : "transparent"
            onClicked: page.expandedId = (page.expandedId === rec.id ? "" : rec.id)

            ListItemLayout {
                id: layout
                title.text: page.recTitle(rec)
                title.font.weight: Font.Medium
                subtitle.text: rec.contact && rec.number ? rec.number + " · " + page.when(rec.start) : page.when(rec.start)
                summary.text: (item.isPlaying ? page.whichLabel(page.playing.which) + "  ·  " : "")
                              + callRecorder.formatDuration(rec.duration) + "  ·  " + callRecorder.formatLabel(rec.format)
                              + (rec.bytes ? "  ·  " + page.sizeText(rec.bytes) : "")
                summary.color: item.isPlaying ? theme.palette.normal.activity : theme.palette.normal.backgroundSecondaryText

                Icon {
                    SlotsLayout.position: SlotsLayout.Leading
                    name: rec.direction === "in" ? "incoming-call" : "outgoing-call"
                    color: theme.palette.normal.backgroundSecondaryText
                    width: units.gu(2.5); height: width
                }

                Item {
                    SlotsLayout.position: SlotsLayout.Trailing
                    SlotsLayout.overrideVerticalPositioning: true
                    width: badge.width
                    height: layout.height
                    Rectangle {
                        id: badge
                        anchors { top: parent.top; topMargin: units.gu(1.5); right: parent.right }
                        width: badgeLabel.implicitWidth + units.gu(1.5)
                        height: badgeLabel.implicitHeight + units.gu(0.5)
                        radius: units.gu(0.5)
                        color: "transparent"
                        border.width: units.dp(1)
                        border.color: rec.path === "fallback" ? theme.palette.normal.negative
                                    : rec.path === "volte" ? theme.palette.normal.positive
                                    : theme.palette.normal.backgroundSecondaryText
                        Label {
                            id: badgeLabel
                            anchors.centerIn: parent
                            textSize: Label.XSmall
                            text: callRecorder.pathLabel(rec.path)
                            color: badge.border.color
                        }
                    }
                }
            }

            Grid {
                id: actions
                visible: item.expanded
                anchors { left: parent.left; right: parent.right; top: layout.bottom; leftMargin: units.gu(2); rightMargin: units.gu(2) }
                columns: 3
                spacing: units.gu(1)
                readonly property real buttonWidth: (width - 2 * spacing) / 3

                Button {
                    objectName: "playBoth"
                    width: actions.buttonWidth
                    text: i18n.tr("Play")
                    color: item.isPlaying && page.playing.which === "both" ? theme.palette.normal.positive : theme.palette.normal.base
                    enabled: callRecorder.available
                    onClicked: callRecorder.play(rec.id, "both")
                }
                Button {
                    objectName: "playUplink"
                    width: actions.buttonWidth
                    text: i18n.tr("My side")
                    color: item.isPlaying && page.playing.which === "uplink" ? theme.palette.normal.positive : theme.palette.normal.base
                    enabled: callRecorder.available && !!rec.uplink
                    onClicked: callRecorder.play(rec.id, "uplink")
                }
                Button {
                    objectName: "playDownlink"
                    width: actions.buttonWidth
                    text: i18n.tr("Other side")
                    color: item.isPlaying && page.playing.which === "downlink" ? theme.palette.normal.positive : theme.palette.normal.base
                    enabled: callRecorder.available && !!rec.downlink
                    onClicked: callRecorder.play(rec.id, "downlink")
                }
                Button {
                    objectName: "stopOne"
                    width: actions.buttonWidth
                    text: i18n.tr("Stop")
                    enabled: item.isPlaying
                    onClicked: callRecorder.stopPlayback()
                }
                Button {
                    objectName: "share"
                    width: actions.buttonWidth
                    text: i18n.tr("Share")
                    onClicked: pageStackNormalMode.push(sharePage, { "rec": rec })
                }
                Button {
                    objectName: "delete"
                    width: actions.buttonWidth
                    text: i18n.tr("Delete")
                    color: theme.palette.normal.negative
                    enabled: callRecorder.available
                    onClicked: PopupUtils.open(deleteDialog, page, { "rec": rec })
                }
            }

            leadingActions: ListItemActions {
                actions: [
                    Action {
                        iconName: "delete"
                        text: i18n.tr("Delete")
                        enabled: callRecorder.available
                        onTriggered: PopupUtils.open(deleteDialog, page, { "rec": item.rec })
                    }
                ]
            }
            trailingActions: ListItemActions {
                actions: [
                    Action {
                        iconName: item.isPlaying ? "media-playback-stop" : "media-playback-start"
                        text: item.isPlaying ? i18n.tr("Stop") : i18n.tr("Play")
                        enabled: callRecorder.available
                        onTriggered: item.isPlaying ? callRecorder.stopPlayback() : callRecorder.play(item.rec.id, "both")
                    },
                    Action {
                        iconName: "share"
                        text: i18n.tr("Share")
                        onTriggered: pageStackNormalMode.push(sharePage, { "rec": item.rec })
                    }
                ]
            }
        }
    }

    // empty / service missing
    Column {
        anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; margins: units.gu(4) }
        spacing: units.gu(1.5)
        visible: list.count === 0

        Icon {
            anchors.horizontalCenter: parent.horizontalCenter
            name: callRecorder.available ? "media-record" : "dialog-warning-symbolic"
            width: units.gu(6); height: width
            color: theme.palette.normal.backgroundTertiaryText
        }
        Label {
            objectName: "emptyLabel"
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            textSize: Label.Large
            text: callRecorder.available ? i18n.tr("No recordings yet") : i18n.tr("Call recorder service is not running")
        }
        Label {
            width: parent.width
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
            color: theme.palette.normal.backgroundSecondaryText
            text: !callRecorder.available ? i18n.tr("Start it with: systemctl --user start fp6-callrec")
                : callRecorder.mode === "off" && !callRecorder.debugRecordAll
                    ? i18n.tr("Call recording is off. Turn it on in Settings > Call recording.")
                    : callRecorder.mode === "manual" ? i18n.tr("Tap the Record button during a call to record it.")
                    : i18n.tr("Calls are recorded automatically.")
        }
    }
}
