/*
 * FP6 call recording: first-use notice. Shown before the first recording
 * (call screen) or when recording is first switched on (Settings).
 * open with PopupUtils.open(component) and connect accepted()/cancelled().
 */
import QtQuick 2.4
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3

Dialog {
    id: dialog
    objectName: "callRecordingNotice"

    signal accepted()
    signal cancelled()

    title: i18n.tr("Recording calls")
    text: i18n.tr("Recording laws differ between countries and states. Some require everyone on the call to agree to it.") + "\n\n" +
          i18n.tr("You are responsible for telling the other party that the call is being recorded.") + "\n\n" +
          i18n.tr("Recordings are stored only on this phone, in ~/Recordings/Calls.")

    Button {
        objectName: "acceptNotice"
        text: i18n.tr("I understand")
        color: theme.palette.normal.positive
        onClicked: {
            dialog.accepted()
            PopupUtils.close(dialog)
        }
    }
    Button {
        objectName: "cancelNotice"
        text: i18n.tr("Cancel")
        onClicked: {
            dialog.cancelled()
            PopupUtils.close(dialog)
        }
    }
}
