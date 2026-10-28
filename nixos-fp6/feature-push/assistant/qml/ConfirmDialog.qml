// Per-action confirmation: shows exactly what will be sent / changed.
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3

Dialog {
    id: dialog
    objectName: "confirmDialog"
    property var prompt: ({})
    signal answered(string answer)

    title: prompt.title || i18n.tr("Confirm")

    Repeater {
        model: prompt.fields || []
        Column {
            width: parent ? parent.width : 0
            spacing: units.gu(0.25)
            Label {
                text: modelData.label
                textSize: Label.Small
                color: theme.palette.normal.backgroundSecondaryText
            }
            TextEdit {
                width: parent.width
                text: modelData.value
                readOnly: true
                selectByMouse: true
                wrapMode: Text.Wrap
                color: theme.palette.normal.baseText
                font.pixelSize: units.gu(2)
                font.bold: modelData.label === "To" || modelData.label === "Call"
            }
        }
    }
    Label {
        visible: prompt.tainted && prompt.tainted.length > 0
        width: parent.width
        wrapMode: Text.Wrap
        color: theme.palette.normal.negative
        text: i18n.tr("The assistant read content from outside before this (%1). Check that this is what you asked for.").arg((prompt.tainted || []).join(", "))
    }
    Button {
        objectName: "confirmAction"
        text: prompt.action || i18n.tr("OK")
        color: prompt.destructive ? theme.palette.normal.negative : theme.palette.normal.positive
        onClicked: dialog.answered("confirm")
    }
    Button {
        objectName: "cancelAction"
        text: i18n.tr("Cancel")
        onClicked: dialog.answered("cancel")
    }
}
