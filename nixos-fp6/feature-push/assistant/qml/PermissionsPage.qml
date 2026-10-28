// "Always allowed" resources (revocable). Grants for one chat are in that
// chat's activity page. The Assistant app's own system permissions are in
// System Settings > Apps.
import QtQuick 2.12
import Lomiri.Components 1.3

Page {
    id: page
    property var grants: []
    function reload() { root.callJson("Grants", [], function(v) { if (v) page.grants = v.always }) }
    Component.onCompleted: reload()
    header: PageHeader { id: header; title: i18n.tr("Always allowed") }
    ListView {
        anchors { top: header.bottom; left: parent.left; right: parent.right; bottom: parent.bottom }
        model: page.grants
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
                text: page.grants.length ? i18n.tr("The assistant may use these in every chat without asking. Actions are still confirmed.")
                                     : i18n.tr("Nothing is always allowed. The assistant asks the first time it needs something in a chat.")
            }
        }
        delegate: ListItem {
            height: l.height
            ListItemLayout {
                id: l
                title.text: modelData.title
                subtitle.text: i18n.tr("since %1").arg(Qt.formatDate(new Date(modelData.since * 1000)))
                Button { SlotsLayout.position: SlotsLayout.Trailing; text: i18n.tr("Revoke"); width: units.gu(10)
                         onClicked: root.call("RevokeGrant", [modelData.key], function() { page.reload() }) }
            }
        }
    }
}
