// Per tool: on/off, and for actions (send, call, write, toggle...) whether
// each one is confirmed. Confirmations come back anyway while the turn has
// read untrusted content.
import QtQuick 2.12
import Lomiri.Components 1.3

Page {
    id: page
    property var settingsPage: null
    property var tools: []
    function reload() { root.callJson("Tools", [], function(v) { if (v) page.tools = v }) }
    Component.onCompleted: reload()
    function setPolicy(name, policy) {
        settingsPage.save(function(c) {
            if (policy === "default") delete c.tool_policies[name]
            else c.tool_policies[name] = policy
        })
        reloadTimer.restart()
    }
    Timer { id: reloadTimer; interval: 300; onTriggered: page.reload() }

    header: PageHeader { id: header; title: i18n.tr("Tools and confirmations") }
    ListView {
        id: list
        anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
        clip: true
        model: page.tools.slice().sort(function(a, b) { return a.group === b.group ? 0 : (a.group < b.group ? -1 : 1) })
        header: Item {
            width: parent ? parent.width : 0
            height: hl.height + units.gu(3)
            Label {
                id: hl
                x: units.gu(2)
                y: units.gu(1.5)
                width: parent.width - units.gu(4)
                wrapMode: Text.Wrap
                textSize: Label.Small
                text: i18n.tr("Actions are confirmed one by one unless you switch “Ask first” off. After the assistant reads messages, e-mail, web pages or files, it always asks again for the rest of that reply.")
            }
        }
        delegate: Column {
            width: list.width
            readonly property bool firstOfGroup: index === 0 || list.model[index - 1].group !== modelData.group
            ListItem {
                visible: firstOfGroup
                height: visible ? units.gu(5) : 0
                Label { x: units.gu(2); anchors.verticalCenter: parent.verticalCenter; text: modelData.group; font.bold: true }
            }
            ListItem {
                height: tl.height + (askRow.visible ? askRow.height : 0)
                ListItemLayout {
                    id: tl
                    title.text: modelData.title
                    subtitle.text: modelData.policy === "disabled" ? i18n.tr("Off")
                                 : modelData.side_effect ? (modelData.policy === "relaxed" ? i18n.tr("Runs without asking") : i18n.tr("Asks before every action"))
                                 : (modelData.untrusted ? i18n.tr("On · reads outside content") : i18n.tr("On"))
                    Switch {
                        objectName: "toolSwitch_" + modelData.name
                        SlotsLayout.position: SlotsLayout.Trailing
                        checked: modelData.policy !== "disabled"
                        onClicked: page.setPolicy(modelData.name, checked ? "default" : "disabled")
                    }
                }
                Row {
                    id: askRow
                    visible: modelData.side_effect && modelData.policy !== "disabled"
                    anchors { top: tl.bottom; left: parent.left; leftMargin: units.gu(2) }
                    height: units.gu(4)
                    spacing: units.gu(1)
                    CheckBox {
                        anchors.verticalCenter: parent.verticalCenter
                        checked: modelData.policy !== "relaxed"
                        onClicked: page.setPolicy(modelData.name, checked ? "default" : "relaxed")
                    }
                    Label { anchors.verticalCenter: parent.verticalCenter; text: i18n.tr("Ask first"); textSize: Label.Small }
                }
            }
        }
    }
}
