// The hosting status at the top of the library: image, mode, connection,
// transfer counters and the Stop button.
import QtQuick 2.12
import Lomiri.Components 1.3
import "Fmt.js" as Fmt

Rectangle {
    id: card
    property var st: root.status
    width: parent ? parent.width : units.gu(40)
    height: col.height + units.gu(4)
    radius: units.gu(1)
    color: theme.palette.normal.foreground
    border.color: theme.palette.normal.positive
    border.width: units.dp(1)

    Column {
        id: col
        anchors { left: parent.left; right: parent.right; top: parent.top; margins: units.gu(2) }
        spacing: units.gu(1)
        Row {
            width: parent.width
            spacing: units.gu(1.5)
            Icon {
                id: ic
                name: st.mode === "cdrom" ? "media-optical-symbolic" : "drive-removable-symbolic"
                width: units.gu(4); height: width
                color: theme.palette.normal.positive
            }
            Column {
                width: parent.width - ic.width - parent.spacing
                Label {
                    width: parent.width
                    text: i18n.tr("Sharing %1").arg(st.name || "")
                    textSize: Label.Large
                    elide: Text.ElideMiddle
                    color: theme.palette.normal.foregroundText
                }
                Label {
                    width: parent.width
                    text: Fmt.modeName(st.mode) + (st.exclusive ? " · " + i18n.tr("exclusive") : "")
                    textSize: Label.Small
                    color: theme.palette.normal.backgroundTertiaryText
                }
            }
        }
        Row {
            spacing: units.gu(1)
            ActivityIndicator {
                running: st.active === true
                visible: running
                width: units.gu(2); height: width
            }
            Icon {
                visible: st.active !== true
                name: st.hostConnected ? "tick" : "stock_usb"
                width: units.gu(2); height: width
                color: st.hostConnected ? theme.palette.normal.positive : theme.palette.normal.backgroundTertiaryText
            }
            Label {
                text: st.active ? i18n.tr("Transferring %1").arg(Fmt.rate((st.readRate || 0) + (st.writeRate || 0)))
                      : st.hostConnected ? i18n.tr("Computer connected")
                      : i18n.tr("Waiting for a computer on the USB cable")
                color: theme.palette.normal.foregroundText
            }
        }
        Label {
            width: parent.width
            text: i18n.tr("Read %1").arg(Fmt.size(st.readBytes || 0))
                  + ((st.writtenBytes || 0) > 0 ? " · " + i18n.tr("written %1").arg(Fmt.size(st.writtenBytes)) : "")
            textSize: Label.Small
            color: theme.palette.normal.backgroundTertiaryText
        }
        Label {
            width: parent.width
            visible: st.exclusive === true
            wrapMode: Text.Wrap
            textSize: Label.Small
            color: theme.palette.normal.negative
            text: i18n.tr("USB networking and ADB are off. They come back when you stop or unplug the cable.")
        }
        Button {
            width: parent.width
            text: root.busy ? i18n.tr("Working…") : i18n.tr("Stop sharing")
            enabled: !root.busy
            color: theme.palette.normal.negative
            onClicked: root.start("stop")
        }
    }
}
