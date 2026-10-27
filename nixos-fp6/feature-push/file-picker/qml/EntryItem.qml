// One file or folder row.
import QtQuick 2.12
import Lomiri.Components 1.3
import "Util.js" as Util

ListItem {
    id: item
    property string name
    property string path
    property bool isDir
    property string subtitle
    property bool checkable: false
    property bool checked: false
    signal activated()

    height: layout.height + (divider.visible ? divider.height : 0)
    color: checked ? theme.palette.highlighted.background : "transparent"
    onClicked: item.activated()

    ListItemLayout {
        id: layout
        title.text: item.name
        title.elide: Text.ElideMiddle
        subtitle.text: item.subtitle
        subtitle.visible: item.subtitle !== ""

        Icon {
            SlotsLayout.position: SlotsLayout.Leading
            width: units.gu(3.5)
            height: width
            name: item.isDir ? "folder-symbolic" : Util.iconFor(item.name)
            color: item.isDir ? theme.palette.normal.activity : theme.palette.normal.backgroundSecondaryText
        }
        CheckBox {
            SlotsLayout.position: SlotsLayout.Trailing
            visible: item.checkable
            checked: item.checked
            onTriggered: item.activated()
        }
        ProgressionSlot {
            visible: item.isDir
        }
    }
}
