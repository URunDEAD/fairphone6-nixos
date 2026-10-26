// Running and finished background tasks (checksum, decompress, create,
// download), with progress and cancel / dismiss.
import QtQuick 2.12
import Lomiri.Components 1.3
import "Fmt.js" as Fmt

Column {
    id: jc
    property string filterKind: ""
    width: parent ? parent.width : units.gu(40)
    Repeater {
        model: root.jobs.filter(function (j) { return j.kind !== "host" && (jc.filterKind === "" || j.kind === jc.filterKind) })
        delegate: ListItem {
            height: lay.height + (bar.visible ? bar.height + units.gu(1) : 0)
            divider.visible: true
            ListItemLayout {
                id: lay
                title.text: modelData.title
                subtitle.text: modelData.status === "running"
                    ? (modelData.total > 0 ? Fmt.size(modelData.done) + " / " + Fmt.size(modelData.total)
                                           : Fmt.size(modelData.done))
                    : modelData.message
                subtitle.color: modelData.status === "failed" ? theme.palette.normal.negative
                              : modelData.status === "done" ? theme.palette.normal.positive
                              : theme.palette.normal.backgroundSecondaryText
                subtitle.maximumLineCount: 3
                subtitle.wrapMode: Text.Wrap
                summary.text: modelData.kind === "verify" && modelData.result && modelData.result.sha256
                              ? "SHA-256 " + Fmt.shortHash(modelData.result.sha256) : ""
                Icon {
                    SlotsLayout.position: SlotsLayout.Leading
                    width: units.gu(3); height: width
                    name: modelData.status === "running" ? "sync"
                        : modelData.status === "done" ? "tick"
                        : modelData.status === "cancelled" ? "cancel" : "security-alert"
                    color: modelData.status === "failed" ? theme.palette.normal.negative
                         : modelData.status === "done" ? theme.palette.normal.positive
                         : theme.palette.normal.backgroundText
                }
                Button {
                    SlotsLayout.position: SlotsLayout.Trailing
                    width: units.gu(5)
                    iconName: modelData.status === "running" ? "close" : "clear"
                    color: "transparent"
                    onClicked: {
                        root.backend_cmd(modelData.status === "running" ? "cancel" : "dismiss", { job: modelData.id });
                    }
                }
            }
            ProgressBar {
                id: bar
                visible: modelData.status === "running"
                anchors { left: parent.left; right: parent.right; bottom: parent.bottom; margins: units.gu(2); bottomMargin: units.gu(1) }
                indeterminate: modelData.fraction < 0
                minimumValue: 0; maximumValue: 1
                value: Math.max(0, modelData.fraction)
                showProgressPercentage: false
            }
        }
    }
}
