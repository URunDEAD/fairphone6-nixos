/*
 * FP6 call recording: Phone > Settings > Call recording.
 * Installed in src/qml/SettingsPage.
 */
import QtQuick 2.9
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3
import Lomiri.Components.ListItems 1.3 as ListItems
import Lomiri.Telephony 0.1
import "../"

Page {
    id: page
    objectName: "callRecordingSettingsPage"

    property alias recorder: callRecorder

    header: PageHeader {
        id: pageHeader
        title: i18n.tr("Call recording")
        flickable: flick
    }

    CallRecorder {
        id: callRecorder
    }

    Component {
        id: noticeComponent
        CallRecordingNotice { }
    }

    // Mode changes go through here: switching recording on for the first
    // time needs the notice to be accepted; Cancel keeps it off.
    function chooseMode(m) {
        if (m === callRecorder.mode) return
        if (m !== "off" && !callRecorder.noticeAccepted) {
            var dialog = PopupUtils.open(noticeComponent, page)
            dialog.accepted.connect(function() {
                callRecorder.acceptNotice()
                callRecorder.setMode(m)
                modeSelector.selectedIndex = Qt.binding(function() { return callRecorder.modes.indexOf(callRecorder.mode) })
            })
            dialog.cancelled.connect(function() {
                callRecorder.setMode("off")
                modeSelector.selectedIndex = Qt.binding(function() { return callRecorder.modes.indexOf(callRecorder.mode) })
            })
            return
        }
        callRecorder.setMode(m)
    }

    function addTypedNumber() {
        if (callRecorder.addNumber(numberField.text))
            numberField.text = ""
    }

    Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: width
        contentHeight: column.height + units.gu(4)
        boundsBehavior: (contentHeight > height) ? Flickable.DragAndOvershootBounds : Flickable.StopAtBounds

        Column {
            id: column
            anchors { left: parent.left; right: parent.right }

            // service missing
            Item {
                visible: !callRecorder.available
                width: parent.width
                height: visible ? serviceLabel.height + units.gu(2) : 0
                Icon {
                    id: serviceIcon
                    name: "dialog-warning-symbolic"
                    color: theme.palette.normal.negative
                    width: units.gu(2.5); height: width
                    anchors { left: parent.left; leftMargin: units.gu(2); verticalCenter: parent.verticalCenter }
                }
                Label {
                    id: serviceLabel
                    anchors { left: serviceIcon.right; leftMargin: units.gu(1); right: parent.right; rightMargin: units.gu(2); verticalCenter: parent.verticalCenter }
                    wrapMode: Text.WordWrap
                    textSize: Label.Small
                    text: i18n.tr("Call recorder service is not running. Settings are saved and apply once it starts.")
                }
            }

            ListItems.Header { text: i18n.tr("Record calls") }

            OptionSelector {
                id: modeSelector
                objectName: "recordingMode"
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                expanded: true
                model: [ i18n.tr("Off"),
                         i18n.tr("Manual (Record button)"),
                         i18n.tr("Automatically: all calls"),
                         i18n.tr("Automatically: chosen contacts") ]
                selectedIndex: callRecorder.modes.indexOf(callRecorder.mode)
                onDelegateClicked: page.chooseMode(callRecorder.modes[index])
            }

            // chosen numbers (auto-contacts)
            Column {
                id: numbersSection
                objectName: "autoContacts"
                visible: callRecorder.mode === "auto-contacts"
                anchors { left: parent.left; right: parent.right }
                topPadding: units.gu(1)

                ListItems.Header { text: i18n.tr("Numbers recorded automatically") }

                Label {
                    visible: callRecorder.autoContacts.length === 0
                    anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                    height: visible ? implicitHeight + units.gu(2) : 0
                    verticalAlignment: Text.AlignVCenter
                    wrapMode: Text.WordWrap
                    color: theme.palette.normal.backgroundSecondaryText
                    text: i18n.tr("No numbers yet. Add one below.")
                }

                Repeater {
                    model: callRecorder.autoContacts
                    delegate: ListItem {
                        id: numberItem
                        objectName: "autoContact" + index
                        height: numberLayout.height + (divider.visible ? divider.height : 0)

                        ContactWatcher {
                            id: watcher
                            identifier: modelData
                            addressableFields: ["tel"]
                        }

                        ListItemLayout {
                            id: numberLayout
                            title.text: watcher.alias !== "" ? watcher.alias : modelData
                            subtitle.text: watcher.alias !== "" ? modelData : i18n.tr("Not in contacts")

                            Icon {
                                SlotsLayout.position: SlotsLayout.Leading
                                name: "contact"
                                width: units.gu(3); height: width
                            }
                            AbstractButton {
                                objectName: "removeNumber"
                                SlotsLayout.position: SlotsLayout.Trailing
                                width: units.gu(4); height: units.gu(4)
                                onClicked: callRecorder.removeNumber(modelData)
                                Icon {
                                    anchors.centerIn: parent
                                    name: "close"
                                    width: units.gu(2); height: width
                                    color: theme.palette.normal.backgroundSecondaryText
                                }
                            }
                        }

                        leadingActions: ListItemActions {
                            actions: [
                                Action {
                                    iconName: "delete"
                                    text: i18n.tr("Remove")
                                    onTriggered: callRecorder.removeNumber(modelData)
                                }
                            ]
                        }
                    }
                }

                Item {
                    anchors { left: parent.left; right: parent.right }
                    height: units.gu(7)

                    TextField {
                        id: numberField
                        objectName: "numberField"
                        anchors { left: parent.left; leftMargin: units.gu(2); right: addButton.left; rightMargin: units.gu(1); verticalCenter: parent.verticalCenter }
                        placeholderText: i18n.tr("Phone number")
                        inputMethodHints: Qt.ImhDialableCharactersOnly
                        onAccepted: page.addTypedNumber()
                    }
                    Button {
                        id: addButton
                        objectName: "addNumber"
                        anchors { right: parent.right; rightMargin: units.gu(2); verticalCenter: parent.verticalCenter }
                        width: units.gu(9)
                        text: i18n.tr("Add")
                        enabled: numberField.text.trim() !== ""
                        onClicked: page.addTypedNumber()
                    }
                }

                ListItem {
                    objectName: "addFromContacts"
                    height: pickLayout.height + (divider.visible ? divider.height : 0)
                    onClicked: {
                        var picker = pageStackNormalMode.push(Qt.resolvedUrl("CallRecordingContactPicker.qml"))
                        picker.numberPicked.connect(function(number) {
                            callRecorder.addNumber(number)
                            pageStackNormalMode.pop()
                        })
                    }
                    ListItemLayout {
                        id: pickLayout
                        title.text: i18n.tr("Add from contacts")
                        Icon {
                            SlotsLayout.position: SlotsLayout.Leading
                            name: "contact-new"
                            width: units.gu(3); height: width
                        }
                        ProgressionSlot { }
                    }
                }
            }

            ListItems.Header { text: i18n.tr("Recording") }

            OptionSelector {
                objectName: "recordingFormat"
                anchors { left: parent.left; right: parent.right; margins: units.gu(2) }
                text: i18n.tr("File format")
                model: [ i18n.tr("FLAC (lossless, compressed)"),
                         i18n.tr("WAV (uncompressed, large)"),
                         i18n.tr("Opus (small)") ]
                selectedIndex: callRecorder.formats.indexOf(callRecorder.format)
                onDelegateClicked: callRecorder.settings.format = callRecorder.formats[index]
            }

            Item { width: 1; height: units.gu(1) }

            ListItem {
                objectName: "announce"
                height: announceLayout.height + (divider.visible ? divider.height : 0)
                onClicked: callRecorder.settings.announce = !callRecorder.announce
                ListItemLayout {
                    id: announceLayout
                    title.text: i18n.tr("Announce recording")
                    subtitle.text: i18n.tr("Plays a short tone into the call when recording starts. On regular (non-VoLTE) calls it may only be heard on this phone.")
                    subtitle.maximumLineCount: 4
                    subtitle.wrapMode: Text.WordWrap
                    Switch {
                        id: announceSwitch
                        SlotsLayout.position: SlotsLayout.Trailing
                        checked: callRecorder.announce
                        onTriggered: callRecorder.settings.announce = checked
                    }
                }
            }

            ListItem {
                objectName: "debugRecordAll"
                height: debugLayout.height + (divider.visible ? divider.height : 0)
                onClicked: callRecorder.settings.debugRecordAll = !callRecorder.debugRecordAll
                ListItemLayout {
                    id: debugLayout
                    title.text: i18n.tr("Record all calls (test sessions)")
                    subtitle.text: i18n.tr("Records every call whatever the setting above, without asking. For testing only; switch it off afterwards.")
                    subtitle.color: callRecorder.debugRecordAll ? theme.palette.normal.negative : theme.palette.normal.backgroundSecondaryText
                    subtitle.maximumLineCount: 4
                    subtitle.wrapMode: Text.WordWrap
                    Icon {
                        SlotsLayout.position: SlotsLayout.Leading
                        name: "dialog-warning-symbolic"
                        color: theme.palette.normal.negative
                        width: units.gu(2.5); height: width
                    }
                    Switch {
                        id: debugSwitch
                        objectName: "debugSwitch"
                        SlotsLayout.position: SlotsLayout.Trailing
                        checked: callRecorder.debugRecordAll
                        onTriggered: callRecorder.settings.debugRecordAll = checked
                    }
                }
            }

            ListItem {
                objectName: "recordings"
                height: recLayout.height + (divider.visible ? divider.height : 0)
                onClicked: pageStackNormalMode.push(Qt.resolvedUrl("RecordingsPage.qml"))
                ListItemLayout {
                    id: recLayout
                    title.text: i18n.tr("Recordings")
                    subtitle.text: !callRecorder.available ? i18n.tr("Call recorder service is not running")
                                 : i18n.tr("%1 recording", "%1 recordings", callRecorder.recordings.length).arg(callRecorder.recordings.length)
                    Icon {
                        SlotsLayout.position: SlotsLayout.Leading
                        name: "media-record"
                        width: units.gu(3); height: width
                    }
                    ProgressionSlot { }
                }
            }
        }
    }
}
