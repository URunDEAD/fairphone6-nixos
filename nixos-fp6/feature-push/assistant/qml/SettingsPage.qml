// Settings: models and keys, default model, local-only, tools and
// confirmations, permissions, e-mail accounts, history, instructions.
import QtQuick 2.12
import Lomiri.Components 1.3
import Lomiri.Components.Popups 1.3

Page {
    id: page
    property var s: null
    property var keys: ({})
    property string keyring: ""
    property string localStatus: ""

    function reload() {
        root.callJson("GetSettings", [], function(v) { if (v) { page.s = v.settings; page.keys = v.keys; page.keyring = v.keyring } })
        root.call("LocalStatus", [], function(r) { page.localStatus = r || "" })
    }
    function save(mutate) {
        var copy = JSON.parse(JSON.stringify(s))
        mutate(copy)
        root.call("SetSettings", [JSON.stringify(copy)], function(r, err) {
            if (err) root.showError(err)
            page.reload()
        })
    }
    function backendName(id) {
        if (!s) return ""
        for (var i = 0; i < s.backends.length; i++) if (s.backends[i].id === id) return s.backends[i].name
        return id
    }
    Component.onCompleted: reload()
    onVisibleChanged: if (visible) reload()

    header: PageHeader { id: header; title: i18n.tr("Assistant settings") }

    Flickable {
        anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
        contentHeight: col.height
        clip: true
        Column {
            id: col
            width: parent.width
            visible: page.s !== null

            ListItem { height: units.gu(5); Label { x: units.gu(2); anchors.verticalCenter: parent.verticalCenter; text: i18n.tr("Models"); font.bold: true } }
            Repeater {
                model: page.s ? page.s.backends : []
                ListItem {
                    height: bl.height
                    onClicked: stack.push(Qt.resolvedUrl("BackendPage.qml"), {backend: modelData, hasKey: !!page.keys["backend:" + modelData.id], settingsPage: page})
                    ListItemLayout {
                        id: bl
                        title.text: modelData.name + (page.s.default_backend === modelData.id ? "  ·  " + i18n.tr("default") : "")
                        subtitle.text: modelData.kind === "local" ? (page.localStatus ? i18n.tr("On-device model service: ready") : i18n.tr("On-device model service not found; set a URL"))
                                     : (modelData.model || i18n.tr("no model chosen")) + " · " + (modelData.base_url || "")
                        summary.text: modelData.kind === "local" ? i18n.tr("Stays on the phone")
                                    : (modelData.kind === "anthropic" || modelData.base_url.indexOf("127.0.0.1") < 0 ? i18n.tr("Sends chats off the phone") : "")
                        ProgressionSlot {}
                    }
                }
            }
            ListItem {
                height: units.gu(6)
                onClicked: stack.push(Qt.resolvedUrl("BackendPage.qml"), {backend: {id: "", name: "", kind: "openai", base_url: "", model: "", effort: "", models: []}, hasKey: false, settingsPage: page})
                ListItemLayout { title.text: i18n.tr("Add a model service…"); Icon { SlotsLayout.position: SlotsLayout.Leading; name: "add"; width: units.gu(2.5); height: width } }
            }
            ListItem {
                height: ll.height
                ListItemLayout {
                    id: ll
                    title.text: i18n.tr("Local only")
                    subtitle.text: i18n.tr("Never send anything to a model that is not on this phone")
                    subtitle.maximumLineCount: 3
                    Switch { SlotsLayout.position: SlotsLayout.Trailing; checked: page.s ? page.s.local_only : false
                             onClicked: page.save(function(c) { c.local_only = checked }) }
                }
            }

            ListItem { height: units.gu(5); Label { x: units.gu(2); anchors.verticalCenter: parent.verticalCenter; text: i18n.tr("Privacy and safety"); font.bold: true } }
            ListItem {
                height: tl.height
                onClicked: stack.push(Qt.resolvedUrl("ToolsPage.qml"), {settingsPage: page})
                ListItemLayout { id: tl; title.text: i18n.tr("Tools and confirmations"); subtitle.text: i18n.tr("What the assistant may use, and which actions you confirm"); ProgressionSlot {} }
            }
            ListItem {
                height: pl.height
                onClicked: stack.push(Qt.resolvedUrl("PermissionsPage.qml"))
                ListItemLayout { id: pl; title.text: i18n.tr("Always allowed"); subtitle.text: i18n.tr("Resources you allowed for every chat"); ProgressionSlot {} }
            }
            ListItem {
                height: kl.height
                ListItemLayout { id: kl; title.text: i18n.tr("Keys are stored in"); subtitle.text: page.keyring === "keyring" ? i18n.tr("the keyring") : i18n.tr("a private file (no keyring service found)") }
            }

            ListItem { height: units.gu(5); Label { x: units.gu(2); anchors.verticalCenter: parent.verticalCenter; text: i18n.tr("E-mail"); font.bold: true } }
            Repeater {
                model: page.s ? page.s.email_accounts : []
                ListItem {
                    height: el.height
                    onClicked: stack.push(Qt.resolvedUrl("EmailPage.qml"), {account: modelData, hasKey: !!page.keys["email:" + modelData.id], settingsPage: page})
                    ListItemLayout { id: el; title.text: modelData.name; subtitle.text: modelData.address; ProgressionSlot {} }
                }
            }
            ListItem {
                height: units.gu(6)
                onClicked: stack.push(Qt.resolvedUrl("EmailPage.qml"), {account: null, hasKey: false, settingsPage: page})
                ListItemLayout { title.text: i18n.tr("Add an e-mail account…"); Icon { SlotsLayout.position: SlotsLayout.Leading; name: "add"; width: units.gu(2.5); height: width } }
            }

            ListItem { height: units.gu(5); Label { x: units.gu(2); anchors.verticalCenter: parent.verticalCenter; text: i18n.tr("History"); font.bold: true } }
            OptionSelector {
                id: retention
                x: units.gu(2)
                width: parent.width - units.gu(4)
                text: i18n.tr("Keep chats")
                readonly property var days: [7, 30, 90, 365, 0]
                model: [i18n.tr("1 week"), i18n.tr("1 month"), i18n.tr("3 months"), i18n.tr("1 year"), i18n.tr("Forever")]
                selectedIndex: page.s ? Math.max(0, days.indexOf(page.s.retention_days)) : 2
                onDelegateClicked: page.save(function(c) { c.retention_days = retention.days[index] })
            }
            Item { width: 1; height: units.gu(1) }
            Button {
                x: units.gu(2)
                text: i18n.tr("Delete all chats")
                color: theme.palette.normal.negative
                onClicked: PopupUtils.open(clearDialog)
            }
            Item { width: 1; height: units.gu(2) }

            ListItem { height: units.gu(5); Label { x: units.gu(2); anchors.verticalCenter: parent.verticalCenter; text: i18n.tr("Your instructions"); font.bold: true } }
            TextArea {
                id: instr
                x: units.gu(2)
                width: parent.width - units.gu(4)
                autoSize: true
                maximumLineCount: 8
                placeholderText: i18n.tr("e.g. Answer in Romanian. Call me Paul.")
                text: page.s ? page.s.custom_instructions : ""
            }
            Button {
                x: units.gu(2)
                text: i18n.tr("Save instructions")
                onClicked: page.save(function(c) { c.custom_instructions = instr.text })
            }
            Item { width: 1; height: units.gu(2) }
            ListItem {
                height: lim.height
                ListItemLayout {
                    id: lim
                    title.text: i18n.tr("Limits")
                    subtitle.maximumLineCount: 4
                    subtitle.text: page.s ? i18n.tr("Up to %1 steps and %2 tool calls per message, %3 actions per 10 minutes, %4 permission questions per message.")
                                             .arg(page.s.limits.max_steps).arg(page.s.limits.tool_calls_per_turn).arg(page.s.limits.actions_per_10_min).arg(page.s.limits.prompts_per_turn) : ""
                }
            }
            Item { width: 1; height: units.gu(4) }
        }
    }
    Component {
        id: clearDialog
        Dialog {
            id: cd
            title: i18n.tr("Delete all chats?")
            text: i18n.tr("Every conversation and its activity log is deleted. Always-allowed permissions stay.")
            Button { text: i18n.tr("Delete"); color: theme.palette.normal.negative
                     onClicked: { root.call("ClearHistory", []); PopupUtils.close(cd) } }
            Button { text: i18n.tr("Cancel"); onClicked: PopupUtils.close(cd) }
        }
    }
}
