// "Allow the assistant to …?" — Allow once / Allow for this chat /
// Always allow / Deny. Always is hidden for the most sensitive resources
// and after the assistant read untrusted content in this turn.
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3

Dialog {
    id: dialog
    objectName: "permissionDialog"
    property var prompt: ({})
    signal answered(string answer)

    title: i18n.tr("Allow the assistant: %1?").arg(prompt.title || "")
    text: (prompt.explain || "") + (prompt.tool_title ? "\n" + i18n.tr("Needed for: %1").arg(prompt.tool_title) : "")

    Label {
        visible: !!prompt.scope
        width: parent.width
        wrapMode: Text.Wrap
        font.bold: true
        text: i18n.tr("Folder: %1").arg(prompt.scope || "")
    }
    Label {
        visible: prompt.tainted && prompt.tainted.length > 0
        width: parent.width
        wrapMode: Text.Wrap
        color: theme.palette.normal.negative
        text: i18n.tr("Careful: this request comes after the assistant read content from outside (%1). Such content can try to trick it. Allow only if you asked for this.").arg((prompt.tainted || []).join(", "))
    }
    Label {
        visible: prompt.remote === true
        width: parent.width
        wrapMode: Text.Wrap
        textSize: Label.Small
        text: i18n.tr("This chat uses a cloud model: what the tool returns is sent to %1.").arg(prompt.provider || "")
    }
    Button {
        objectName: "allowOnce"
        text: i18n.tr("Allow once")
        color: theme.palette.normal.positive
        onClicked: dialog.answered("once")
    }
    Button {
        objectName: "allowChat"
        text: i18n.tr("Allow for this chat")
        onClicked: dialog.answered("chat")
    }
    Button {
        objectName: "allowAlways"
        visible: prompt.allow_always === true
        text: i18n.tr("Always allow")
        onClicked: dialog.answered("always")
    }
    Button {
        objectName: "deny"
        text: i18n.tr("Deny")
        onClicked: dialog.answered("deny")
    }
}
